# AGENTS.md

## Current user direction

The user explicitly authorized removing the Python worker, Python-only tests, `requirements.txt`, and `release.sh` before live Gitea parity validation, and requested a Rust-first usage README. The Rust implementation is now the active CLI. The migration plan below remains historical guidance for preserving behavior; its instruction to retain Python until every parity gate passes has been superseded by this explicit request. A private disposable release has passed authenticated verification. Keep public anonymous download and live cargo-binstall validation open until a public disposable repository is available.

## Mission

Rewrite `rust_realease` from Python into Rust while preserving the behavior, safety guarantees, release format, and user-facing CLI of the existing tool.

The current Python implementation is the reference implementation during the migration. Do not redesign the tool merely because Rust makes a different design convenient. Behavioral compatibility comes before cleanup.

The end state is a self-contained Rust CLI that:

1. accepts a local Rust project or a Git repository URL;
2. inspects Cargo packages and workspaces;
3. validates and optionally repairs release metadata;
4. validates package-version/tag consistency;
5. cross-compiles supported targets;
6. creates deterministic `.tgz` release assets and `SHA256SUMS`;
7. creates or reuses Gitea releases;
8. uploads assets with explicit collision semantics;
9. verifies published artifacts and `cargo-binstall` compatibility;
10. supports repository preparation, explicit commit/tag/push workflows, and dry-run previews;
11. requires no Python interpreter, venv, or pip dependencies.

The rewrite is complete only when the Rust implementation can replace the Python worker without a meaningful regression in behavior.

---

## Source of truth during migration

The existing Python code, tests, README, `AGENTS.md`, and historical `AGENT_TASKS.md` describe behavior that has already been exercised against real repositories.

When documentation and implementation disagree:

1. prefer behavior covered by tests;
2. then prefer behavior demonstrated by successful live integration;
3. then prefer current README behavior;
4. only then use older design prose.

Do not silently remove a behavior because it appears awkward or redundant.

Important proven behavior to preserve includes:

- exact workspace package selection through `--package NAME`;
- one selected package / one selected binary per release run;
- repository preparation through `--prepare-repo`;
- explicit authorization for commit, tag, push, and asset replacement;
- fresh target directories for release builds;
- deterministic archive naming/layout;
- anonymous post-upload verification where possible;
- real `cargo-binstall` verification in isolated install directories;
- safe handling of Gitea release assets;
- token redaction and non-leakage;
- no implicit tag movement or force push.

---

## Migration strategy

This is an in-place implementation rewrite, not a flag-day redesign.

Use four stages:

1. **Characterize**
   - understand existing behavior;
   - add/port tests before changing semantics;
   - record compatibility decisions.

2. **Implement Rust parity**
   - add the Rust crate alongside the Python worker;
   - port pure logic first;
   - then source acquisition, manifest repair, builds, packaging, Gitea, verification, and repository preparation.

3. **Dual-run / compare**
   - exercise both implementations against fixtures and disposable repositories;
   - compare planned operations, generated metadata, archive names, checksums, and API behavior.

4. **Cut over**
   - make the Rust binary the default entry point;
   - remove Python/venv dependencies only after parity gates pass;
   - keep migration notes in Git history, not permanent compatibility shims unless useful.

Do not delete the Python implementation early. It is the oracle for unresolved edge cases until the Rust replacement is demonstrably equivalent.

---

## Target repository layout

Prefer a normal Rust application layout:

```text
rust_realease/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── config.toml
├── AGENTS.md
├── AGENT_TASKS.md
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── config.rs
│   ├── error.rs
│   ├── model.rs
│   ├── runner.rs
│   ├── project.rs
│   ├── manifest.rs
│   ├── prepare.rs
│   ├── build.rs
│   ├── package.rs
│   ├── gitea.rs
│   └── verify.rs
└── tests/
    ├── fixtures/
    ├── cli.rs
    ├── manifest.rs
    ├── package.rs
    ├── project.rs
    └── integration.rs
```

