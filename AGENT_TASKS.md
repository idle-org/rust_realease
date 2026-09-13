# AGENT_TASKS.md

## Repository preparation and corrected assets (2026-09-13)

- Corrected efo v0.1.0 assets and SHA256SUMS were explicitly authorized for replacement, replaced successfully, and reverified via anonymous downloads and real cargo-binstall installation for all three targets. These assets embed the tagged e9cf45c revision.
- Fixed a live-discovered API bug: Gitea's release-assets endpoint returns all attachments without pagination. The former pagination loop repeated the same nonempty list indefinitely; the client now reads it once, with collision/replacement regression coverage.
- Added `--prepare-repo`, `--commit-preparation`, `--create-tag`, `--push-preparation`, `--cross-image-tag`, and `--tag-prefix`.
- Preparation previews diffs, repairs known release inputs, generates missing lockfiles, forwards the selected toolchain through Cross.toml, and can explicitly commit/tag/push. It refuses dirty commit inputs, existing-tag moves, mismatched push destinations, and implicit Git writes. Remote preparation checkouts are retained.
- All real release builds now use fresh target subdirectories, eliminating reused build-script Git revision data.
- 43 unit/mocked tests passed; the two opt-in cargo-binstall tests had already passed, and live replacement reverified all three installations.
- The new prepare mode was also tested on epoch_forge's original 13eb69c source in a disposable checkout. It created signed local commit ec46455 and a local v0.1.0 tag, then all three locked cross-builds passed with the original build scripts, no manual environment overrides, and a clean Git worktree. The native --version embeds ec46455 correctly. This separate validation checkout was not pushed or published.

## epoch_forge live publication (2026-09-13)

After the user updated permissions, the atomic push succeeded: main and v0.1.0 now reference fix commit e9cf45ca7a52dda4b794b0a55764e6515255b625. The publisher created the live v0.1.0 release, uploaded all three archives plus SHA256SUMS, verified anonymous downloads/checksums/layouts, and successfully installed all three targets with cargo-binstall into temporary directories.

A final native --version check found cached build-script output embedded the previous commit ID (13eb69c). A fresh target-directory rebuild now reports e9cf45c and all corrected local archives pass checksum/layout validation. Corrected assets are staged at `/tmp/binstall-publisher-epoch-forge/dist/release-ls5v7la8`, using target directory `/tmp/binstall-epoch-tagged-target`. Replacing the four published assets awaits explicit authorization under the asset-collision policy. No tag move or version bump is needed.

## epoch_forge release-fix push attempt (2026-09-13)

The user explicitly authorized fixes, commits, pushes and necessary tag changes.

- Fixed both build scripts to use rustc when RUSTUP_TOOLCHAIN is absent, avoiding a rustup requirement inside cross images.
- Added the tested Cross.toml image matrix, retained the already-tracked Cargo.lock, ignored dist output, added binstall metadata, and documented release/install commands.
- Locked check/test and all three cross builds passed without temporary toolchain/image environment overrides.
- Created signed commit `e9cf45ca7a52dda4b794b0a55764e6515255b625` and annotated local tag `v0.1.0` in `/tmp/binstall-publisher-epoch-forge`.
- Attempted an atomic push of main and v0.1.0 with the supplied token. Gitea rejected both refs with `User permission denied for writing`. No remote branch or tag changes occurred, and no release was published.
- Retained a portable patch at `/tmp/epoch-forge-release-fix.patch`. Continuing requires credentials whose account has repository code-write permission (and Releases write permission for publication).

## epoch_forge integration (2026-09-13)

