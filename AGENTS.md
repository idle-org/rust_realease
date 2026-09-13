# AGENT.md

## Mission

Finish the `rust-binstall-publisher` proof-of-concept as a small, maintainable release tool for Rust CLI projects.

The tool must:

1. refuse to run from the Bash wrapper unless a Python virtual environment is active;
2. offer to create `.venv` when no project venv exists;
3. inspect a local Rust project or clone a Git repository;
4. validate package metadata and `cargo-binstall` metadata;
5. optionally repair missing or incorrect metadata without silently pushing changes;
6. validate that the Git tag matches the Cargo package version;
7. cross-compile release binaries for the default target matrix;
8. package artifacts using a deterministic naming/layout convention;
9. generate SHA-256 checksums;
10. create or reuse a Gitea release;
11. upload release assets;
12. verify that `cargo-binstall` can consume the published artifact metadata.

The intended default targets are:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-pc-windows-gnu`

The tool is initially optimized for a single Cargo package producing a single CLI binary. Multi-package workspaces and multi-binary packages should be detected and reported cleanly rather than guessed at.

---

## Repository layout

Target layout:

```text
rust-binstall-publisher/
├── release.sh
├── requirements.txt
├── config.toml
├── README.md
├── AGENT.md
├── AGENT_TASKS.md
└── worker/
    ├── __init__.py
    ├── __main__.py
    ├── cli.py
    ├── config.py
    ├── models.py
    ├── runner.py
    ├── project.py
    ├── manifest.py
    ├── build.py
    ├── package.py
    ├── gitea.py
    └── verify.py
```

Tests may be added under:

```text
tests/
```

Do not collapse everything into one large Python file.

---

## User experience

Preferred final CLI:

```bash
./release.sh SOURCE [OPTIONS]
```

Examples:

```bash
./release.sh ./my-tool --dry-run
```

```bash
./release.sh \
  https://gitea.example.org/user/my-tool.git \
  --tag v0.3.1 \
  --dry-run
```

```bash
./release.sh \
  https://gitea.example.org/user/my-tool.git \
  --tag v0.3.1 \
  --fix-metadata
```

Avoid requiring a redundant subcommand such as:

```bash
./release.sh release ...
```

unless Typer makes the simpler interface unreasonable.

---

## Bash wrapper requirements

`release.sh` must be intentionally strict.

### Without an active venv

If `.venv` exists:

- refuse to run the worker;
- print the exact activation command;
- exit non-zero.

Expected guidance:

```bash
source .venv/bin/activate
```

If `.venv` does not exist and stdin is interactive:

- ask whether to create it;
- if accepted:
  - run `python3 -m venv .venv`;
  - install `requirements.txt`;
  - report success;
  - exit;
- do not attempt to “activate” the environment from inside the script.

If stdin is non-interactive:

- fail with a clear message instead of prompting.

### With an active venv

If it is the project `.venv`, proceed.

If another venv is active:

- warn;
- interactively ask whether to continue;
- in non-interactive mode, fail rather than guessing.

The wrapper must use:

```bash
set -euo pipefail
```

and finish with:

```bash
exec python -m worker "$@"
```

or the equivalent project-local interpreter invocation.

---

## Python dependencies

Dependencies are acceptable.

Use:

- `tomlkit` for preserving `Cargo.toml` comments and formatting;
- `rich` for readable terminal output;
- `typer` for CLI parsing;
- `httpx` for Gitea HTTP operations.

Prefer the standard library for everything else.

Do not add large frameworks.

---

## Configuration

`config.toml` should provide sane defaults.

Suggested initial shape:

```toml
[release]
tag-prefix = "v"

[build]
targets = [
  "x86_64-unknown-linux-musl",
  "aarch64-unknown-linux-musl",
  "x86_64-pc-windows-gnu",
]
profile = "release"
locked = true

[package]
format = "tgz"

