# Automated macOS releases from Linux

`rust_realease` can build `aarch64-apple-darwin` and
`x86_64-apple-darwin` on a Linux release host. Apple targets use a pinned
`cargo-zigbuild` container; Linux and Windows targets continue to use Cross.
One invocation may mix both backends and feeds every result into the existing
archive, standalone binary, checksum, collision, upload, and download
verification pipeline.

This is compile and packaging support for separate Mach-O executables. It does
not create universal binaries or `.app` bundles, sign with Developer ID,
notarize, or support macOS below 13.0.

## Agent workflow

1. Add either or both supported Apple triples to `build.targets`. Apple targets
   are opt-in; existing configurations retain their current target matrix.
2. Keep project-specific Linux/Windows settings in `Cross.toml` or the existing
   `CROSS_CONFIG`. Apple targets do not use Cross configuration or require
   Darwin Cross images.
3. Run `rust_realease SOURCE --package PACKAGE --dry-run`. Confirm each target's
   printed backend, Apple image digest, SDK path, deployment minimum, artifact
   names, and release destination. Dry-run does not start containers, compile,
   create artifacts, read a publication token, or contact a release API.
4. Run the project's normal token-free compile-only command. This builds every
   selected target and packages all outputs before returning success. For
   `tiny-wb`, the entry point remains `./release.sh --compile-only`.
5. Review `SHA256SUMS`, archive layout and modes, and the target-aware check
   report. Apple runtime checks are explicitly `skipped` on Linux.
6. Perform native smoke/hardware testing on compatible macOS machines. Publish
   later with the normal release command; no separate Apple publisher or manual
   `build-macos.sh` step is needed.

`--target` overrides the configured target list and can be repeated. `--features`
accepts a comma-separated list and overrides `build.features`:

```console
rust_realease . --package my-tool \
  --target aarch64-apple-darwin \
  --target x86_64-apple-darwin \
  --features serial,ui \
  --artifact-format both --compile-only
```

Package name, version and declared binary names come from Cargo metadata. The
selected profile, lock policy, and features are applied identically to Cross
and cargo-zigbuild.

## Configuration

The default backend mapping is fixed and target-aware:

| Target | Backend |
| --- | --- |
| `aarch64-apple-darwin` | pinned cargo-zigbuild container |
| `x86_64-apple-darwin` | pinned cargo-zigbuild container |
| every other valid Rust target | existing Cross command |

The complete supported syntax is:

```toml
[build]
targets = [
  "x86_64-unknown-linux-gnu",
  "x86_64-pc-windows-gnu",
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
]
profile = "release"
locked = true
features = ["serial", "ui"] # Optional; default is no explicit features.

[build.apple]
image = "ghcr.io/rust-cross/cargo-zigbuild@sha256:9b4f6b3eb9e8f9fefb3960fe288ddba576627d66d9eff851ce7f62d2455564a5"
deployment-target = "13.0"
sdk-root = "/opt/MacOSX11.3.sdk"

[package]
format = "both"
```

The `[build.apple]` table is optional because these exact values are built-in
defaults. Declaring it explicitly makes the compiler/SDK contract visible in a
release policy. The image must use an immutable lowercase SHA-256 digest;
mutable tags are rejected. `deployment-target` must parse as a macOS version
at least 13.0. `sdk-root` must be an absolute path inside the image without
parent traversal. Unknown settings, invalid feature names and unsupported
Apple triples fail configuration validation.

Layered configuration follows the existing rules. A later `build.features`
list replaces an earlier list. Individual values in `[build.apple]` override
earlier values, so a user-level image pin cannot silently erase a project-level
deployment target or SDK path. `--features` replaces the configured feature
list when supplied.

## Container contract and security

The publisher invokes the selected Docker or rootless Podman engine directly:

```text
ENGINE run --rm --user UID:GID [--userns=keep-id for Podman]
  -v ABSOLUTE_WORKSPACE:/io
  -v ABSOLUTE_FRESH_OUTPUT:/output
  -w /io
  -e HOME=/tmp
  -e CARGO_HOME=/output/cargo-home
  -e CARGO_TARGET_DIR=/output/target
  -e ZIG_GLOBAL_CACHE_DIR=/output/zig-cache
  -e MACOSX_DEPLOYMENT_TARGET=13.0
  -e SDKROOT=/opt/MacOSX11.3.sdk
  PINNED_IMAGE
  cargo zigbuild --locked --release --package PACKAGE --bin BINARY
    --target APPLE_TARGET --message-format=json-render-diagnostics
```