- Selected workspace package `efo`, version `0.1.0`, at commit `13eb69cd2b49ed44543386bdc936aff31b8c6e3d`.
- Cargo check/test commands succeeded (the selected package's test run discovered zero tests).
- The initial cross build failed because both build scripts invoke `rustup` when `RUSTUP_TOOLCHAIN` is absent. Retrying with `RUSTUP_TOOLCHAIN=nightly` and `CROSS_CONTAINER_OPTS='--env RUSTUP_TOOLCHAIN=nightly'`, plus the documented `:main` image overrides, built all three default targets without source edits beyond binstall metadata.
- Three archives and SHA256SUMS were produced in `/tmp/binstall-publisher-epoch-forge/dist/release-poo7ocro`; all checksums and layouts were verified. The native musl binary's `--version` command succeeded.
- Read-only authenticated Gitea API requests succeeded. Repository permissions reported `pull: true`, `push: false`, `admin: false`; these code-access flags do not establish the account's separate Releases unit permission.
- Publication remains blocked by the absence of matching tag `v0.1.0`. Existing tags `0.0.1a` and `fullnames` do not match the selected package version. The `v0.1.0` release is also absent. No remote changes were made.

## Workspace selection extension (2026-09-13)

User-authorized extension to the original single-package scope:

- [x] Add `--package NAME` for exact workspace member selection; retain explicit errors for ambiguous, unknown, library-only, and multi-binary selections.
- [x] Use Cargo's resolved inherited version and repository, workspace lockfile/profile/target directory, and selected member manifest.
- [x] Scope Cargo check/test and cross build to the chosen package, with ordinary dependency compilation.
- [x] Repair only the selected member's metadata and verify cargo-binstall against its manifest.
- [x] Add workspace regression tests and real cargo-binstall coverage for inherited workspace versions on all three default targets.
- [x] Confirm `ravenous-tools` inspection selects `crates/ravenous-tools/Cargo.toml`, version `0.3.34`.

Validation: 33 unit/mocked tests and both opt-in cargo-binstall integration tests passed. The workspace integration fixture includes an unrelated member that deliberately fails compilation; selecting the CLI still passes Cargo check/test and all three binstall installations.

Retried the user-authorized live workflow on existing tag `0.3.32` (commit `61206c7a88c6b2d461ddee35fe4d7e2968b97c45`) using `--package ravenous-tools`, an empty tag prefix, `--unlocked`, and the documented cross image overrides. Package inspection, member-only metadata repair, and host check/test completed. The first cross build reached linking and failed with `cannot find -lsqlite3`: the sample's unbundled SQLite dependency is unavailable for the musl target in the cross image. The retained checkout is `/tmp/binstall-publisher-fsd8p_ke/source`. The publisher never reached Gitea API calls; no remote changes were made. This sample still needs a target-compatible SQLite build setup before live publication can be verified.

This supersedes the original rejection of all multi-package workspaces. Multi-package release orchestration and multi-binary packages remain out of scope.

## Implementation status (2026-09-13)

The implementation phases below are complete. Checkboxes describe implemented behavior; live release publication remains an integration gate, not an assumed success.

Validation performed:

- 26 standard-library unit/mocked tests passed, including all stop-stage boundaries, wrapper guards, metadata repair, source cleanup, tag checks, asset replacement, archive checks, and token redaction.
- The opt-in real cargo-binstall integration test passed for all three default targets using a temporary loopback asset server and temporary install paths.
- Cloned `https://proxy.dandaedre.com/gitea/idle-org/john_slop.git` at `a144f47452696ae8bbcc3eb04b357c399b4b12d7` into `/tmp/binstall-publisher-john-slop`.
- Dry-run identified missing repository/binstall metadata, Cargo.lock, and the expected `v0.1.0` tag. Metadata repair was checked in the disposable clone and was idempotent.
- Cargo check and all sample tests passed. All three real cross-builds passed with explicit `:main` cross image overrides after the installed cross 0.2.5 images failed with GLIBC incompatibilities. The required overrides are documented in README.
- Three real archives and SHA256SUMS were produced in `/tmp/binstall-publisher-john-slop/dist/release-ksxg81rg`; their checksums and layouts were independently verified.
- Shell syntax and `git diff --check` passed. No additional formatting/lint tooling was introduced.

Still unverified against live Gitea: release creation/reuse, authenticated asset upload, and consumption of those live assets. The sample has no release tag and this environment has no GITEA_TOKEN. No live releases, tags, remote assets, commits, or pushes were made. The final integration checklist remains open for that reason.

---

## Goal

Finish the `rust-binstall-publisher` proof-of-concept described in `AGENTS.md`.

Work in small, verifiable increments. Prefer completing one stage cleanly before adding the next.

---

## Phase 0 — Establish the project skeleton

- [x] Create the final directory structure from `AGENTS.md`.
- [x] Add `worker/__init__.py`.
- [x] Add `worker/__main__.py`.
- [x] Add `requirements.txt`.
- [x] Add `config.toml`.
- [x] Add or update `README.md`.
- [x] Ensure the package launches through `python -m worker`.

### Acceptance criteria

```bash
source .venv/bin/activate
python -m worker --help
```

works without traceback.

---

## Phase 1 — Harden `release.sh`

- [x] Add `set -euo pipefail`.
- [x] Resolve the script directory reliably.
- [x] Detect whether `VIRTUAL_ENV` is set.
- [x] If `.venv` exists but is not active, refuse and print:
  ```bash
  source .venv/bin/activate
  ```
- [x] If `.venv` does not exist and stdin is interactive, ask to create it.
- [x] On approval:
  - [x] run `python3 -m venv .venv`;
  - [x] upgrade/install pip if desired;
  - [x] install `requirements.txt`;
  - [x] exit after setup.
- [x] Do not pretend to activate the environment from the child shell.
- [x] If another venv is active, warn and ask whether to continue.
- [x] In non-interactive mode, fail instead of prompting.
- [x] End with:
  ```bash
  exec python -m worker "$@"
  ```

### Acceptance criteria

Test all four cases:

1. no venv, no `.venv`;
2. no venv, `.venv` exists;
3. wrong venv active;
4. project `.venv` active.

---

## Phase 2 — CLI and configuration

- [x] Implement a Typer CLI without a redundant mandatory `release` subcommand.
- [x] Add positional `SOURCE`.
- [x] Add:
  - [x] `--tag`
  - [x] repeatable `--target`
  - [x] `--dry-run`
  - [x] `--fix-metadata`
  - [x] `--skip-tests`
  - [x] `--skip-binstall-test`
  - [x] `--draft`
  - [x] `--prerelease`
  - [x] `--replace-assets`
  - [x] `--keep-workdir`
  - [x] `--verbose`
  - [x] `--stop-after`
- [x] Implement `config.py`.
- [x] Parse `config.toml`.
- [x] Merge CLI overrides over config defaults.
- [x] Keep the default build target list in configuration or one centralized policy object.

### Acceptance criteria

```bash
./release.sh --help
```

shows all supported options clearly.

---

## Phase 3 — Command runner

- [x] Implement a shared command runner.
- [x] Do not use `shell=True`.
- [x] Support:
  - [x] captured output;
  - [x] streamed output for long-running builds;
  - [x] working directory;
  - [x] custom environment overrides;
  - [x] verbose command logging.
- [x] Create a dedicated exception for command failures.
- [x] Include useful stderr in errors.
- [x] Hide Python tracebacks by default.

### Acceptance criteria

A deliberately failing command produces a concise readable error.

---

## Phase 4 — Acquire project source

- [x] Accept local paths.
- [x] Accept HTTPS Git URLs.
- [x] Accept SSH-style Git URLs.
- [x] Clone remote repositories into a temporary directory.
- [x] Support checking out `--tag`.
- [x] Record:
  - [x] source URL/path;
  - [x] root path;
  - [x] commit SHA;
  - [x] selected tag.
- [x] Respect `--keep-workdir`.
- [x] Never delete a local source project.

### Acceptance criteria

Both work:

```bash
./release.sh ./sample-project --dry-run
```

and:

```bash
./release.sh https://gitea.example.org/user/sample-project.git --dry-run
```

---

## Phase 5 — Cargo project inspection

- [x] Read `Cargo.toml` with `tomlkit`.
- [x] Run:
  ```bash
  cargo metadata --format-version=1 --no-deps
  ```
- [x] Discover:
  - [x] package name;
  - [x] version;
  - [x] repository URL;
  - [x] binary targets.
- [x] Detect workspace-only roots.
- [x] Detect multiple matching packages.
- [x] Detect multiple binaries.
- [x] For unsupported ambiguous cases, stop with a clear message instead of guessing.
- [x] Populate shared dataclasses in `models.py`.

### Acceptance criteria

Dry-run prints a project summary containing:

```text
package
version
repository
binary
commit
tag
```

---

## Phase 6 — Tag/version validation

- [x] Read `tag-prefix` from config.
- [x] Generate expected tag from Cargo version.
- [x] If `--tag` is supplied, require an exact match by default.
- [x] Fail before build on mismatch.
- [x] Add unit tests.

### Acceptance criteria

For version `0.3.1`:

```text
expected: v0.3.1
```

and `--tag v0.3.0` fails clearly.

---

## Phase 7 — cargo-binstall metadata policy

- [x] Centralize artifact naming policy.
- [x] Centralize archive root-directory naming policy.
- [x] Centralize expected cargo-binstall metadata.
- [x] Use:
  ```text
  {name}-{target}-v{version}.tgz
  ```
- [x] Use the corresponding directory name inside the archive.
- [x] Add target-aware binary extension handling.
- [x] Inspect `[package.metadata.binstall]`.
- [x] Report:
  - [x] missing section;
  - [x] missing keys;
  - [x] mismatched values.
- [x] Represent issues as structured objects rather than plain strings.
- [x] Add unit tests for generated metadata.

### Acceptance criteria

Dry-run can show:

```text
Binstall metadata
  missing: package.metadata.binstall
  proposed pkg-url: ...
  proposed pkg-fmt: tgz
  proposed bin-dir: ...
```

without modifying the file.

---

## Phase 8 — Metadata repair

- [x] Implement `--fix-metadata`.
- [x] Preserve Cargo.toml formatting/comments with `tomlkit`.
- [x] Change only the relevant metadata.
- [x] Print a concise before/after or list of changed keys.
- [x] Do not commit.
- [x] Do not push.
- [x] In dry-run, never write the file even if `--fix-metadata` is present.
- [x] Add tests using temporary Cargo.toml fixtures.

### Acceptance criteria

Running twice is idempotent:

```bash
./release.sh ./project --fix-metadata
./release.sh ./project --fix-metadata
```

The second run should report no further metadata change.

---

## Phase 9 — Prerequisite validation

- [x] Check `git`.
- [x] Check `cargo`.
- [x] Check `cross`.
- [x] Check the container backend used by `cross`.
- [x] Check Cargo.lock expectations if `locked = true`.
- [x] Report all missing prerequisites together where practical.
- [x] In dry-run, report status without building.

### Acceptance criteria

Example output:

```text
Tools
  ✓ git
  ✓ cargo
  ✓ cross
  ✓ docker
```

or a clear grouped error.

---

## Phase 10 — Pre-build verification

- [x] Run `cargo metadata`.
- [x] Run a fast project check before cross compiling.
- [x] Prefer:
  ```bash
  cargo check --locked
  ```
  when locked mode is enabled.
- [x] Honor `--skip-tests` if test/check behavior expands.
- [x] Stop before expensive builds if verification fails.

### Acceptance criteria

A broken Rust project fails before `cross build` starts.

---

## Phase 11 — Build implementation

- [x] Implement `build.py`.
- [x] Build each configured target using `cross`.
- [x] Default command:
  ```bash
  cross build --release --locked --target TARGET
  ```
- [x] Respect configured profile.
- [x] Respect locked/unlocked mode.
- [x] Stream compiler output.
- [x] Locate the produced binary deterministically.
- [x] Record build outputs in artifact/build dataclasses.
- [x] Stop on the first failed target.
- [x] Print target progress.

### Acceptance criteria

All three default binaries are produced for a compatible sample project.

---

## Phase 12 — Packaging

- [x] Implement `package.py`.
- [x] Create or clean a controlled `dist/` directory.
- [x] For each target:
  - [x] create archive root directory;
  - [x] copy the binary;
  - [x] preserve Unix executable bit;
  - [x] create `.tgz`;
  - [x] remove unnecessary staging content if appropriate.
- [x] Generate deterministic archive filenames.
- [x] Generate SHA-256 for each archive.
- [x] Generate `SHA256SUMS`.
- [x] Add checksum unit tests.

### Acceptance criteria

Expected result resembles:

```text
dist/
├── mytool-x86_64-unknown-linux-musl-v0.3.1.tgz
├── mytool-aarch64-unknown-linux-musl-v0.3.1.tgz
├── mytool-x86_64-pc-windows-gnu-v0.3.1.tgz
└── SHA256SUMS
```

Each archive contains only its expected root directory and binary.

---

## Phase 13 — Gitea client

- [x] Implement `gitea.py` using `httpx`.
- [x] Read token from configured environment variable.
- [x] Never print the token.
- [x] Parse Gitea base URL and owner/repository safely.
- [x] Support explicit CLI/config override if repository URL inference is ambiguous.
- [x] Implement:
  - [x] release lookup by tag;
  - [x] release creation;
  - [x] asset listing;
  - [x] asset upload;
  - [x] asset deletion for replacement mode.
- [x] Handle HTTP errors with useful messages.
- [x] Structure API methods so they can be mocked in tests.

### Acceptance criteria

With mocked HTTP responses, tests cover:

- release missing;
- release exists;
- upload success;
- duplicate asset;
- replacement flow;
- authentication failure.

---

## Phase 14 — Release creation/reuse

- [x] Derive release name from tag.
- [x] Reuse an existing release with the requested tag.
- [x] Otherwise create one.
- [x] Honor:
  - [x] `--draft`
  - [x] `--prerelease`
- [x] Do not create releases in `--dry-run`.
- [x] Print the intended release destination during dry-run.

### Acceptance criteria

Repeated publication attempts do not create duplicate releases for the same tag.

---

## Phase 15 — Asset upload and collision behavior

- [x] Upload all `.tgz` files.
- [x] Upload `SHA256SUMS`.
- [x] Before upload, inspect existing asset names.
- [x] Default on collision: fail.
- [x] With `--replace-assets`: delete existing matching asset and upload replacement.
- [x] Never rename duplicates automatically.
- [x] Print uploaded asset names.

### Acceptance criteria

Running a second time without `--replace-assets` fails safely.

Running with `--replace-assets` replaces exact matching assets only.

---

## Phase 16 — Published artifact verification

- [x] Implement `verify.py`.
- [x] Verify each expected published asset can be resolved.
- [x] Download to a temporary location where practical.
- [x] Check SHA-256 against local artifact.
- [x] Open the archive.
- [x] Verify expected root directory.
- [x] Verify expected binary filename.
- [x] Ensure verification never overwrites local project binaries.

### Acceptance criteria

A deliberately malformed or mismatched uploaded archive is detected.

---

## Phase 17 — cargo-binstall verification

- [x] Verify the cargo-binstall contract after upload.
- [x] Prefer a temporary install location.
- [x] Avoid modifying the user’s normal Cargo bin directory.
- [x] Use the local manifest when appropriate.
- [x] Verify at least the current host-compatible target automatically.
- [x] For non-host targets, validate URL/archive structure even if execution is impossible.
- [x] Honor `--skip-binstall-test`.

### Acceptance criteria

On a supported host, the just-published host-target binary can be installed into a temporary bin directory via cargo-binstall or an equivalent safe verification path.

---

## Phase 18 — `--stop-after`

- [x] Implement the stage enum.
- [x] Support:
  - [x] `inspect`
  - [x] `metadata`
  - [x] `verify`
  - [x] `build`
  - [x] `package`
  - [x] `release`
  - [x] `upload`
  - [x] `binstall`
- [x] Stop successfully immediately after the requested completed stage.
- [x] Ensure cleanup still runs.

### Acceptance criteria

```bash
./release.sh ./project --stop-after package
```

builds/packages but never contacts Gitea.

---

## Phase 19 — Dry-run completion

- [x] Confirm `--dry-run` performs no mutation.
- [x] It must not:
  - [x] edit Cargo.toml;
  - [x] build;
  - [x] create releases;
  - [x] delete assets;
  - [x] upload.
- [x] It should report:
  - [x] project;
  - [x] version/tag;
  - [x] metadata status;
  - [x] proposed fixes;
  - [x] prerequisite status;
  - [x] build matrix;
  - [x] expected artifact names;
  - [x] expected release destination.

### Acceptance criteria

A dry-run can be used safely against an important repository.

---

## Phase 20 — Documentation

- [x] Update README with real behavior only.
- [x] Document installation:
  ```bash
  python3 -m venv .venv
  source .venv/bin/activate
  pip install -r requirements.txt
  ```
- [x] Document wrapper-assisted venv creation.
- [x] Document prerequisites:
  - [x] Rust/Cargo
  - [x] cross
  - [x] Docker/Podman
  - [x] cargo-binstall for final verification
- [x] Document Gitea token:
  ```bash
  export GITEA_TOKEN="..."
  ```
- [x] Document dry-run.
- [x] Document metadata repair.
- [x] Document target list.
- [x] Document artifact naming.
- [x] Document known limitations.

---

## Phase 21 — Tests and cleanup

- [x] Add unit tests for pure logic.
- [x] Add manifest fixture tests.
- [x] Add mocked Gitea tests.
- [x] Ensure no token is printed in tests/logging.
- [x] Run Python formatting/linting if tooling is added.
- [x] Run tests.
- [x] Remove dead code.
- [x] Ensure no temporary release artifacts are committed unintentionally.
- [x] Ensure `.venv/` is gitignored.
- [x] Ensure Python cache files are gitignored.
- [x] Ensure `dist/` is ignored unless intentionally retained.

Suggested `.gitignore`:

```text
.venv/
__pycache__/
*.py[cod]
.pytest_cache/
dist/
```

---

# Final integration test

Use a real or disposable single-binary Rust project on Gitea.

## Setup

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt

export GITEA_TOKEN="..."
```

## Inspect

```bash
./release.sh \
  https://gitea.example.org/owner/tool.git \
  --tag v0.1.0 \
  --dry-run
```

Confirm:

- [ ] project recognized;
- [ ] tag/version valid;
- [ ] binstall metadata status shown;
- [ ] targets shown;
- [ ] prerequisites shown;
- [ ] no mutation.

## Publish

```bash
./release.sh \
  https://gitea.example.org/owner/tool.git \
  --tag v0.1.0 \
  --fix-metadata
```

Confirm:

- [ ] metadata corrected locally if needed;
- [ ] all target builds succeed;
- [ ] all archives exist;
- [ ] `SHA256SUMS` exists;
- [ ] release exists;
- [ ] assets are attached;
- [ ] published artifacts verify;
- [ ] cargo-binstall verification succeeds for the host-compatible artifact.

---

# Non-goals for this PoC

Do not delay completion for these:

- [ ] automatic Git commits;
- [ ] automatic Git pushes;
- [ ] automatic crate publishing to crates.io;
- [ ] Windows MSVC cross-compilation;
- [ ] macOS artifacts;
- [ ] multi-package workspace orchestration;
- [ ] multiple binary selection UI;
- [ ] artifact signing;
- [ ] release changelog generation;
- [ ] CI integration;
- [ ] parallel cross builds.

These can be follow-up work once the single-package release path is reliable.