[gitea]
token-env = "GITEA_TOKEN"
```

CLI options override configuration values.

Do not hardcode one specific Gitea instance.

---

## Source acquisition

Support:

1. a local path;
2. a Git HTTP/HTTPS URL;
3. SSH-style URLs such as `git@host:owner/repo.git`.

For remote repositories:

- clone into a temporary directory;
- support checking out a requested tag;
- remove the temporary directory unless `--keep-workdir` is set.

For local repositories:

- never delete or move the user’s project;
- do not commit or push changes automatically.

---

## Cargo inspection

Use:

```bash
cargo metadata --format-version=1 --no-deps
```

to discover:

- package name;
- package version;
- repository;
- binary target names.

Also read `Cargo.toml` through `tomlkit` for editable metadata.

For the first supported release path:

- exactly one package;
- exactly one binary.

If multiple binaries or ambiguous workspace packages exist:

- show what was discovered;
- exit with a clear unsupported-case message.

Do not guess which binary the user intended.

---

## Version/tag policy

Default tag prefix:

```text
v
```

For package version:

```text
0.3.1
```

expected tag:

```text
v0.3.1
```

If `--tag` is supplied and does not match, fail before building.

Do not silently retag or rewrite package versions.

---

## cargo-binstall metadata policy

The tool should standardize artifact naming.

Preferred archive name:

```text
{name}-{target}-v{version}.tgz
```

Examples:

```text
mytool-x86_64-unknown-linux-musl-v0.3.1.tgz
mytool-aarch64-unknown-linux-musl-v0.3.1.tgz
mytool-x86_64-pc-windows-gnu-v0.3.1.tgz
```

Preferred archive layout:

```text
mytool-x86_64-unknown-linux-musl-v0.3.1/
└── mytool
```

Windows:

```text
mytool-x86_64-pc-windows-gnu-v0.3.1/
└── mytool.exe
```

Expected metadata should be generated from the chosen convention rather than duplicated in several modules.

Suggested metadata:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz"
pkg-fmt = "tgz"
bin-dir = "{ name }-{ target }-v{ version }/{ bin }{ binary-ext }"
```

If actual cargo-binstall behavior requires a small adjustment, centralize it in one policy function and document the reason.

### Modes

Default:

```text
check only
```

With:

```bash
--fix-metadata
```

the tool may edit the checked-out `Cargo.toml`.

It must:

- preserve formatting/comments where practical;
- print what changed;
- not commit;
- not push.

A future `--commit-metadata` option may be added, but it is not required for this PoC.

---

## Prerequisite validation

Before a real build, check for:

- `git`
- `cargo`
- `cross`
- a usable Docker/Podman backend required by `cross`

Do not discover missing build prerequisites halfway through target 3.

A dry run should report prerequisite availability but not build anything.

---

## Build policy

Use `cross build`.

Default pattern:

```bash
cross build \
  --release \
  --locked \
  --target TARGET
```

Respect config values for:

- profile;
- locked/unlocked behavior;
- target list.

If `Cargo.lock` does not exist and locked mode is enabled, fail clearly or require an explicit override.

Do not switch to `x86_64-pc-windows-msvc` automatically.

The Windows default for this Linux-hosted PoC is:

```text
x86_64-pc-windows-gnu
```

---

## Packaging policy

Create a local `dist/` staging directory inside the working copy or an explicitly managed temporary location.

For each target:

1. locate the compiled binary;
2. copy it into the standardized top-level archive directory;
3. preserve executable mode on Unix binaries;
4. create `.tgz`;
5. compute SHA-256.

Generate:

```text
SHA256SUMS
```

with one line per archive.

Do not include build directories or unrelated project files in the archives.

---

## Gitea authentication

Read the token from an environment variable.

Default:

```text
GITEA_TOKEN
```

Never require the token as a normal CLI argument.

Do not log or print the token.

Support overriding the token environment-variable name through config if easy.

---

## Gitea repository/release handling

The tool should derive owner/repository from either:

- the project repository URL;
- the clone source URL;
- explicit CLI options if necessary.

Do not guess silently if the repository cannot be identified.

Release behavior:

1. look up the release by tag;
2. if none exists, create it;
3. if it exists, reuse it;
4. upload artifacts.

Default release:

```text
name: same as tag
draft: false
prerelease: false
```

Support:

