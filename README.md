# Rust Binstall Publisher

Small release helper for Rust CLI projects.

Its intended job is to:

1. acquire a Rust project;
2. inspect and validate its Cargo metadata;
3. ensure cargo-binstall metadata is correct;
4. cross-compile supported targets;
5. package release artifacts;
6. create a Gitea release;
7. upload the artifacts;
8. verify that cargo-binstall can install them.

## Setup

The wrapper intentionally refuses to run without an active Python
virtual environment.

Run:

```bash
./release.sh --help
