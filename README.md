# rust_realease

![version](https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/version.svg)
![tests](https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/test.svg)
![test coverage](https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/coverage.svg)

`rust_realease` builds and publishes the binaries from a Rust package to a Gitea release. Give it a local project directory or a Git URL. It inspects Cargo metadata, checks the version tag, builds the selected targets with Cross, creates cargo-binstall-compatible `.tgz` archives, directly downloadable executables, or both, writes `SHA256SUMS`, uploads the assets, and verifies release downloads. It can also prepare a project for release with explicit commit, tag, and push options.

## Install

The preferred installation method is cargo-binstall, which downloads the matching prebuilt release artifact:

```bash
cargo binstall --git https://proxy.dandaedre.com/gitea/idle-org/rust_realease \
  rust_realease
rust_realease --help
```

To build and publish releases, also install Git, Cross, and Docker or Podman. Install cargo-binstall itself when producing the default `tgz` artifacts and running end-to-end installation verification. A working container backend is needed for release builds.

For development, or when a prebuilt artifact is unavailable for the host, install from this checkout:

```bash
cargo install --path . --locked
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

Each run handles one package and all of its Cargo binary targets. A dry run reports the package, version, expected tag, metadata changes, build targets, artifact names, prerequisites, and release destination. It writes no project files, builds no binaries, and changes no Git refs or releases. It exits nonzero when it finds requirements that still need attention.

Interactive terminals use colored status markers. Cross builds report elapsed
time for every target and for the complete build. Redirected output and CI logs
remain plain.

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

If you only want to add or repair the recommended `[package.metadata.binstall]` entries in `Cargo.toml`, use the existing metadata-only path:

```bash
rust_realease ./my-tool --fix-metadata --stop-after metadata
```

Preview the same change without writing it by adding `--dry-run`. The generated values follow the archive layout produced by `rust_realease`, including the configured release tag prefix.

The preparation commit uses your configured Git author and signing settings. The tag is annotated and follows your Git tag-signing configuration (`git config tag.gpgsign true` to sign it); an existing tag is never moved. The branch and tag are pushed atomically without force. Use `--cross-image-tag main` only when you want preparation to set Cross images for the selected targets.

## Build local artifacts

```bash
rust_realease ./my-tool --compile-only
```

This runs host `cargo check` and `cargo test`, builds the selected targets with Cross, and writes the configured artifacts plus `SHA256SUMS` into a new `dist/release-*/` directory. It does not contact Gitea. The default targets are:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-pc-windows-gnu`

Override the matrix with repeatable `--target TARGET` options. Use `--stop-after verify` for host checks alone.

A completed Cross build from a clean checkout is reused automatically when the Git commit and build settings still match. Packaging is repeated so the upload always gets a newly validated artifact set. Pass `--recompile` to ignore the completed-build cache and build every target again. Dirty checkouts are never reused.

Use `--artifact-format binary` when you only want raw executables, or `--artifact-format both` to publish raw executables alongside the default cargo-binstall archives:

```bash
rust_realease ./my-tool --artifact-format binary --compile-only
rust_realease ./my-tool --artifact-format both
```

## Publish to Gitea

The package version and release tag must agree. For version `0.1.0`, the default tag is `v0.1.0`. Before ordinary publication, the local tag must point to the checkout and the matching remote tag must exist. Publication requires a clean checkout and an HTTPS Gitea destination.

Set a Gitea token in the environment and run the command without `--stop-after`:

```bash
read -r -s -p 'Gitea token: ' GITEA_TOKEN
printf '\n'
export GITEA_TOKEN
rust_realease ./my-tool
```

Add release notes inline or read them from a UTF-8 file:

```bash
rust_realease ./my-tool -m "Highlights and upgrade notes"
rust_realease ./my-tool --message-file CHANGELOG.md
```

The two message options are mutually exclusive. Supplying notes also updates the body when the tagged Gitea release already exists; omitting them preserves an existing body.

To publish `rust_realease` itself, install the current publisher and use the
repository wrapper. It reads the token from a file without placing it in the
process arguments and publishes both archive and standalone-binary assets:

```bash
cargo install --path . --locked
./release.sh ../gitea_token.txt --compile-only
./release.sh ../gitea_token.txt --dry-run
./release.sh ../gitea_token.txt
```

For a remote repository, select the exact tag to check out:

```bash
rust_realease https://gitea.example.org/owner/my-tool.git --tag v0.1.0
```

If Git uses an internal repository URL but releases must use a public proxy, add an exact repository fallback to a project `rust-release.toml` or to the user configuration. The public fallback is used for Cargo metadata, Gitea publication, download verification, and cargo-binstall while the configured Git origin remains valid for cloning and preparation pushes. See [Advanced usage](docs/advanced-usage.md#configuration) for the file locations and an example.

The token account needs repository read access and release write access. `--push-preparation` additionally needs code write access. The token is read from `GITEA_TOKEN` by default and is kept out of child build processes. Public releases are verified by anonymous downloads. Private releases are downloaded with Gitea authentication; use `--skip-binstall-test` for them because cargo-binstall cannot install from their private attachment URLs. An existing asset with the same name stops the upload; `--replace-assets` explicitly permits replacing matching assets.

## More options

See [Advanced usage](docs/advanced-usage.md) for configuration, metadata, workspace behavior, stage controls, artifact layout, token handling, and troubleshooting. Run `rust_realease --help` for the complete option list.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

Jenkins runs Cargo tests and enforces an 80% line-coverage floor. Run the exact
coverage command locally with:

```bash
cargo tarpaulin --config tarpaulin.toml --out Json --output-dir coverage
```

The current suite measures 80.27% (1,351/1,683 lines). `Cargo.toml` is the
authoritative version source. Jenkins temporarily renames Tarpaulin's JSON to
`coverage/summary.json`, extracts and validates the overall percentage, renders
badges locally, then removes the summary. It archives only
`coverage/version.svg`, `coverage/test.svg`, and `coverage/coverage.svg`, with
fingerprinting. Successful `main` builds publish those files atomically under
`/mnt/assets/coverage/idle-org/rust_realease/main/`, serving:

- `https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/version.svg`
- `https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/test.svg`
- `https://assets.dandaedre.com/coverage/idle-org/rust_realease/main/coverage.svg`

The asset server must mount published assets read-only, disable directory
listings, serve SVG with a restrictive content security policy, and revalidate
these mutable URLs or use a short cache lifetime. Cross builds and live Gitea
releases remain separate release checks. The small
[target_test_project fixture](https://proxy.dandaedre.com/gitea/idle-org/target_test_project)
is maintained in its own repository for fast target matrix experiments. Clone
it separately when needed; it is not part of this checkout.

With `cargo-binstall` installed, run the opt-in isolated install fixture for all three targets with `cargo test real_binstall_installs_local_release_fixture -- --ignored`.