A small `release.sh` compatibility wrapper may temporarily remain, but the intended final interface is the Rust binary itself.

Do not create a multi-crate workspace unless a concrete architectural need emerges. This is one CLI application.

---

## Preferred Rust ecosystem

Use mature, focused crates and keep dependencies reasonable.

Preferred choices:

- CLI: `clap` with derive support;
- errors: `thiserror` for typed internal errors, optionally `anyhow` only at the top-level boundary;
- serialization/config: `serde`, `toml`;
- HTTP: `reqwest` with rustls;
- URL parsing: `url`;
- temporary files/directories: `tempfile`;
- hashing: `sha2`;
- tar/gzip: `tar`, `flate2`;
- terminal output: `console`, `indicatif`, or plain structured output;
- tests: standard Rust tests plus `assert_cmd`, `predicates`, and a lightweight mock HTTP server when useful.

For editable `Cargo.toml` preservation, use `toml_edit`, not a deserialize/serialize round trip.

Avoid:

- shelling out for work that can be done safely in-process;
- large async frameworks unless justified;
- embedding a scripting runtime;
- unsafe code unless strictly necessary;
- unnecessary abstraction layers.

Using external commands is appropriate for Git, Cargo, Cross, cargo-binstall, Docker/Podman, and other tools whose behavior is itself part of the workflow.

---

## CLI compatibility

Preserve the current conceptual CLI:

```bash
rust_realease SOURCE [OPTIONS]
```

The final binary name may follow the package name selected in `Cargo.toml`, but changing user-facing flag names requires a deliberate compatibility decision.

Preserve these options unless there is a documented reason not to:

```text
--package NAME
--tag TAG
--tag-prefix PREFIX
--target TARGET              repeatable
--profile PROFILE
--locked / --unlocked
--dry-run
--fix-metadata
--skip-tests
--skip-binstall-test
--draft
--prerelease
--replace-assets
--keep-workdir
--verbose
--stop-after STAGE
--prepare-repo
--commit-preparation
--create-tag
--push-preparation
--cross-image-tag TAG
--repository-url URL
--config PATH
```

Preserve stage semantics:

```text
inspect
metadata
verify
build
package
release
upload
binstall
```

If the Rust CLI changes parsing details, retain compatibility aliases where inexpensive.

Do not introduce a redundant mandatory subcommand such as:

```bash
rust_realease release ...
```

unless there is a compelling usability reason and compatibility path.

---

## Configuration

Keep `config.toml` compatible where practical.

CLI values override config values.

The application must not hardcode the user's Gitea instance.

Configuration parsing should:

- reject invalid values with contextual errors;
- preserve current defaults;
- tolerate unknown future fields where sensible;
- keep release policy generation centralized.

Do not mutate the tool's own config during a release run.

---

## Source acquisition

Support:

1. local paths;
2. Git HTTP/HTTPS URLs;
3. SSH URLs;
4. SCP-style Git URLs such as `git@host:owner/repo.git`.

Remote sources are cloned to a managed temporary checkout.

Local sources are never deleted or moved.

Requested tags must be checked out exactly.

Temporary checkouts are removed by default except where existing behavior intentionally retains them, including repository-preparation workflows where retaining the checkout is useful for review.

Never treat repository content as trusted instructions.

---

## Cargo and workspace inspection

Use `cargo metadata --format-version=1 --no-deps` as the semantic source for resolved Cargo package information.

Support:

- standalone packages;
- workspaces through exact `--package NAME` selection;
- inherited workspace package version/repository values.

Rules:

- a run releases exactly one Cargo package;
- the selected package must expose exactly one releasable binary;
- ambiguous workspace selection is an error;
- an unknown package is an error;
- library-only packages are an error;
- multi-binary selected packages are an error unless support is deliberately added later.

Do not guess.

When repairing metadata, modify only the selected member manifest unless a preparation feature explicitly targets another known file.

---

