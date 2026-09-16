# AGENT_TASKS.md

## Goal

Migrate `rust_realease` from its current Python implementation to a native Rust CLI without regressing release behavior.

The Python implementation remains the behavioral oracle until the Rust replacement passes parity and end-to-end release validation.

## Updated product goals (2026-09-17)

- Keep useful local unit and fixture tests throughout the rewrite.
- Accept a project and eventually produce either local artifacts or a remote release.
- Support local and remote project repair before building. An explicitly requested repair workflow may create an additional fix commit, tag, and push; ordinary release runs retain the existing mutation boundaries until that workflow is implemented and tested.
- Defer live API release tests until a disposable token is provided.
- Use the small `target_test_project` submodule for fast build and cross-target experiments.

Current Rust milestone: `inspect` and `metadata` stages work for local packages and managed remote Git checkouts, including exact workspace package selection. The metadata stage validates and can repair the selected manifest with `--fix-metadata`; dry-run previews changes without writing. A shared subprocess runner redacts tokens and URL arguments and removes Gitea tokens from child environments. Preparation, builds, and release actions still use the Python worker.

Progress tracking: check a task only when the Rust behavior is implemented and locally verified. Keep phase gates open until their full fixture/parity criteria pass. Latest verified Rust checks: `cargo test --locked --offline --all` (10 tests), `cargo fmt --check`, and strict Clippy. The Python baseline has 45 passing tests and two opt-in tests skipped. CI runs Cargo tests only; Cross and live Gitea tests are deferred. Next work: complete runner environment controls and source acquisition tests, then build the repository preparation and local build pipeline.

Work in small, testable phases. Commit migration work in coherent increments.

---

## Migration rules

- [ ] Do not delete the Python implementation before Rust parity is proven.
- [ ] Do not change release semantics merely to simplify the port.
- [ ] Keep mutation operations opt-in.
- [ ] Preserve artifact names/layouts and cargo-binstall metadata.
- [ ] Preserve workspace `--package NAME` behavior.
- [ ] Preserve repository preparation behavior.
- [ ] Preserve fresh build target directories.
- [ ] Preserve Gitea asset collision/replacement semantics.
- [ ] Preserve token redaction and secret isolation.
- [ ] Treat live release operations as opt-in integration tests.
- [ ] Record intentional incompatibilities in this file and README.

---

## Phase 0 — Freeze and characterize the Python behavior

### Inventory

- [ ] Record the current CLI flags and defaults from the Python implementation.
- [ ] Record the config schema/defaults.
- [ ] Record stage ordering and `--stop-after` behavior.
- [ ] Record workspace/package-selection behavior.
- [ ] Record preparation flag dependency rules.
- [ ] Record artifact naming and archive layout.
- [ ] Record Gitea release and asset-collision semantics.
- [ ] Record verification behavior.
- [ ] Record token handling and redaction rules.

### Baseline test run

- [x] Run the existing Python unit/mocked test suite.
- [ ] Run existing opt-in cargo-binstall tests where the environment permits.
- [ ] Save representative dry-run outputs for fixtures.
- [ ] Save representative metadata-repair diffs.
- [ ] Save representative preparation diffs.
- [ ] Save representative archive listings and SHA256SUMS.

### Compatibility fixtures

Create or preserve fixtures for:

- [ ] standalone single-binary package;
- [ ] workspace with an explicitly selected CLI package;
- [ ] inherited workspace version/repository;
- [ ] ambiguous workspace;
- [ ] unknown package;
- [ ] library-only package;
- [ ] selected package with multiple binaries;
- [ ] missing Cargo.lock;
- [ ] missing binstall metadata;
- [ ] conflicting binstall metadata;
- [ ] custom/empty tag prefix;
- [ ] existing release asset collision.

### Gate

- [ ] Existing behavior is sufficiently characterized that a Rust result can be compared against it.

---

## Phase 1 — Create the Rust application skeleton