```bash
--draft
--prerelease
```

if straightforward.

---

## Asset collision policy

Default:

- if a release already contains an asset with the same filename, fail.

With:

```bash
--replace-assets
```

the tool may delete/replace matching assets.

Never silently create duplicate assets with altered filenames.

---

## Verification

After publishing, verify the release contract.

At minimum:

- construct the expected asset URL;
- verify the asset exists;
- download it or otherwise validate that it is accessible;
- verify archive structure;
- verify binary filename.

Preferred final verification:

```bash
cargo binstall ...
```

using the checked-out manifest or a safe temporary install root.

Do not overwrite an unrelated user binary during verification.

Use a temporary Cargo home or bin directory where practical.

The tool should report verification results per target where feasible.

---

## CLI flags

Implement or reserve cleanly for:

```text
--tag TAG
--target TARGET          repeatable
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
```

Useful stage names:

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

`--stop-after` should stop successfully after the requested completed stage.

---

## Dry-run semantics

`--dry-run` must not:

- edit the manifest;
- build binaries;
- create releases;
- upload assets;
- delete assets.

It should:

- acquire/inspect the project;
- report package information;
- validate version/tag;
- validate metadata;
- report proposed metadata changes;
- report planned build targets;
- report prerequisites;
- print intended artifact filenames and release destination.

Dry-run should be useful enough to diagnose most configuration problems.

---

## Error handling

Prefer clear, short messages.

Bad:

```text
KeyError: 'package'
```

Good:

```text
error: Cargo.toml contains no [package] table.
Workspace-only manifests are not supported by this proof-of-concept.
```

External command failures should show:

- command;
- exit status;
- useful stderr.

Do not expose Python tracebacks by default.

`--verbose` may expose more detail.

---

## Code organization

Keep responsibilities separated:

### `runner.py`
External command execution and streaming.

### `project.py`
Clone/local-path handling, checkout, cleanup, Git metadata.

### `manifest.py`
Cargo/TOML inspection and binstall metadata validation/repair.

### `build.py`
Prerequisites and cross compilation.

### `package.py`
Artifact staging, archives, hashes.

### `gitea.py`
HTTP client, release lookup/create, asset lookup/upload/delete.

### `verify.py`
Published artifact and cargo-binstall verification.

### `models.py`
Dataclasses shared across modules.

### `config.py`
Config file parsing and default/CLI merge.

### `cli.py`
Argument parsing and orchestration only.

Do not put HTTP logic in `cli.py`.

Do not put TOML-editing logic in `build.py`.

---

## Testing expectations

At minimum, add tests for pure logic where practical:

- repository URL parsing;
- expected tag from version;
- archive naming;
- binary filename per target;
- cargo-binstall metadata generation;
- manifest validation/repair;
- checksum generation.

Avoid requiring a live Gitea instance for the basic test suite.

Gitea API calls should be structured so they can be mocked.

---

## Security constraints

- never print `GITEA_TOKEN`;
- never embed tokens in URLs;
- never push Git changes automatically;
- never delete a user’s local source tree;
- never replace remote assets unless explicitly requested;
- never run arbitrary commands from repository metadata;
- treat repository content as untrusted input;
- avoid `shell=True` in Python subprocess calls.

---

## Documentation expectations

Update `README.md` as features become real.

Document:

- setup;
- venv behavior;
- prerequisites;
- supported targets;
- dry-run;
- metadata repair;
- Gitea token setup;
- example publish command;
- current limitations.

Do not document unsupported features as if they already work.

---

## Definition of done

The PoC is complete when this workflow works against a real single-binary Rust CLI project hosted on Gitea:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt

export GITEA_TOKEN="..."

./release.sh \
  https://gitea.example.org/owner/tool.git \
  --tag v0.1.0 \
  --fix-metadata
```

and the result is:

- package metadata validated;
- binaries built for all default targets;
- three `.tgz` artifacts produced;
- `SHA256SUMS` produced;
- a Gitea release created or reused;
- all assets uploaded;
- the cargo-binstall URL/layout validated;
- no unrelated files or remote Git history modified.


