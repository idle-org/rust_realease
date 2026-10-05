# Advanced usage

## Disposable release fixture

The [target_test_project repository](https://proxy.dandaedre.com/gitea/idle-org/target_test_project) is a small standalone Cargo binary for Cross and Gitea integration tests. Clone it separately rather than expecting a submodule in this repository:

```bash
git clone https://proxy.dandaedre.com/gitea/idle-org/target_test_project.git
rust_realease ./target_test_project --dry-run
```

Its public [v0.1.3 release](https://proxy.dandaedre.com/gitea/idle-org/target_test_project/releases/tag/v0.1.3) was used to validate preparation, all three default Cross targets, published checksums, and isolated cargo-binstall installs. Use a new version and tag for future publication tests; release assets with the same names are deliberately treated as collisions.

With cargo-binstall installed, run the opt-in isolated install fixture for all
three targets with:

```bash
cargo test --locked real_binstall_installs_local_release_fixture -- --ignored
```

## Sources and package selection

`SOURCE` can be a local directory, an HTTP(S) Git URL, an SSH URL, or an SCP-style URL such as `git@host:owner/repo.git`. Remote sources are cloned into a managed checkout. Use `--keep-workdir` to retain that checkout and its artifacts; preparation retains remote checkouts automatically. Local sources are never deleted.

Use `--package NAME` to select one workspace member by its exact Cargo package name. Selection uses `cargo metadata` and accepts inherited workspace version and repository values. Library-only packages cannot be released. Every binary target declared by the selected package is built and packaged; this includes binaries with `required-features` when those features are enabled by the package's defaults. Checks, tests, and builds are scoped to the selected package.

`--repository-url https://host/owner/repo` selects the release destination when Cargo metadata or Git origin needs an explicit web URL. Materially conflicting origin and release destinations are refused. Repository URLs with embedded credentials are refused.

## Configuration

Configuration is layered in this order, with later values taking precedence:

1. bundled defaults;
2. the user configuration at the platform-native config directory returned by the `directories` crate (`~/.config/rust_realease/config.toml` on Linux);
3. `rust-release.toml` in the acquired project root;
4. the file passed with `--config PATH`;
5. CLI options.

Missing automatically discovered files are ignored. A path explicitly passed with `--config` must exist and be valid TOML. Repository fallback lists are combined across layers; a later entry for the same normalized origin replaces the earlier entry.

```toml
[release]
tag-prefix = "v"

[build]
targets = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-pc-windows-gnu",
]
profile = "release"
locked = true

[package]
format = "tgz"

[repository]
fallbacks = []

[gitea]
token-env = "GITEA_TOKEN"
```

Use `--tag-prefix ""` for an unprefixed tag such as `0.3.1`. `--tag TAG` must match the selected package version and configured prefix. `--unlocked` permits builds without the usual locked dependency resolution; the default uses `--locked`.

`package.format` accepts `"tgz"`, `"binary"`, or `"both"`. The default `"tgz"` produces cargo-binstall archives. `"binary"` produces only directly downloadable executables and does not require or verify cargo-binstall metadata. `"both"` produces both forms. The `--artifact-format` option overrides this setting for an individual run.

When a checkout uses an internal Git URL but its Gitea API and release downloads are exposed through another URL, configure an exact fallback pair. For example, either the project file or the user file can contain:

```toml
[[repository.fallbacks]]
origin = "http://git.dandaedre.com/idle-org/john-slop"
fallback-url = "https://proxy.dandaedre.com/gitea/idle-org/john-slop"
```

URLs are normalized, including removal of a trailing `.git`; SCP-style and SSH URLs normalize to HTTPS. The scheme remains significant for HTTP(S), so add separate rules if the same origin is used through both HTTP and SSH. Each rule matches one repository, not an entire host. The fallback must use HTTPS because it becomes the Gitea API and public download base. It is also written to `package.repository` during metadata repair. Git continues to clone and push through `origin`; the mapping tells the safety checks that the internal and public repositories are equivalent.

## Metadata and preparation

The publisher checks the selected member's repository and, when archives are enabled, its cargo-binstall metadata. `--fix-metadata` repairs supported entries in that member's `Cargo.toml` while preserving comments where practical. It does not commit, tag, or push. To preview the full preparation diff, use `--prepare-repo --dry-run`.

The required binstall contract is:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz"
pkg-fmt = "tgz"
bin-dir = "{ name }-{ target }-v{ version }/{ bin }{ binary-ext }"
```

The generated `pkg-url` uses the configured tag prefix; the archive filename itself retains `-v{version}`. Unsupported binstall keys are reported as metadata conflicts. A preparation run may create one extra fix commit, as intended. `--commit-preparation` requires a clean worktree and index. `--create-tag` requires that commit option, and `--push-preparation` requires both. No option moves an existing tag or force pushes.

For repositories whose build scripts need a selected Rust toolchain inside Cross, preparation can add the supported `Cross.toml` environment setting. `--cross-image-tag TAG` explicitly selects `ghcr.io/cross-rs/TARGET:TAG` images. Existing unrelated Cross settings are preserved. Repository-provided Cross hooks such as `pre-build`, `dockerfile`, and `runner` are rejected by the release build.

## Pipeline stages

`--compile-only` runs through the package stage and then stops: it performs host checks, cross-compiles, and writes local artifacts without requiring a Gitea token or creating or uploading a release. It is the explicit artifact-build mode and conflicts with `--dry-run`, `--prepare-repo`, and `--stop-after`. For this repository, `./release.sh --compile-only` provides the same token-free path; a later `./release.sh TOKEN_FILE` publication reuses the completed build when its fingerprint still matches.

`--stop-after` can stop at `inspect`, `metadata`, `verify`, `build`, `package`, `release`, `upload`, or `binstall`. `verify` runs host `cargo check` and `cargo test`; `--skip-tests` skips only tests. A successful Cross build from a clean checkout records its Git commit, package, targets, profile, lock mode, container engine, active Rust toolchain, and binary list. A later run with the same inputs reuses those regular binary files from Cargo's target directory; dirty checkouts, changed inputs, missing files, and invalid cache records build in a fresh target directory instead. `--recompile` explicitly bypasses a matching completed build. `release` creates or reuses the Gitea release; `upload` uploads archives and checksums. The final stage downloads assets anonymously for public repositories or with Gitea authentication for private repositories, checks their checksums and archive layout, and runs isolated `cargo-binstall` installs for public targets. `--skip-binstall-test` skips that install check, while retaining download verification.

`--draft --stop-after upload` is useful when a release must stay private for review. Private repositories require `--skip-binstall-test`; cargo-binstall cannot fetch their attachment URLs, even when Gitea authentication is configured. `--prerelease` and `--draft` set properties when a release is created; reusing a release preserves its existing settings. `-m/--message TEXT` sets release notes, while `--message-file PATH` reads them from a UTF-8 file; they are mutually exclusive. Notes are included when creating a release and replace the body of an existing tagged release when explicitly supplied. Asset replacement is not transactional: if an upload fails after a matching asset is deleted, retry the publication explicitly.

## Artifact contract

Each archive is named `{name}-{target}-v{version}.tgz` and contains exactly one root directory with every binary declared by the selected package:

```text
my-tool-x86_64-unknown-linux-musl-v0.1.0.tgz
└── my-tool-x86_64-unknown-linux-musl-v0.1.0/
    ├── my-tool
    └── my-tool-helper
```

Windows archive members have an `.exe` suffix. Raw assets are target-qualified to remain unambiguous, for example `my-tool-x86_64-unknown-linux-musl-v0.1.0` and `my-tool-x86_64-pc-windows-gnu-v0.1.0.exe`. `SHA256SUMS` lists every generated asset. Archive ownership and timestamps are normalized; identical binary bytes and modes produce identical archives. Separate builds can still produce different binaries.

## Authentication and security

Authenticated Gitea API publication requires HTTPS. Pass `--token-file PATH` to read one token from a UTF-8 file; trailing line endings are ignored, while empty and multiline files are rejected. The file takes precedence over the configured environment variable. Without that option, the token comes from `gitea.token-env`, which defaults to `GITEA_TOKEN`. Tokens are never accepted as a positional argument or URL. Child Cargo and Cross processes do not receive them. An HTTPS preparation push to the matching release repository can use the token through a temporary askpass helper; SSH pushes use normal Git credentials. Grant the token account code read and release write access, plus code write when using `--push-preparation`.

Normal publication does not create a commit, tag, or push. Existing asset names cause an error unless `--replace-assets` is set; unrelated assets are not deleted. Private assets are verified with authenticated downloads and do not establish public installability.

## Releasing this repository

Install the publisher from the checkout, then use the repository wrapper. The
wrapper selects this package, publishes both archive and standalone binary
assets, and forwards its token file to `--token-file` when publishing:

```bash
cargo install --path . --locked
./release.sh --compile-only
./release.sh ../gitea_token.txt --dry-run
./release.sh ../gitea_token.txt
```

## CI badge pipeline

Jenkins runs the locked test suite and Tarpaulin configuration, enforces the
80% line-coverage floor, and renders version, test, and coverage SVGs with the
scripts in `scripts/`. It validates and archives exactly
`coverage/version.svg`, `coverage/test.svg`, and `coverage/coverage.svg`.
Every `main` build publishes those files during post-build cleanup under
`/mnt/assets/coverage/idle-org/rust_realease/main/` for the public badge URLs
used by the README; other branches do not publish them. A failed build renders
the test badge as failing and the coverage badge as unavailable.

Run the same coverage command locally with:

```bash
cargo tarpaulin --config tarpaulin.toml --out Json --output-dir coverage
```

The asset server must mount the published directory read-only, disable
directory listings, serve SVG with a restrictive content security policy, and
revalidate the mutable URLs or use a short cache lifetime. Cross builds and
live Gitea releases remain separate release checks.

## Troubleshooting

- **Missing tag:** prepare and push the expected tag explicitly, or use `--prepare-repo --commit-preparation --create-tag --push-preparation` from a clean checkout.
- **Missing Cargo.lock:** run preparation or use `--unlocked` when unlocked resolution is intentional.
- **Container unavailable:** start Docker or Podman. `CROSS_CONTAINER_ENGINE` can select `docker` or `podman`.
- **Cross image or native library failure:** configure an appropriate Cross image and target libraries in the project. The publisher does not substitute an MSVC target or hide linker failures.
- **Private release installation:** pass `--skip-binstall-test` to verify the uploaded assets with Gitea authentication. Use a public disposable release to test real cargo-binstall installation.
- **Asset collision:** inspect the existing release; use `--replace-assets` only when replacement is intended.

`cargo-binstall` verification installs into temporary directories and does not run foreign-target binaries. Plain registry based `cargo binstall my-tool` additionally requires a published crate with the release metadata; this tool does not publish crates.
