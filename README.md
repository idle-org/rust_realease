# rust_realease

![current version](https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/version.svg)
![test status](https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/test.svg)
![line coverage](https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/coverage.svg)

`rust_realease` builds Rust binary packages for multiple targets and publishes
them as Gitea release assets. It is intended for maintainers who want
repeatable Cross builds, checksums, cargo-binstall archives, and post-upload
verification without maintaining a separate release pipeline.

## Prerequisites and installation

Install [Git](https://git-scm.com/),
[Cross](https://github.com/cross-rs/cross), and a Cross-compatible Docker or
Podman backend. The default archive format also requires
[cargo-binstall](https://github.com/cargo-bins/cargo-binstall).

Install the latest prebuilt release:

```bash
cargo binstall --git https://proxy.dandaedre.com/gitea/idle-org/rust_realease \
  rust_realease
rust_realease --help
```

To build the current checkout instead:

```bash
cargo install --path . --locked
```

## Quick start

Preview a local project without changing files, building binaries, or
contacting Gitea:

```bash
rust_realease ./my-tool --dry-run
```

The preview reports the selected package and binaries, expected version tag,
metadata changes, build targets, artifact names, prerequisites, and release
destination. It exits nonzero when something needs attention. Select a package
explicitly when the source is a workspace:

```bash
rust_realease ./my-workspace --package my-tool --dry-run
```

Build local artifacts through the package stage with:

```bash
rust_realease ./my-tool --compile-only
```

This mode does not require a Gitea token or contact Gitea. Artifacts and
`SHA256SUMS` are written under `dist/release-*/`. A later publication with the
same clean commit and build settings reuses the completed target builds; pass
`--recompile` to rebuild them instead.

## Publish a Gitea release

The package version must match a local tag, and the same tag must already
exist on the remote. Publication also requires a clean checkout and an HTTPS
Gitea release destination.

Store a Gitea access token in a single-line UTF-8 file and pass it directly:

```bash
rust_realease ./my-tool --token-file ../gitea_token.txt
```

The token file takes precedence over the configured token environment
variable. Without `--token-file`, the default is `GITEA_TOKEN`. Grant the token
repository read and release write access; preparation pushes also require code
write access. Tokens are redacted from diagnostics and are not passed to Cargo
or Cross child processes.

Use `--prepare-repo --dry-run` to preview supported metadata and repository
preparation before publishing. See the advanced guide for explicit commit,
tag, push, release-note, private-repository, and asset-replacement workflows.

## Documentation

- [Advanced usage](docs/advanced-usage.md) covers sources, configuration,
  metadata preparation, pipeline stages, artifact formats, authentication,
  this repository's release procedure, and troubleshooting.
- `rust_realease --help` is the complete option reference.

## Development

From the repository root, run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked --all
```

Jenkins enforces an 80% line-coverage floor. The opt-in real cargo-binstall
fixture requires its external test repository and is ignored by the ordinary
test suite; its setup is documented in [Advanced usage](docs/advanced-usage.md#disposable-release-fixture).

## License and support

This project is licensed under the [MIT License](LICENSE). Report problems in
the canonical [Gitea repository](https://proxy.dandaedre.com/gitea/idle-org/rust_realease/issues).