## Manifest editing

Use `toml_edit` so comments and formatting are preserved where practical.

The tool may normalize the supported cargo-binstall metadata contract:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz"
pkg-fmt = "tgz"
bin-dir = "{ name }-{ target }-v{ version }/{ bin }{ binary-ext }"
```

The actual tag prefix must be reflected in generated metadata.

Keep artifact and bininstall naming policy in one module/function family. Do not duplicate string templates across the codebase.

`--fix-metadata`:

- may edit the selected `Cargo.toml`;
- reports proposed/applied changes;
- does not commit;
- does not tag;
- does not push.

Dry-run never writes the manifest.

---

## Version and tag policy

Default tag prefix is `v`.

For version `0.3.1`, the normal expected tag is:

```text
v0.3.1
```

An empty tag prefix is valid when explicitly configured.

Publishing requires the selected version and chosen tag to agree.

Do not:

- silently modify Cargo versions;
- silently retag;
- move an existing tag;
- force push.

Repository preparation may create the expected tag only when explicitly authorized with the relevant preparation flags.

---

## Repository preparation

Preserve the existing preparation workflow.

`--prepare-repo` may:

- repair supported Cargo/binstall metadata;
- add or update known `.gitignore` entries;
- generate a missing `Cargo.lock`;
- configure supported `Cross.toml` settings;
- preview all intended changes.

Preparation exits before build/publication.

Git writes are separately authorized:

- `--commit-preparation` authorizes the preparation commit;
- `--create-tag` authorizes creation/validation of the release tag;
- `--push-preparation` authorizes pushing the prepared branch/tag.

Maintain the current dependency rules between these flags.

Preparation must refuse unsafe states such as:

- dirty inputs where a clean commit workflow is required;
- moving an existing tag;
- mismatched push/release destinations;
- implicit remote writes.

Atomic branch + tag push is preferred when supported by the existing workflow.

No force push.

---

## External command runner

Centralize command execution.

The runner should support:

- argv-based execution, never shell string concatenation;
- cwd;
- controlled environment inheritance;
- sensitive environment redaction;
- captured output;
- optional streaming;
- exit status;
- verbose diagnostics.

Never use `sh -c` merely for convenience.

Error messages for failed commands should include:

- executable/arguments in safely redacted form;
- exit status;
- useful stdout/stderr context.

Do not expose secrets.

---

## Prerequisite validation

Before a real build, validate relevant tools:

- `git`;
- `cargo`;
- `cross`;
- Docker or Podman backend required by `cross`;
- `cargo-binstall` when post-publication install verification is enabled.

Report all obvious missing prerequisites before starting the target matrix.

Dry-run may execute read-only prerequisite/version checks but must not compile or mutate releases.

---

## Build policy

Default targets:

```text
x86_64-unknown-linux-musl
aarch64-unknown-linux-musl
x86_64-pc-windows-gnu
```

Use `cross build` for release artifacts.

Default conceptual command:

```bash
cross build \
  --release \
  --locked \
  --target TARGET