Every argument is passed as a distinct process argument, so spaces and Unicode
in checkout/output paths do not undergo shell interpretation. The source is
mounted at `/io`; a fresh publisher-owned directory is mounted at `/output`.
Cargo and Zig caches stay writable there, and generated files retain the
caller's UID/GID. There are no interactive flags or container-engine socket
mounts. Podman receives `--userns=keep-id`; Docker does not.

Only the listed environment variables enter the compiler container. Gitea's
configured token variable and host variables whose names contain `TOKEN`,
`PASSWORD`, `SECRET`, or `CREDENTIAL` are removed from compiler subprocesses.
They do not appear in arguments or verbose logs. Private Cargo dependencies
therefore need a separately designed non-secret-in-container acquisition
strategy; do not add publication tokens to Apple settings.

`CROSS_CONTAINER_ENGINE=podman` or `docker` selects the engine for both
backends. Otherwise the existing Docker-then-Podman discovery applies. The
engine must answer `info`. Apple builds require a Linux host; neither a Mac nor
host Xcode is required. `CROSS_CONFIG` continues to reach Cross for Linux and
Windows and is irrelevant to the direct Apple container invocation.

The pinned image is the environment verified by the downstream reference:
Rust 1.93.0, cargo-zigbuild 0.23.4, Zig 0.16.0, Apple Rust targets, and
`SDKROOT=/opt/MacOSX11.3.sdk`. The SDK is supplied by the upstream
`rust-cross/cargo-zigbuild` image. The digest identifies the actual image;
the SDK directory name alone does not establish the deployment minimum. During
the real verification below, LLVM reported an SDK load-command value of 26.4
and a minimum of 13.0. The publisher validates the minimum independently.

## Build discovery and cache identity

Apple builds request Cargo JSON messages and use each `compiler-artifact`
executable path. `/output` container paths are remapped to the exact mounted
host directory and canonicalized beneath that fresh directory. The publisher
does not reconstruct artifacts from a conventional checkout `target/` path.
This supports invocation outside the checkout, paths with spaces, workspace
package selection, custom profiles, and publisher-owned output directories.

A clean-commit build cache includes:

- commit, package, version, declared binaries and complete target list;
- profile, lock policy and Cargo features;
- container engine and active host Rust toolchain;
- Apple image digest, deployment target and SDK root.

Cached output paths must remain regular files beneath the publisher cache and
their SHA-256 hashes must still match. Cached Apple files are re-inspected as
Mach-O before reuse. Changing any fingerprinted setting, changing a binary,
using a dirty checkout, deleting an output, or passing `--recompile` forces a
fresh build. A backend failure propagates immediately and cannot be reported as
a successful build or create/upload a partial release.

## Artifacts and target-aware verification

Apple binary names have no `.exe` suffix. Existing archive and raw-binary
naming applies unchanged. For version `{version}` and package `tiny-wb`:

```text
tiny-wb-aarch64-apple-darwin-v{version}.tgz
tiny-wb-aarch64-apple-darwin-v{version}
tiny-wb-x86_64-apple-darwin-v{version}.tgz
tiny-wb-x86_64-apple-darwin-v{version}
```

Each tgz has the cargo-binstall layout:

```text
tiny-wb-aarch64-apple-darwin-v{version}/
  tiny-wb
```

The member and standalone file retain executable permission. Ownership and
timestamps in tgz files remain normalized. A mixed four-target release using
`format = "both"` produces eight payload assets and one `SHA256SUMS` asset.
All local archives are checked before any release creation. Existing remote
collision handling, replacement policy, upload, authenticated/private download
handling, and checksum verification apply to Apple assets too.

For every Apple executable, the Linux host parses the Mach-O header and load
commands and requires:

- a little-endian 64-bit executable matching arm64 or x86-64 as requested;
- macOS `LC_BUILD_VERSION` exactly matching `deployment-target`;
- every declared loaded/re-exported/weak/upward dylib under `/usr/lib/` or
  `/System/Library/`;