- [x] Add `Cargo.toml`.
- [x] Add `Cargo.lock`.
- [x] Add `src/main.rs`.
- [x] Add modules:
  - [x] `cli.rs`
  - [x] `config.rs`
  - [x] `error.rs`
  - [x] `model.rs`
  - [x] `runner.rs`
  - [x] `project.rs`
  - [x] `manifest.rs`
  - [x] `prepare.rs`
  - [x] `build.rs`
  - [x] `package.rs`
  - [x] `gitea.rs`
  - [x] `verify.rs`
- [ ] Add test fixture directory.
- [ ] Add `cargo fmt --check` gate.
- [ ] Add strict Clippy gate.
- [x] Add `cargo test --all` gate.

Suggested dependencies:

- [x] `clap`
- [x] `serde`
- [x] `toml`
- [x] `toml_edit`
- [ ] `thiserror`
- [ ] `reqwest` with rustls
- [x] `url`
- [x] `tempfile`
- [ ] `sha2`
- [ ] `tar`
- [ ] `flate2`

Test-only as useful:

- [ ] `assert_cmd`
- [ ] `predicates`
- [ ] mock HTTP server crate

### Gate

- [x] Rust binary builds.
- [x] `--help` works.
- [x] No release behavior has been removed from Python yet.

---

## Phase 2 — Port models, config, and pure policy

### Models

- [x] Define selected package model.
- [ ] Define target/artifact model.
- [ ] Define release destination model.
- [ ] Define preparation plan model.
- [x] Define stage enum.
- [x] Define config model.

### Configuration

- [x] Parse current `config.toml`.
- [x] Preserve defaults.
- [x] Implement CLI-over-config precedence.
- [x] Support custom config path.
- [x] Validate target/profile/package format settings.

### Pure policy

- [x] Implement expected tag generation.
- [x] Implement empty/custom tag prefix.
- [x] Implement archive naming.
- [x] Implement archive root naming.
- [x] Implement target-specific binary filename.
- [x] Implement expected binstall metadata.
- [ ] Implement release URL construction.
- [x] Implement stage ordering.

### Tests

- [ ] Port pure Python tests to Rust.
- [x] Add tests for non-`v` tags.
- [x] Add tests for Windows `.exe`.
- [x] Add tests for malformed config.
- [ ] Add tests for CLI precedence.

### Gate

- [ ] Rust pure-policy outputs match Python for the fixture matrix.

---

## Phase 3 — Port command execution and redaction

### Runner

- [x] Implement argv-based subprocess runner.
- [x] Support cwd.
- [ ] Support env additions/removals.
- [x] Support captured output.
- [ ] Support streaming where useful.
- [ ] Add verbose diagnostics.
- [x] Add exit-code/error context.

### Secret safety

- [ ] Redact `GITEA_TOKEN`.
- [x] Redact configured token env variable.
- [x] Redact credentials in URLs.
- [ ] Ensure child Cargo/Cross build processes do not inherit Gitea token.
- [x] Test error messages for token leakage.

### Gate

- [x] All runner tests pass.
- [x] No shell-string execution is used for normal commands.

---

## Phase 4 — Port source acquisition and Git behavior

### Local source

- [x] Accept local paths.
- [x] Resolve workspace root safely.
- [x] Never delete/move local source.
- [x] Read Git origin.
- [ ] Inspect worktree cleanliness.
- [ ] Resolve HEAD commit.

### Remote source

- [x] Accept HTTP/HTTPS Git URLs.
- [x] Accept SSH URLs.
- [x] Accept SCP-style URLs.
- [x] Clone to temporary directory.
- [x] Checkout requested tag exactly.
- [x] Implement default cleanup.
- [x] Implement `--keep-workdir`.
- [x] Preserve preparation-mode retained checkout behavior.

### Repository URL normalization

- [x] Normalize `.git` suffix.
- [x] Convert supported SSH/SCP repository identity to web/API identity safely.
- [x] Reject URLs with embedded HTTP credentials for release metadata.
- [x] Reject query strings where existing behavior does.
- [x] Support explicit `--repository-url`.

### Tests

- [ ] Port repository URL parsing tests.
- [ ] Test temp cleanup.
- [ ] Test keep-workdir.
- [x] Test local source remains untouched.
- [x] Test exact tag checkout.