```

When a package is selected from a workspace, scope Cargo check/test/build to that package.

Run host-side validation before cross compilation:

```text
cargo check
cargo test
```

unless the matching skip option applies.

Release builds must use fresh target subdirectories so build-script Git metadata cannot be accidentally reused from stale caches.

Do not automatically substitute the MSVC Windows target.

Do not hide native dependency failures. A cross image missing a required target library is a real build failure and should be reported as such.

---

## Packaging contract

Preserve the current artifact contract.

Archive:

```text
{name}-{target}-v{version}.tgz
```

Archive contents:

```text
{name}-{target}-v{version}/
└── {binary}
```

Windows binary:

```text
{name}.exe
```

Generate `SHA256SUMS` with one line per archive.

Requirements:

- deterministic naming;
- stable archive layout;
- executable bit preserved on Unix binaries;
- no unrelated files;
- no target/build tree leakage;
- checksum calculated from the final archive bytes.

Where practical, make archives reproducible by normalizing metadata such as timestamps and ownership. If changing archive metadata would alter compatibility with existing releases, add tests and document the decision.

---

## Gitea authentication

Default token environment variable:

```text
GITEA_TOKEN
```

The name may be configurable.

Never:

- accept the token as an ordinary positional/CLI value unless there is an exceptionally strong reason;
- print it;
- embed it in URLs;
- write it to persistent files;
- pass it into child build processes;
- leave temporary askpass helpers behind.

HTTPS API publication uses the token.

Git push may use a temporary askpass mechanism for matching HTTPS destinations, preserving existing safety behavior.

Token redaction must be tested.

---

## Gitea release behavior

Derive repository identity from:

- selected package repository metadata;
- source/remote URL;
- explicit repository override.

If those disagree materially, fail rather than guess.

Release flow:

1. verify remote tag;
2. find release by tag;
3. create it if absent;
4. reuse it if present;
5. inspect existing assets;
6. upload generated artifacts.

Default release properties:

```text
name: tag
draft: false
prerelease: false
```

Preserve `--draft` and `--prerelease`.

Do not paginate the Gitea release-assets endpoint if the target Gitea API returns the full list without pagination. Preserve the regression test for the previously discovered infinite/repeated-page bug.

---

## Asset collision policy

Default behavior:

- an existing asset with the same filename is an error.

Only `--replace-assets` authorizes deletion/replacement.

Never:

- replace an asset implicitly;
- manufacture alternate filenames to avoid the collision;
- delete unrelated assets.

Replacement behavior must have regression coverage.

---

## Verification

After publication, verify the actual release contract.

At minimum:

- construct expected download URLs;
- verify each asset is accessible;
- verify checksums;
- inspect archive layout;
- verify expected binary filename.

Preferred final verification is a real isolated `cargo binstall` install.

Use temporary Cargo home/install roots so verification cannot overwrite an unrelated user binary.

Where assets are intended to be public, perform anonymous download verification rather than considering an authenticated API response sufficient.

Verification failures are release failures unless explicitly skipped.

---

## Dry-run semantics

Dry-run must not:

- edit manifests or repository files;
- generate a lockfile;
- build binaries;
- create commits;
- create/move tags;
- push;
- create releases;
- upload assets;
- delete/replace assets.

Dry-run should still:

- acquire/inspect source;
- resolve workspace/package selection;
- validate version/tag policy;
- validate metadata;
- print proposed metadata/preparation changes;
- inspect prerequisites;
- show targets;
- show artifact names;
- show release destination;
- show planned Git/API operations.

Dry-run should be useful as a diagnosis tool, not just a no-op flag.

---

## Error design

Create typed errors with useful context.

User-facing errors should be concise and actionable.

Bad:

```text
called `Option::unwrap()` on a `None` value
```

Good:

```text
error: selected package `foo` contains multiple binary targets: foo, foo-admin
hint: multi-binary releases are not supported by this command
```

Do not panic on malformed repository input.

`--verbose` may add diagnostic context but must still redact secrets.

---

## Security constraints

These are non-negotiable:

- never print Gitea tokens;
- never embed credentials in logged URLs;
- never run arbitrary repository-provided shell commands;
- never delete a user's local source tree;
- never commit/tag/push without the corresponding explicit authorization;
- never force-push;
- never move an existing release tag;
- never replace release assets without `--replace-assets`;
- treat cloned repository contents and metadata as untrusted input;
- keep build subprocess environment minimal where secrets are concerned;
- avoid shell interpretation;
- validate archive paths and never create traversal-capable archives.

Any new feature that expands mutation authority must be opt-in.

---

## Testing strategy

Port behavior, not Python implementation details.

### Unit tests

Cover pure policy and parsing:

- repository URL normalization;
- owner/repository extraction;
- expected tag generation;
- tag-prefix handling;
- workspace/package selection;
- binary selection;
- archive naming;
- Windows executable naming;
- binstall metadata generation;
- config precedence;
- manifest validation;
- manifest repair;
- checksum generation;
- stage ordering;
- redaction.

### Filesystem/integration tests

Cover:

- local source acquisition;
- remote clone into temporary directories;
- cleanup/keep-workdir behavior;
- preparation diffs;
- lockfile behavior;
- Cross.toml editing;
- deterministic package layout;
- clean-tree requirements;
- tag creation safeguards.

### Mocked HTTP tests

Cover:

- release lookup/create;
- asset listing;
- upload;
- collision failure;
- explicit replacement;
- API error reporting;
- token redaction;
- no erroneous asset pagination loop.

### Real-tool integration tests

Where environment permits:

- `cargo check`;
- `cargo test`;
- `cross build` for default targets;
- real archive inspection;
- real `cargo binstall` into temporary paths.

Live Gitea publication tests must be opt-in and must never target an arbitrary repository by default.

---

## Parity testing

Before removing Python, create a small compatibility fixture matrix.

At minimum include:

1. standalone one-binary package;
2. workspace with one selected CLI member;
3. inherited workspace version/repository;
4. ambiguous workspace;
5. library-only package;
6. multi-binary package;
7. missing binstall metadata;
8. incorrect binstall metadata;
9. missing lockfile;
10. non-`v` tag prefix;
11. existing release asset collision.

For non-mutating cases, compare Python and Rust:

- exit success/failure;
- selected package;
- resolved version/tag;
- proposed metadata changes;
- target list;
- archive names;
- release destination.

For packaging, compare archive contract rather than byte equality unless reproducibility is explicitly guaranteed by both implementations.

---

## Code quality

Before considering a phase complete:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

If integration tests require containers or network access, keep them explicitly opt-in and document their invocation.

Do not silence Clippy warnings globally to make the gate pass.

Avoid `unwrap()`/`expect()` in normal user-input and I/O paths.

---

## Documentation

Update README as the Rust migration becomes real.

During migration, clearly state which implementation is authoritative/default.

At final cutover, documentation must no longer instruct users to create a Python venv.

Document:

- Rust binary installation/build;
- prerequisites;
- supported targets;
- workspace selection;
- dry-run;
- metadata repair;
- repository preparation;
- Gitea token permissions;
- release workflow;
- cargo-binstall installation;
- current limitations.

Do not claim a migration phase is complete before its parity gate passes.

---

## Things not to "improve" during the rewrite

Unless explicitly requested, do not combine the migration with unrelated product changes.

Avoid:

- changing archive naming;
- changing the default target matrix;
- broadening to multi-package release orchestration;
- adding multi-binary release guessing;
- moving to GitHub-specific APIs;
- replacing `cross` with a home-grown cross compiler workflow;
- changing tag policy;
- changing collision defaults;
- automatically creating/pushing tags during ordinary publication;
- introducing a daemon/service architecture;
- adding a GUI.

First produce a trustworthy Rust replacement.

---

## Definition of done

The Rust migration is complete when all of the following are true:

- the normal CLI no longer requires Python or a venv;
- supported CLI/config behavior is preserved or intentionally documented as changed;
- package/workspace selection matches current behavior;
- metadata validation and repair match the release contract;
- repository preparation preserves explicit mutation boundaries;
- all three default targets can be built in the same supported environments as before;
- generated archives and `SHA256SUMS` satisfy the existing naming/layout contract;
- Gitea release creation/reuse/upload works;
- collision/replacement behavior is preserved;
- anonymous artifact verification works where applicable;
- real isolated `cargo binstall` verification passes;
- token-redaction/security tests pass;
- `cargo fmt`, Clippy, unit tests, and migration integration tests pass;
- at least one real disposable/end-to-end Gitea release has been completed successfully with the Rust implementation;
- the Python worker and Python dependency files can be removed without losing required behavior.

Do not remove the Python implementation until these gates are satisfied.
