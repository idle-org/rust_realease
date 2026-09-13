# Rust Binstall Publisher 🦀📦

A small release tool for cross-compiling Rust CLI projects, publishing their binaries to Gitea releases, and making them available through `cargo binstall`.

## What it does

Rust Binstall Publisher automates the boring parts of releasing small Rust tools:

* Pulls a Rust project from Git or works from a local checkout.
* Checks and optionally fixes `cargo-binstall` metadata.
* Verifies Cargo version and Git tag consistency.
* Cross-compiles binaries for multiple architectures.
* Packages release artifacts and generates checksums.
* Creates or updates a Gitea release.
* Uploads the compiled binaries.
* Verifies that the resulting artifacts can be consumed by `cargo binstall`.

### Default targets

```text
x86_64-unknown-linux-musl
aarch64-unknown-linux-musl
x86_64-pc-windows-gnu
```

## Setup

The tool requires a Python virtual environment.

On first run:

```bash
./release.sh --help
```

The wrapper will offer to create and configure `.venv` if necessary.

Then activate it:

```bash
source .venv/bin/activate
```

For publishing, provide a Gitea API token:

```bash
export GITEA_TOKEN="..."
```

## Usage

Inspect a local project without changing anything:

```bash
./release.sh ./my-project --dry-run
```

Inspect a remote project:

```bash
./release.sh \
    https://gitea.example.org/user/my-project.git \
    --tag v0.1.0 \
    --dry-run
```

Build and publish a release:

```bash
./release.sh \
    https://gitea.example.org/user/my-project.git \
    --tag v0.1.0 \
    --fix-metadata
```

The resulting release artifacts are intended to be installable with:

```bash
cargo binstall my-project
```

## Status

🚧 **Work in progress / proof of concept.**

The initial focus is intentionally narrow: single-package, single-binary Rust CLI projects hosted on Gitea.

See `AGENT.md` and `AGENT_TASKS.md` for the current design and implementation plan.