### Gate

- [ ] Fixture acquisition behavior matches Python.

---

## Phase 5 — Port Cargo metadata and package selection

### Metadata

- [x] Execute `cargo metadata --format-version=1 --no-deps`.
- [x] Parse package/workspace information.
- [x] Resolve workspace root.
- [x] Resolve selected member manifest.
- [ ] Resolve target directory.
- [x] Resolve inherited package version.
- [x] Resolve inherited repository.

### Selection

- [x] Standalone package works without `--package`.
- [x] Multi-member workspace requires exact package selection when ambiguous.
- [x] `--package NAME` selects exact package.
- [x] Unknown package fails clearly.
- [x] Library-only package fails clearly.
- [x] Multi-binary selected package fails clearly.
- [x] Exactly one binary is accepted.

### Tests

- [ ] Port all workspace regression tests.
- [ ] Include unrelated failing workspace member fixture.
- [ ] Ensure selecting the intended package does not build/check unrelated member unnecessarily.

### Gate

- [ ] Rust selection decisions match Python for all compatibility fixtures.

---

## Phase 6 — Port manifest validation and repair

### Reading/editing

- [x] Read selected `Cargo.toml` with `toml_edit`.
- [x] Detect missing/incorrect `package.repository`.
- [x] Derive repository from Git origin where safe.
- [x] Validate `[package.metadata.binstall]`.
- [x] Detect unsupported/conflicting custom binstall options.

### Repair

- [x] Implement proposed-change plan.
- [x] Implement `--fix-metadata`.
- [x] Preserve comments/formatting where practical.
- [x] Edit only selected package manifest.
- [x] Make repair idempotent.
- [x] Report every changed key.

### Dry-run

- [ ] Print proposed diff.
- [x] Never write.

### Tests

- [ ] Port metadata validation tests.
- [ ] Port repair tests.
- [x] Add idempotency test.
- [x] Add workspace member-only edit test.
- [ ] Add inherited workspace version/repository cases.

### Gate

- [ ] Generated metadata matches current release contract.
- [ ] Python and Rust proposed repairs are equivalent for fixtures.

---

## Phase 7 — Port version/tag validation

- [x] Compute expected tag from selected package version.
- [x] Apply configured/CLI tag prefix.
- [x] Validate explicit `--tag`.
- [x] Validate local tag points to HEAD where required.
- [ ] Validate remote tag exists and points to expected commit before publication.
- [ ] Preserve preparation-mode distinction: intended tag naming without checking out an existing tag.
- [ ] Never move an existing tag.
- [ ] Never rewrite Cargo package version.

### Tests

- [ ] Matching tag.
- [ ] Mismatched tag.
- [x] Empty prefix.
- [x] Existing tag at wrong commit.
- [ ] Preparation with intended new tag.
- [ ] Ordinary publication without matching tag.

### Gate

- [ ] Tag policy matches Python behavior.

---

## Phase 8 — Port repository preparation

### Planning

- [ ] Implement preparation plan/diff.
- [ ] Repair selected Cargo metadata.
- [ ] Add build/artifact ignore entries.
- [ ] Add Cargo.lock inclusion rule as required.
- [ ] Generate Cargo.lock when missing.
- [ ] Preserve existing lockfile.
- [ ] Edit/create `Cross.toml`.
- [ ] Preserve unrelated Cross.toml settings.
- [ ] Support `--cross-image-tag`.

### Mutation boundaries

- [ ] `--prepare-repo` alone never commits/tags/pushes.
- [ ] `--commit-preparation` requires correct clean state.
- [ ] Stage only files belonging to the preparation.
- [ ] Preserve user's Git signing configuration.
- [ ] `--create-tag` requires commit-preparation workflow as currently specified.
- [ ] Create annotated release tag.
- [ ] Existing correct tag is validated, not moved.
- [ ] `--push-preparation` requires required predecessor flags.
- [ ] Verify push destination matches release repository.
- [ ] Push branch + tag atomically where current behavior expects it.
- [ ] Never force push.

### Dry-run

