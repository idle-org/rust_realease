# AGENT_TASKS.md

## Goal

Finish the `rust-binstall-publisher` proof-of-concept described in `AGENT.md`.

Work in small, verifiable increments. Prefer completing one stage cleanly before adding the next.

---

## Phase 0 — Establish the project skeleton

- [ ] Create the final directory structure from `AGENT.md`.
- [ ] Add `worker/__init__.py`.
- [ ] Add `worker/__main__.py`.
- [ ] Add `requirements.txt`.
- [ ] Add `config.toml`.
- [ ] Add or update `README.md`.
- [ ] Ensure the package launches through `python -m worker`.

### Acceptance criteria

```bash
source .venv/bin/activate
python -m worker --help
```

works without traceback.

---

## Phase 1 — Harden `release.sh`

- [ ] Add `set -euo pipefail`.
- [ ] Resolve the script directory reliably.
- [ ] Detect whether `VIRTUAL_ENV` is set.
- [ ] If `.venv` exists but is not active, refuse and print:
  ```bash
  source .venv/bin/activate
  ```
- [ ] If `.venv` does not exist and stdin is interactive, ask to create it.
- [ ] On approval:
  - [ ] run `python3 -m venv .venv`;
  - [ ] upgrade/install pip if desired;
  - [ ] install `requirements.txt`;
  - [ ] exit after setup.
- [ ] Do not pretend to activate the environment from the child shell.
- [ ] If another venv is active, warn and ask whether to continue.
- [ ] In non-interactive mode, fail instead of prompting.
- [ ] End with:
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

- [ ] Implement a Typer CLI without a redundant mandatory `release` subcommand.
- [ ] Add positional `SOURCE`.
- [ ] Add:
  - [ ] `--tag`
  - [ ] repeatable `--target`
  - [ ] `--dry-run`
  - [ ] `--fix-metadata`
  - [ ] `--skip-tests`
  - [ ] `--skip-binstall-test`
  - [ ] `--draft`
  - [ ] `--prerelease`
  - [ ] `--replace-assets`
  - [ ] `--keep-workdir`
  - [ ] `--verbose`
  - [ ] `--stop-after`
- [ ] Implement `config.py`.
- [ ] Parse `config.toml`.
- [ ] Merge CLI overrides over config defaults.
- [ ] Keep the default build target list in configuration or one centralized policy object.

### Acceptance criteria

```bash
./release.sh --help
```

shows all supported options clearly.

---

## Phase 3 — Command runner

- [ ] Implement a shared command runner.
- [ ] Do not use `shell=True`.
- [ ] Support:
  - [ ] captured output;
  - [ ] streamed output for long-running builds;
  - [ ] working directory;
  - [ ] custom environment overrides;
  - [ ] verbose command logging.
- [ ] Create a dedicated exception for command failures.
- [ ] Include useful stderr in errors.
- [ ] Hide Python tracebacks by default.

### Acceptance criteria

A deliberately failing command produces a concise readable error.

---

## Phase 4 — Acquire project source

- [ ] Accept local paths.
- [ ] Accept HTTPS Git URLs.
- [ ] Accept SSH-style Git URLs.
- [ ] Clone remote repositories into a temporary directory.
- [ ] Support checking out `--tag`.
- [ ] Record:
  - [ ] source URL/path;
  - [ ] root path;
  - [ ] commit SHA;
  - [ ] selected tag.
- [ ] Respect `--keep-workdir`.
- [ ] Never delete a local source project.

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

- [ ] Read `Cargo.toml` with `tomlkit`.
- [ ] Run:
  ```bash
  cargo metadata --format-version=1 --no-deps
  ```
- [ ] Discover:
  - [ ] package name;
  - [ ] version;
  - [ ] repository URL;
  - [ ] binary targets.
- [ ] Detect workspace-only roots.
- [ ] Detect multiple matching packages.
- [ ] Detect multiple binaries.
- [ ] For unsupported ambiguous cases, stop with a clear message instead of guessing.
- [ ] Populate shared dataclasses in `models.py`.

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

- [ ] Read `tag-prefix` from config.
- [ ] Generate expected tag from Cargo version.
- [ ] If `--tag` is supplied, require an exact match by default.
- [ ] Fail before build on mismatch.
- [ ] Add unit tests.