- at least one system dynamic dependency.

This detects architecture mistakes, a silently raised deployment minimum, and
build-machine/container library leakage. It does not execute Mach-O files.
`cargo-binstall` verification installs foreign binaries into an isolated
directory but does not run them. Console output records macOS runtime as
`skipped (no compatible macOS runner)`, never passed. A native Mac should test
startup, the application's normal workflow, and relevant hardware.

## tiny-wb migration

Apply this configuration change in the downstream checkout; its existing
unprefixed tags, release profile, locked mode, Gitea token setting, repository
fallback and `release/Cross.toml` remain unchanged:

```diff
 [build]
-targets = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"]
+targets = [
+  "x86_64-unknown-linux-gnu",
+  "x86_64-pc-windows-gnu",
+  "aarch64-apple-darwin",
+  "x86_64-apple-darwin",
+]
 profile = "release"
 locked = true
+
+[build.apple]
+image = "ghcr.io/rust-cross/cargo-zigbuild@sha256:9b4f6b3eb9e8f9fefb3960fe288ddba576627d66d9eff851ce7f62d2455564a5"
+deployment-target = "13.0"
+sdk-root = "/opt/MacOSX11.3.sdk"
```

`release.sh` already passes `--artifact-format both` and selects
`release/Cross.toml`; it needs no extra Apple build command. The publisher
reads version and binaries from Cargo metadata, so do not put `0.1.4` or any
other tested version in release configuration. `scripts/build-macos.sh` remains
a useful diagnostic reference and can be retired from the release procedure
after adopting a publisher version containing this feature.

Run:

```console
./release.sh --compile-only
```

No Gitea token is required. Publication remains a separate explicit invocation.

## Verification performed for this implementation

No release, upload, tag, push, Jenkins job, or global publisher installation was
performed. A disposable clone of `tiny-wb` commit `8ed07cb` under `/tmp` was
built with this checkout's publisher and rootless Podman.

Real two-target command:

```console
CROSS_CONTAINER_ENGINE=podman ./target/debug/rust_realease \
  /tmp/tiny-wb-rust-release-macos --package tiny-wb \
  --target aarch64-apple-darwin --target x86_64-apple-darwin \
  --artifact-format both --compile-only --skip-tests
```

It completed both targets in 95.7 seconds. A second identical invocation reused
commit `8ed07cb60eb6` from the hash-validated cache. The first artifacts were:

```text
/tmp/tiny-wb-rust-release-macos/dist/release-IxUtOI/
  tiny-wb-aarch64-apple-darwin-v0.1.4
  tiny-wb-aarch64-apple-darwin-v0.1.4.tgz
  tiny-wb-x86_64-apple-darwin-v0.1.4
  tiny-wb-x86_64-apple-darwin-v0.1.4.tgz
  SHA256SUMS
```

`sha256sum -c SHA256SUMS` passed. `file` identified 64-bit arm64 and x86-64
Mach-O executables. Both tgz files had one root directory and a mode-0755
`tiny-wb`. `llvm-objdump` reported macOS minimum 13.0 and these dependencies for
both architectures: IOKit, CoreFoundation, libSystem, libobjc, libiconv and
libcharset, all under Apple system paths.

The real four-target command additionally set
`CROSS_CONFIG=/tmp/tiny-wb-rust-release-macos/release/Cross.toml` and selected
`x86_64-unknown-linux-gnu` plus `x86_64-pc-windows-gnu`. It completed all four
builds in 169.7 seconds and produced exactly eight payload files in:

```text
/tmp/tiny-wb-rust-release-macos/dist/release-3XAf1R/
```

The automated suite also covers Docker/Podman argument differences, paths with
spaces and Unicode, mixed routing, credential removal, failure propagation,
dry-run, token-free compile-only operation, cache reuse/invalidation fields,
Mach-O rejection cases, artifact names, permissions, archive layout and eight
checksum entries. Native macOS execution, exact macOS versions/CPUs exercised by
the maintainer's earlier successful testing, hardware behavior, Gatekeeper,
signing, notarization, Jenkins and real publication remain external checks.