- [ ] Show file diffs.
- [ ] Show lockfile-generation intention.
- [ ] Show Git commit/tag/push intention.
- [ ] Perform none of them.

### Tests

- [ ] Clean preparation.
- [ ] Dirty-worktree refusal.
- [ ] Existing tag safety.
- [ ] Destination mismatch refusal.
- [ ] No implicit Git writes.
- [ ] Cross.toml preservation.
- [ ] Missing lockfile.
- [ ] Existing lockfile.

### Gate

- [ ] Rust preparation behavior matches Python fixtures.
- [ ] Disposable repo preparation produces a clean, reviewable result.

---

## Phase 9 — Port prerequisite checks

- [ ] Detect `git`.
- [ ] Detect `cargo`.
- [ ] Detect `cross`.
- [ ] Detect Docker.
- [ ] Detect Podman.
- [ ] Honor `CROSS_CONTAINER_ENGINE`.
- [ ] Determine usable container backend.
- [ ] Detect `cargo-binstall` when needed.
- [ ] Report all missing prerequisites before cross build.
- [ ] Dry-run performs only read-only checks.

### Gate

- [ ] Failure messages are concise and actionable.

---

## Phase 10 — Port check/test/build pipeline

### Host validation

- [ ] Run `cargo check` for selected package.
- [ ] Run `cargo test` for selected package.
- [ ] Respect `--skip-tests`.
- [ ] Preserve selected workspace package scoping.

### Cross build

- [ ] Build default Linux x86_64 musl.
- [ ] Build default Linux aarch64 musl.
- [ ] Build default Windows GNU x86_64.
- [ ] Respect repeatable `--target`.
- [ ] Respect profile.
- [ ] Respect locked/unlocked behavior.
- [ ] Pass selected workspace package correctly.
- [ ] Use fresh target subdirectory for every real release build.
- [ ] Preserve cross image configuration behavior.
- [ ] Do not pass Gitea token to build subprocesses.

### Tests

- [ ] Command-construction unit tests.
- [ ] Fake-runner orchestration tests.
- [ ] Real host Cargo fixture test.
- [ ] Real Cross tests behind opt-in environment gate.

### Gate

- [ ] Default target matrix builds the known integration fixture in the supported environment.
- [ ] Fresh target directory regression is covered.

---

## Phase 11 — Port packaging and checksums

### Packaging

- [ ] Locate built binary.
- [ ] Apply target-specific executable filename.
- [ ] Create standardized archive root.
- [ ] Preserve Unix executable mode.
- [ ] Create `.tgz`.
- [ ] Exclude unrelated files.
- [ ] Validate archive members against traversal paths.

### Checksums

- [ ] Compute SHA-256.
- [ ] Generate deterministic `SHA256SUMS`.
- [ ] Keep stable asset ordering.

### Reproducibility

- [ ] Normalize archive metadata where safe.
- [ ] Document any intentional difference from Python archive bytes.
- [ ] Keep naming/layout identical regardless.

### Tests

- [ ] Archive filename.
- [ ] Archive root.
- [ ] Unix binary mode.
- [ ] Windows `.exe`.
- [ ] SHA-256 generation.
- [ ] SHA256SUMS content.
- [ ] No extra files.
- [ ] Traversal/path safety.

### Gate

- [ ] Python and Rust package contracts match for all three default targets.

---

## Phase 12 — Port Gitea API client

### Client

- [ ] Parse base instance/repository coordinates.
- [ ] Load token only from configured environment variable.
- [ ] Use HTTPS for authenticated publication.
- [ ] Set useful user-agent.
- [ ] Implement contextual API errors.

### Releases

- [ ] Get remote tag.
- [ ] Get release by tag.
- [ ] Create release.
- [ ] Reuse existing release.
- [ ] Preserve draft/prerelease fields.

### Assets

- [ ] List release assets once according to current Gitea API behavior.
- [ ] Do not reintroduce the historical repeated pagination bug.
- [ ] Upload assets.
- [ ] Fail on duplicate filename by default.
- [ ] Delete/replace only under `--replace-assets`.
- [ ] Never touch unrelated assets.