### Acceptance criteria

For version `0.3.1`:

```text
expected: v0.3.1
```

and `--tag v0.3.0` fails clearly.

---

## Phase 7 — cargo-binstall metadata policy

- [ ] Centralize artifact naming policy.
- [ ] Centralize archive root-directory naming policy.
- [ ] Centralize expected cargo-binstall metadata.
- [ ] Use:
  ```text
  {name}-{target}-v{version}.tgz
  ```
- [ ] Use the corresponding directory name inside the archive.
- [ ] Add target-aware binary extension handling.
- [ ] Inspect `[package.metadata.binstall]`.
- [ ] Report:
  - [ ] missing section;
  - [ ] missing keys;
  - [ ] mismatched values.
- [ ] Represent issues as structured objects rather than plain strings.
- [ ] Add unit tests for generated metadata.

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

- [ ] Implement `--fix-metadata`.
- [ ] Preserve Cargo.toml formatting/comments with `tomlkit`.
- [ ] Change only the relevant metadata.
- [ ] Print a concise before/after or list of changed keys.
- [ ] Do not commit.
- [ ] Do not push.
- [ ] In dry-run, never write the file even if `--fix-metadata` is present.
- [ ] Add tests using temporary Cargo.toml fixtures.

### Acceptance criteria

Running twice is idempotent:

```bash
./release.sh ./project --fix-metadata
./release.sh ./project --fix-metadata
```

The second run should report no further metadata change.

---

## Phase 9 — Prerequisite validation

- [ ] Check `git`.
- [ ] Check `cargo`.
- [ ] Check `cross`.
- [ ] Check the container backend used by `cross`.
- [ ] Check Cargo.lock expectations if `locked = true`.
- [ ] Report all missing prerequisites together where practical.
- [ ] In dry-run, report status without building.

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

- [ ] Run `cargo metadata`.
- [ ] Run a fast project check before cross compiling.
- [ ] Prefer:
  ```bash
  cargo check --locked
  ```
  when locked mode is enabled.
- [ ] Honor `--skip-tests` if test/check behavior expands.
- [ ] Stop before expensive builds if verification fails.

### Acceptance criteria

A broken Rust project fails before `cross build` starts.

---

## Phase 11 — Build implementation

- [ ] Implement `build.py`.
- [ ] Build each configured target using `cross`.
- [ ] Default command:
  ```bash
  cross build --release --locked --target TARGET
  ```
- [ ] Respect configured profile.
- [ ] Respect locked/unlocked mode.
- [ ] Stream compiler output.
- [ ] Locate the produced binary deterministically.
- [ ] Record build outputs in artifact/build dataclasses.
- [ ] Stop on the first failed target.
- [ ] Print target progress.

### Acceptance criteria

All three default binaries are produced for a compatible sample project.

---

## Phase 12 — Packaging

- [ ] Implement `package.py`.
- [ ] Create or clean a controlled `dist/` directory.
- [ ] For each target:
  - [ ] create archive root directory;
  - [ ] copy the binary;
  - [ ] preserve Unix executable bit;
  - [ ] create `.tgz`;
  - [ ] remove unnecessary staging content if appropriate.
- [ ] Generate deterministic archive filenames.
- [ ] Generate SHA-256 for each archive.
- [ ] Generate `SHA256SUMS`.
- [ ] Add checksum unit tests.

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

- [ ] Implement `gitea.py` using `httpx`.
- [ ] Read token from configured environment variable.
- [ ] Never print the token.
- [ ] Parse Gitea base URL and owner/repository safely.
- [ ] Support explicit CLI/config override if repository URL inference is ambiguous.
- [ ] Implement:
  - [ ] release lookup by tag;
  - [ ] release creation;
  - [ ] asset listing;
  - [ ] asset upload;
  - [ ] asset deletion for replacement mode.
- [ ] Handle HTTP errors with useful messages.
- [ ] Structure API methods so they can be mocked in tests.

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

- [ ] Derive release name from tag.
- [ ] Reuse an existing release with the requested tag.
- [ ] Otherwise create one.
- [ ] Honor:
  - [ ] `--draft`
  - [ ] `--prerelease`
- [ ] Do not create releases in `--dry-run`.
- [ ] Print the intended release destination during dry-run.

