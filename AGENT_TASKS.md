# AGENT_TASKS.md

## Current milestone

The Rust CLI is the active implementation. The Python worker, launcher, and dependency files were removed at the user's request; their implementation and migration checklist remain in Git history. The small `target_test_project` submodule is the disposable integration fixture. Jenkins runs Cargo tests only.

## Product goals

- [x] Provide useful local unit, filesystem, mocked HTTP, and opt-in integration tests.
- [x] Accept local directories or Git URLs and produce local archives with `--stop-after package` or remote Gitea releases with the full pipeline.
- [x] Repair a local or cloned project with `--prepare-repo`, including an optional separate fix commit, tag, and atomic push when explicitly authorized.
- [x] Use the small fixture project for all three Cross targets; it prints target-specific output.
- [x] Complete a disposable public end-to-end release before using the live API workflow as a gate.

## Verified release milestones

- [x] Private v0.1.1 release: build, upload, authenticated archive and checksum verification; `--skip-binstall-test` required because private attachment URLs are inaccessible to cargo-binstall.
- [x] Public [v0.1.2](https://proxy.dandaedre.com/gitea/idle-org/target_test_project/releases/tag/v0.1.2): three Cross builds, four uploads, anonymous checks, and isolated cargo-binstall installs for all targets.
- [x] Public [v0.1.3](https://proxy.dandaedre.com/gitea/idle-org/target_test_project/releases/tag/v0.1.3): preparation dry run left files and refs unchanged; Rust preparation repaired metadata and Cross configuration, made a GPG-signed fix commit and annotated signed tag, and atomically pushed branch and tag using token authentication. The full release then passed the same three-target build, upload, anonymous verification, and isolated install checks.
- [x] Fixture SSH origin restored after temporary HTTPS push and publication checks. No force push, moved tag, or unrelated asset replacement occurred.

## Local quality gates

- [x] Package/workspace selection, inherited metadata, missing/invalid metadata, tag policy, archive contract, repository URL handling, Git safety, Gitea collision/replacement, token redaction, and verification have Rust coverage.
- [x] Opt-in local cargo-binstall fixture installs all three targets from a temporary server into isolated directories.
- [x] Rust install succeeds without Python (`cargo install --path . --locked --offline --root /tmp/rust_realease_install --target-dir target`).
- [x] Final `cargo fmt --check`, strict Clippy, and `cargo test --all` after this documentation update (28 passed; 3 opt-in ignored).
- [x] Final Git status reviewed; signed milestone commit records this tracker, the fixture pointer, and current maintenance guidance.

## Known limits

- Private Gitea releases cannot pass real cargo-binstall URL downloads; use `--skip-binstall-test`, which still verifies assets with authenticated downloads.
- A full automated Python-versus-Rust parity matrix cannot run from the Rust-only tree. Earlier Python baseline and smoke comparison results remain in Git history. The current Rust tests and disposable releases provide the active regression gates.
- `--replace-assets` is explicit and replacement is not transactional; a failed upload after deletion requires a deliberate retry.