### Tests

- [ ] Mock release absence -> create.
- [ ] Mock existing release -> reuse.
- [ ] Asset listing regression test.
- [ ] Collision refusal.
- [ ] Explicit replacement.
- [ ] HTTP failure context.
- [ ] Token redaction.

### Gate

- [ ] Mocked Gitea behavior matches Python.

---

## Phase 13 — Port verification

### Artifact verification

- [ ] Build expected asset URLs.
- [ ] Download published archives.
- [ ] Prefer anonymous downloads for public assets.
- [ ] Validate SHA256SUMS.
- [ ] Validate archive root.
- [ ] Validate binary filename.
- [ ] Validate Unix executable bit where meaningful.

### cargo-binstall

- [ ] Use selected package manifest.
- [ ] Create temporary Cargo home/install root.
- [ ] Install without touching user's normal Cargo bin.
- [ ] Verify all default target contracts where supported.
- [ ] Respect `--skip-binstall-test`.

### Tests

- [ ] Local/mock HTTP asset server integration.
- [ ] Real cargo-binstall integration fixture.
- [ ] Check all three default target artifact layouts.

### Gate

- [ ] Existing opt-in cargo-binstall integration test has a Rust equivalent and passes.

---

## Phase 14 — Port orchestration, stages, and dry-run

### Pipeline

- [ ] Recreate stage ordering.
- [ ] Implement `--stop-after`.
- [ ] Ensure each stage stops after successful completion.
- [ ] Preserve preparation as a separate workflow from ordinary publication.

### Dry-run

Verify no mutation:

- [ ] no file edits;
- [ ] no lock generation;
- [ ] no compile;
- [ ] no commit;
- [ ] no tag creation;
- [ ] no push;
- [ ] no release creation;
- [ ] no asset upload;
- [ ] no asset deletion.

Verify useful reporting:

- [ ] selected package;
- [ ] version;
- [ ] tag;
- [ ] proposed metadata repair;
- [ ] prerequisites;
- [ ] target matrix;
- [ ] artifact names;
- [ ] destination;
- [ ] planned preparation/Git operations.

### Tests

- [ ] Every stop boundary.
- [ ] Dry-run mutation guards.
- [ ] Error propagation.
- [ ] Verbose/non-verbose behavior.

### Gate

- [ ] Pipeline tests match Python stage semantics.

---

## Phase 15 — Build a Python-vs-Rust parity harness

Create a migration-only harness/script or test utility.

For each non-mutating fixture:

- [ ] run Python implementation;
- [ ] run Rust implementation;
- [ ] normalize nondeterministic temp paths;
- [ ] compare success/failure;
- [ ] compare selected package;
- [ ] compare version/tag;
- [ ] compare metadata findings;
- [ ] compare planned targets;
- [ ] compare artifact names;
- [ ] compare release destination.

For repair/preparation fixtures:

- [ ] run each in separate disposable copies;
- [ ] compare meaningful TOML values;
- [ ] compare changed-file set;
- [ ] compare generated lockfile presence;
- [ ] compare Cross.toml semantics;
- [ ] compare Git operations planned.

For packaging:

- [ ] compare asset names;
- [ ] compare archive member paths;
- [ ] compare binary names/modes;
- [ ] compare checksum correctness.

### Gate

- [ ] No unexplained parity differences remain.

---

## Phase 16 — Real disposable repository validation

Use a repository specifically safe for integration testing.

### Preparation

- [ ] Rust tool inspects it successfully.
- [ ] Dry-run is correct.
- [ ] Rust preparation succeeds.
- [ ] Commit creation works when explicitly authorized.
- [ ] Annotated tag creation works when explicitly authorized.
- [ ] Atomic push works when explicitly authorized.

### Build/package

- [ ] Cargo check passes.
- [ ] Cargo tests pass.
- [ ] All default cross builds pass.
- [ ] Archives are produced.
- [ ] SHA256SUMS is correct.

### Publication

- [ ] Matching remote tag is validated.
- [ ] Release is created or reused.
- [ ] All assets upload.
- [ ] Anonymous downloads work.
- [ ] Checksums/layouts verify.
- [ ] cargo-binstall installs from the live release into a temporary location.