### Acceptance criteria

Repeated publication attempts do not create duplicate releases for the same tag.

---

## Phase 15 — Asset upload and collision behavior

- [ ] Upload all `.tgz` files.
- [ ] Upload `SHA256SUMS`.
- [ ] Before upload, inspect existing asset names.
- [ ] Default on collision: fail.
- [ ] With `--replace-assets`: delete existing matching asset and upload replacement.
- [ ] Never rename duplicates automatically.
- [ ] Print uploaded asset names.

### Acceptance criteria

Running a second time without `--replace-assets` fails safely.

Running with `--replace-assets` replaces exact matching assets only.

---

## Phase 16 — Published artifact verification

- [ ] Implement `verify.py`.
- [ ] Verify each expected published asset can be resolved.
- [ ] Download to a temporary location where practical.
- [ ] Check SHA-256 against local artifact.
- [ ] Open the archive.
- [ ] Verify expected root directory.
- [ ] Verify expected binary filename.
- [ ] Ensure verification never overwrites local project binaries.

### Acceptance criteria

A deliberately malformed or mismatched uploaded archive is detected.

---

## Phase 17 — cargo-binstall verification

- [ ] Verify the cargo-binstall contract after upload.
- [ ] Prefer a temporary install location.
- [ ] Avoid modifying the user’s normal Cargo bin directory.
- [ ] Use the local manifest when appropriate.
- [ ] Verify at least the current host-compatible target automatically.
- [ ] For non-host targets, validate URL/archive structure even if execution is impossible.
- [ ] Honor `--skip-binstall-test`.

### Acceptance criteria

On a supported host, the just-published host-target binary can be installed into a temporary bin directory via cargo-binstall or an equivalent safe verification path.

---

## Phase 18 — `--stop-after`

- [ ] Implement the stage enum.
- [ ] Support:
  - [ ] `inspect`
  - [ ] `metadata`
  - [ ] `verify`
  - [ ] `build`
  - [ ] `package`
  - [ ] `release`
  - [ ] `upload`
  - [ ] `binstall`
- [ ] Stop successfully immediately after the requested completed stage.
- [ ] Ensure cleanup still runs.

### Acceptance criteria

```bash
./release.sh ./project --stop-after package
```

builds/packages but never contacts Gitea.

---

## Phase 19 — Dry-run completion

- [ ] Confirm `--dry-run` performs no mutation.
- [ ] It must not:
  - [ ] edit Cargo.toml;
  - [ ] build;
  - [ ] create releases;
  - [ ] delete assets;
  - [ ] upload.
- [ ] It should report:
  - [ ] project;
  - [ ] version/tag;
  - [ ] metadata status;
  - [ ] proposed fixes;
  - [ ] prerequisite status;
  - [ ] build matrix;
  - [ ] expected artifact names;
  - [ ] expected release destination.

### Acceptance criteria

A dry-run can be used safely against an important repository.

---

## Phase 20 — Documentation

- [ ] Update README with real behavior only.
- [ ] Document installation:
  ```bash
  python3 -m venv .venv
  source .venv/bin/activate
  pip install -r requirements.txt
  ```
- [ ] Document wrapper-assisted venv creation.
- [ ] Document prerequisites:
  - [ ] Rust/Cargo
  - [ ] cross
  - [ ] Docker/Podman
  - [ ] cargo-binstall for final verification
- [ ] Document Gitea token:
  ```bash
  export GITEA_TOKEN="..."
  ```
- [ ] Document dry-run.
- [ ] Document metadata repair.
- [ ] Document target list.
- [ ] Document artifact naming.
- [ ] Document known limitations.

---

## Phase 21 — Tests and cleanup

- [ ] Add unit tests for pure logic.
- [ ] Add manifest fixture tests.
- [ ] Add mocked Gitea tests.
- [ ] Ensure no token is printed in tests/logging.
- [ ] Run Python formatting/linting if tooling is added.
- [ ] Run tests.
- [ ] Remove dead code.
- [ ] Ensure no temporary release artifacts are committed unintentionally.
- [ ] Ensure `.venv/` is gitignored.
- [ ] Ensure Python cache files are gitignored.
- [ ] Ensure `dist/` is ignored unless intentionally retained.

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

