# rust_realease

`rust_realease` builds and publishes one binary from a Rust package to a Gitea release. Give it a local project directory or a Git URL. It inspects Cargo metadata, checks the version tag, builds the selected targets with Cross, creates `.tgz` archives and `SHA256SUMS`, uploads them, and verifies release downloads. It can also prepare a project for release with explicit commit, tag, and push options.

## Install

Install Rust and Cargo, Git, Cross, Docker or Podman, and cargo-binstall. A working container backend is needed for release builds.

```bash
cargo install --path . --locked
rust_realease --help
```

To run from this checkout without installing, use `cargo run -- SOURCE [OPTIONS]`.

## Start with a project

Preview a local project or remote Git repository:

```bash
rust_realease ./my-tool --dry-run
rust_realease https://gitea.example.org/owner/my-tool.git --dry-run
```

For a workspace, select the exact Cargo package name:

```bash
rust_realease ./my-workspace --package my-tool --dry-run
```

Each run handles one package with one binary. A dry run reports the package, version, expected tag, metadata changes, build targets, archive names, prerequisites, and release destination. It writes no project files, builds no binaries, and changes no Git refs or releases. It exits nonzero when it finds requirements that still need attention.

## Prepare a release

Preview the project changes first:

```bash
rust_realease ./my-tool --prepare-repo --dry-run
```

Apply the preparation locally:

```bash
rust_realease ./my-tool --prepare-repo
```

Preparation repairs supported `Cargo.toml` release metadata, updates known `.gitignore` and `Cross.toml` settings, and generates a missing `Cargo.lock`. It exits before building or publishing. Review and commit local changes yourself, or authorize the Git steps explicitly from a clean checkout:

```bash
rust_realease ./my-tool --prepare-repo \
  --commit-preparation --create-tag --push-preparation
```

The preparation commit uses your configured Git author and signing settings. The tag is annotated and follows your Git tag-signing configuration (`git config tag.gpgsign true` to sign it); an existing tag is never moved. The branch and tag are pushed atomically without force. Use `--cross-image-tag main` only when you want preparation to set Cross images for the selected targets.

## Build local archives

```bash
rust_realease ./my-tool --stop-after package
```

This runs host `cargo check` and `cargo test`, builds the selected targets with Cross, and writes archives plus `SHA256SUMS` into a new `dist/release-*/` directory. It does not contact Gitea. The default targets are:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-pc-windows-gnu`

Override the matrix with repeatable `--target TARGET` options. Use `--stop-after verify` for host checks alone.

## Publish to Gitea

The package version and release tag must agree. For version `0.1.0`, the default tag is `v0.1.0`. Before ordinary publication, the local tag must point to the checkout and the matching remote tag must exist. Publication requires a clean checkout and an HTTPS Gitea destination.

Set a Gitea token in the environment and run the command without `--stop-after`:

```bash
read -r -s -p 'Gitea token: ' GITEA_TOKEN
printf '\n'
export GITEA_TOKEN
rust_realease ./my-tool
```

For a remote repository, select the exact tag to check out:

```bash
rust_realease https://gitea.example.org/owner/my-tool.git --tag v0.1.0
```

The token account needs repository read access and release write access. `--push-preparation` additionally needs code write access. The token is read from `GITEA_TOKEN` by default and is kept out of child build processes. Public releases are verified by anonymous downloads. Private releases are downloaded with Gitea authentication; use `--skip-binstall-test` for them because cargo-binstall cannot install from their private attachment URLs. An existing asset with the same name stops the upload; `--replace-assets` explicitly permits replacing matching assets.

## More options

See [Advanced usage](docs/advanced-usage.md) for configuration, metadata, workspace behavior, stage controls, artifact layout, token handling, and troubleshooting. Run `rust_realease --help` for the complete option list.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

Jenkins runs Cargo tests only. Cross builds and live Gitea releases are separate release checks. The small `target_test_project` submodule provides a fast target matrix fixture.

With `cargo-binstall` installed, run the opt-in isolated install fixture for all three targets with `cargo test real_binstall_installs_local_release_fixture -- --ignored`.