### Safety

- [ ] No unrelated repo files modified.
- [ ] No unrelated release assets touched.
- [ ] No token appears in logs.
- [ ] No force push.
- [ ] No tag movement.

### Gate

- [ ] One complete real Rust-driven release succeeds end to end.

---

## Phase 17 — Cut over the entry point

Only after the prior gates pass:

- [ ] Make the Rust executable the documented/default tool.
- [ ] If `release.sh` remains, make it a thin compatibility launcher only.
- [ ] Remove venv checks from the active workflow.
- [ ] Remove `requirements.txt` from required setup.
- [ ] Remove Python package installation from README.
- [ ] Update examples to invoke the Rust binary.
- [ ] Update CI/Jenkins to build/test Rust.
- [ ] Add release/install instructions for this tool itself if desired.

### Gate

- [ ] A clean machine with Rust/runtime prerequisites but no Python environment can use the tool.

---

## Phase 18 — Remove the Python implementation

Do this last.

- [ ] Confirm no README command references Python worker.
- [ ] Confirm no CI job requires Python worker.
- [ ] Confirm no fixture/test depends solely on Python code.
- [ ] Preserve useful behavioral tests in Rust.
- [ ] Delete `worker/`.
- [ ] Delete Python-only test code.
- [ ] Delete `requirements.txt`.
- [ ] Simplify/remove obsolete venv `release.sh` logic.
- [ ] Remove parity harness if it has no long-term value.
- [ ] Keep migration rationale in Git history / docs as appropriate.

### Gate

- [ ] `git grep` finds no accidental active Python-runtime dependency.
- [ ] Rust test suite still passes.

---

## Phase 19 — Final quality pass

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

Also run the opt-in integration matrix available in the environment.

Check:

- [ ] no normal-path `unwrap()`/`expect()` on external/user input;
- [ ] no secret leaks;
- [ ] no shell interpolation hazards;
- [ ] archive path safety;
- [ ] helpful errors;
- [ ] README reflects reality;
- [ ] `AGENTS.md` reflects final architecture;
- [ ] obsolete migration notes are clearly historical rather than active instructions.

---

## Final definition of done

- [ ] Native Rust CLI replaces Python worker.
- [ ] No venv is required.
- [ ] Existing supported flags/config semantics are preserved.
- [ ] Workspace `--package` behavior is preserved.
- [ ] Metadata validation/repair is preserved.
- [ ] Preparation + explicit commit/tag/push is preserved.
- [ ] Tag safety is preserved.
- [ ] Fresh-target build behavior is preserved.
- [ ] Default three-target cross build is preserved.
- [ ] `.tgz` naming/layout is preserved.
- [ ] SHA256SUMS is preserved.
- [ ] Gitea release creation/reuse works.
- [ ] Asset collision/refusal/replacement behavior is preserved.
- [ ] Asset-list pagination regression remains fixed.
- [ ] Anonymous artifact validation works.
- [ ] Real isolated cargo-binstall verification works.
- [ ] Token security/redaction passes.
- [ ] Rust formatting, Clippy, unit, mocked, and integration tests pass.
- [ ] At least one live disposable end-to-end Rust release has succeeded.
- [ ] Python runtime/dependencies are removable with no feature regression.

---

## Historical compatibility notes worth preserving during the rewrite

These are not new tasks; they are reminders of bugs/edge cases already discovered in real use:

- [ ] A previous release-assets implementation incorrectly paginated an endpoint that returned the full attachment list, causing repeated nonempty results. Preserve the regression test.
- [ ] Reused Cargo target directories caused stale build-script Git revision metadata to leak into release binaries. Preserve fresh release target directories.
- [ ] Cross builds may require target-compatible native libraries; do not mask failures such as missing target SQLite.
- [ ] Some repositories use inherited workspace metadata. Use Cargo's resolved package information while editing only the selected member manifest.
- [ ] Existing tags must never be silently moved.
- [ ] Release asset replacement must remain explicitly authorized.
