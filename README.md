# Rust Binstall Publisher

> Rust migration status: the Rust binary supports local and remote Git inspection, metadata repair, repository preparation, host check/test (`--stop-after verify`), Cross builds (`--stop-after build`), and local `.tgz`/SHA256SUMS packaging (`--stop-after package`). Preparation has disposable Git tests for dry-run, commit, and tag safety; its remote push path still needs an isolated integration test. The tiny fixture built and packaged all three default targets locally. The Python worker remains the publication implementation.

Inspect locally with `cargo run -- target_test_project --stop-after inspect`. Validate metadata with `cargo run -- target_test_project --stop-after metadata`; add `--dry-run` to preview proposed repairs or `--fix-metadata` to write them. Preview repository changes with `cargo run -- target_test_project --prepare-repo --dry-run`. Use `--commit-preparation`, `--create-tag`, and `--push-preparation` only when those Git writes are intended. Run host checks with `cargo run -- target_test_project --stop-after verify`. Build and package locally with `--stop-after package` once Cross and a container backend are available. Run the Rust checks with `cargo fmt --check`, `cargo clippy --offline --all-targets --all-features -- -D warnings`, and `cargo test --offline`. Live Gitea tests are deferred until a disposable token is available.

A small Python release tool for Rust CLI projects on Gitea, releasing one package with one binary at a time. It inspects Cargo metadata, checks version tags, cross-compiles binaries, creates reproducible `.tgz` archives and SHA-256 checksums, uploads a release, and verifies the downloads with cargo-binstall.

## Setup

Python 3.11 or newer is required.

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
./release.sh --help
```

The wrapper refuses to start without an active venv. If `.venv` is missing and stdin is interactive, it offers to create it and install dependencies, then exits. Activate the environment yourself before rerunning. An existing inactive `.venv` prints activation guidance. Another active venv requires interactive confirmation; non-interactive invocations fail. Paths passed to the wrapper are relative to your current directory, even when invoking the wrapper from elsewhere.

Install Git, Rust/Cargo, `cross`, Docker or Podman, and `cargo-binstall`. A functioning container backend is required. `CROSS_CONTAINER_ENGINE=podman` or `docker` selects the backend; otherwise Docker is preferred when installed. The tool reports all prerequisite failures before cross-compilation. Cargo.lock is required by default; generate it explicitly or pass `--unlocked`. Builds can download Cargo dependencies and cross container images.

## Inspect and repair

```bash
./release.sh ./my-tool --dry-run
./release.sh https://gitea.example.org/owner/tool.git --dry-run
./release.sh ./my-tool --fix-metadata --stop-after metadata
```

Dry-run inspects the project, reports proposed metadata edits, prerequisites, the tag, target matrix, artifact names and destination. It does not edit Cargo.toml, compile code or mutate releases. It exits nonzero if metadata or prerequisite issues are found, even with `--fix-metadata`. Source acquisition still clones remote repositories; Cargo metadata and read-only prerequisite commands run.

Metadata repair preserves TOML comments and formatting where practical, reports each changed key, and only writes Cargo.toml. Missing `package.repository` is derived from Git origin; `--repository-url https://host/gitea/owner/tool` explicitly selects the destination. HTTP(S), SSH and SCP-style Git URLs are supported. SSH repository URLs are normalized to HTTPS web URLs; use the explicit option if SSH and web hosts or paths differ. HTTP credentials and query strings in repository URLs are rejected.

The publisher standardizes these keys:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz"
pkg-fmt = "tgz"
bin-dir = "{ name }-{ target }-v{ version }/{ bin }{ binary-ext }"
```

Custom binstall options, including per-target overrides and signing configuration, are reported as conflicts and removed only with `--fix-metadata`. Review the reported changes. The template convention and local-manifest verification follow the [cargo-binstall support documentation](https://github.com/cargo-bins/cargo-binstall/blob/main/SUPPORT.md).

## Build and publish

Default targets:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-pc-windows-gnu`

Build without contacting Gitea:

```bash
./release.sh ./my-tool --fix-metadata --stop-after package
```

Before publishing, create and push the tag explicitly. For version `0.1.0`, the default tag is `v0.1.0`. Publication requires a clean checkout before the tool's own metadata repair, a local tag pointing to HEAD, and a matching remote tag. Normal publication does not create tags, commit or push; use the explicit preparation options below to automate those steps. A supplied `--tag` checks out that exact tag in a temporary clone; local sources must already be at that tag.

```bash
export GITEA_TOKEN="..."
./release.sh https://gitea.example.org/owner/tool.git \
  --tag v0.1.0 --fix-metadata
```

## Prepare a repository for releases

The preparation mode automates the repeatable repository fixes needed before publishing. Start with a preview:

```bash
./release.sh ./my-tool --package my-tool \
  --prepare-repo --cross-image-tag main --dry-run
```

`--prepare-repo` repairs the selected package's repository/binstall metadata, adds build/artifact ignores and a Cargo.lock inclusion rule, generates Cargo.lock if missing, and configures Cross.toml to forward the selected Rust toolchain into containers. Existing lockfiles are preserved. TOML comments and unrelated cross settings are preserved where practical. `--cross-image-tag main` explicitly sets the configured targets' images to `ghcr.io/cross-rs/TARGET:main`; omit it to preserve existing image choices and cross defaults. This environment setup supports build scripts such as epoch_forge's without rewriting their Rust source.

Apply locally for manual review:

```bash
./release.sh ./my-tool --package my-tool --prepare-repo --cross-image-tag main
```

Or, from a clean checkout, explicitly automate the Git steps too:

```bash
./release.sh ./my-tool --package my-tool --prepare-repo --cross-image-tag main \
  --commit-preparation --create-tag --push-preparation

./release.sh ./my-tool --package my-tool --tag v0.1.0
```

Use the actual package/version in these examples. Preparation exits without building or publishing. The second command performs the release checks, fresh builds, uploads, and verification.

- `--commit-preparation` stages only the prepared manifest, Cross.toml, .gitignore and Cargo.lock, then commits using your configured Git identity and signing settings. It requires a clean worktree/index; if you already applied local preparation, review and commit those edits manually first.
- `--create-tag` requires `--commit-preparation`; it creates an annotated version tag or validates an existing tag. It never moves an existing tag. If repairs would change an already-tagged version, use a new package version or prepare without tag creation and manage the existing tag explicitly yourself.
- `--push-preparation` requires both flags above and a checked-out branch. It atomically pushes that branch and the tag to origin, without force. The push URL must match the release repository. Authentication/permission, branch protection and signing failures are reported; local prepared changes/commits/tags remain available for recovery. No remote push occurs without this flag.

All preparation flags support `--dry-run`, which prints the proposed file diffs and Git operations without generating a lockfile, writing files, committing, tagging or pushing. A preparation preview reports planned fixes successfully; ordinary publication dry-run still exits nonzero for unresolved release requirements. `--prepare-repo` is separate from `--stop-after`. In preparation mode, `--tag` names the intended release tag and validates it against the package version; it does not check out an existing tag.

Remote sources work too and their temporary checkouts are always retained in preparation mode. No token is needed for local preparation. These helpers do not rewrite arbitrary source code, change package versions, fix native dependencies such as SQLite, or install build tools. Compilation requirements still need to be satisfied before publication.

### Select a workspace package

Pass the exact Cargo package name with `--package` when the source is a workspace:

```bash
./release.sh https://proxy.dandaedre.com/gitea/Ravenous/ravenous-tools.git \
  --package ravenous-tools --dry-run
```

Virtual workspace roots and workspaces with multiple members require explicit selection. Omitting `--package` lists the discovered members and stops; an unknown package, a library-only package, or a selected package with multiple binaries also fails clearly. A standalone package still works without this option.

Cargo supplies the selected member's resolved version and repository, including inherited `[workspace.package]` values. `--fix-metadata` edits only that member's Cargo.toml; inherited versions and sibling manifests are preserved. Checks, tests and cross builds use `--package NAME` from the workspace root, sharing its Cargo.lock and profiles. Release builds use fresh subdirectories beneath its Cargo target directory. Dependencies of the selected package are built normally. Archives go into the workspace root's `dist/release-*/`, and cargo-binstall verification uses the selected member's manifest.

The version/tag policy still applies to the selected package. For repositories using unprefixed tags such as `0.3.32`, pass `--tag-prefix "" --tag 0.3.32` (or set `[release].tag-prefix` in your configuration). Select an existing tag whose package version matches; the tool does not retag the current checkout.

The `ravenous-tools` `0.3.32` integration attempt passed selection and host check/test, but the default musl cross-build failed at linking with `cannot find -lsqlite3`. Its system SQLite dependency needs a target-compatible build setup. Workspace selection does not provision native libraries inside cross images; this failure occurs before release creation or upload.

### Gitea token permissions

Create a **personal access token** in **User Settings → Applications → Manage Access Tokens**. Under **Select permissions**, set **Repository → Read and Write** (`write:repository`) and leave the other permission categories disabled. See Gitea's [token creation instructions](https://docs.gitea.com/development/api-usage/#generating-and-listing-api-tokens).

`write:repository` includes repository read, write and delete API operations; a separate `read:repository` grant is unnecessary. This is the scope covering releases and their attachments, not `write:package` (the package registry). No user, organization, admin, issue, or package scope is needed, including when publishing to an organization-owned repository. Gitea's [scope definitions](https://docs.gitea.com/development/oauth2-provider/#scopes) describe this mapping.

The token's account must also have access to the destination repository: **Code: Read** to inspect tags and **Releases: Write** to create releases and manage attachments. `--push-preparation` additionally requires **Code: Write** for the branch and tag push. For an organization repository such as `idle-org/john_slop`, ensure its team permissions grant that access. Token scopes do not grant repository membership or replace account permissions; see [Gitea repository permissions](https://docs.gitea.com/1.22/usage/permissions/).

The publisher uses the permission for these operations:

| Operation | API request under `/repos/{owner}/{repo}` |
| --- | --- |
| Check the remote tag | `GET /tags/{tag}` |
| Find or create a release | `GET /releases/tags/{tag}`, `POST /releases` |
| List or upload attachments | `GET` / `POST /releases/{id}/assets` |
| Replace a matching attachment, only with `--replace-assets` | `DELETE /releases/{id}/assets/{asset_id}` |

The scope permits more repository operations than the publisher uses; Gitea's documented scopes do not provide a release-only write scope.

Load the token into the shell running the publisher. In Bash, this prompts without echoing the token or including its value in shell history:

```bash
read -r -s -p 'Gitea token: ' GITEA_TOKEN
printf '\n'
export GITEA_TOKEN
```

Authenticated publication requires HTTPS. The token is read from `GITEA_TOKEN` (or the variable named by `[gitea].token-env`), redacted from worker diagnostics, and removed from child build processes. Cloning uses your normal Git authentication. With `--push-preparation`, HTTPS pushes to the matching release repository can use the same token through a temporary Git askpass helper; the token is never embedded in URLs, arguments, or helper files. SSH pushes use your normal SSH credentials. No token is needed for dry-run or `--stop-after package`. Full post-upload verification still requires publicly accessible release assets; granting more token permissions does not enable private/draft download verification.

The pipeline runs `cargo check`, `cargo test`, and then `cross build --release --locked --target TARGET` for each target. `--skip-tests` skips tests but retains the check. `--profile` overrides the profile and repeatable `--target` overrides the matrix. Config defaults come from this tool's `config.toml`; use `--config PATH` for another file. Only `tgz` packaging is supported.

Artifacts are stored in a fresh `dist/release-*/` directory so existing dist files are never deleted. For example:

```text
dist/release-abc123/
  tool-x86_64-unknown-linux-musl-v0.1.0.tgz
  tool-aarch64-unknown-linux-musl-v0.1.0.tgz
  tool-x86_64-pc-windows-gnu-v0.1.0.tgz
  SHA256SUMS
```

Each archive contains a matching top-level directory and its binary (with `.exe` on Windows). Archive timestamps and ownership are normalized. Identical binary bytes and modes produce identical archives. This does not guarantee that separate Rust builds produce identical binaries.

Gitea releases are looked up by tag and reused or created. All existing asset names are checked before any upload; collisions fail unless `--replace-assets` explicitly permits deleting matching assets. Replacement is not transactional: a failed upload after deletion requires a retry. Completed uploads remain if a later step fails.

After upload, anonymous downloads must match local checksums and archive structure, including SHA256SUMS. Then cargo-binstall installs each target into a temporary directory using the checked-out manifest, with compile and quick-install fallbacks disabled. Installed binaries are never executed, and the user's Cargo bin directory is not changed. `--skip-binstall-test` skips only this installation check, not download verification.

## Stages and limitations

`--stop-after` succeeds after the requested stage: `inspect`, `metadata`, `verify` (Cargo check/tests), `build`, `package`, `release`, `upload`, or `binstall` (published-download and installation verification). Early stages do not require a token. `--stop-after package` never contacts the Gitea API and can be used before creating tags.

`--draft --stop-after upload` uploads a draft; draft and private assets cannot satisfy anonymous verification. `--prerelease` sets the flag on newly created releases. Reusing a release preserves its existing settings. Full verification currently requires publicly accessible assets.

Remote clones and their artifacts are removed on exit unless `--keep-workdir` is supplied. Preparation mode always retains remote clones and prints their paths so local edits and commits are not lost. Local sources are never removed. Use `--keep-workdir` with remote builds when you want to retain artifacts or metadata repairs. `--verbose` adds commands and tracebacks.

Workspaces require explicit package selection as described above; each invocation releases exactly one member with one binary. Multiple binary selection and publishing an entire workspace in one invocation are unsupported. No crate publishing, artifact signing, MSVC switching, macOS target setup, or automatic installation of build tools is provided. Build only source you trust: Cargo checks, tests and builds can execute project build scripts and procedural macros.

A successful local-manifest install proves the uploaded artifact contract. Plain `cargo binstall tool` additionally requires a registry-published crate containing the corrected metadata; this tool does not publish crates. For an unregistered project, use `cargo binstall --git https://host/owner/tool.git tool` after explicitly committing and pushing the metadata.

## Tests

```bash
source .venv/bin/activate
python -m unittest discover -s tests -v
```

The basic suite uses temporary fixtures and mocked Gitea requests; it needs no token or live Gitea instance. See `AGENT_TASKS.md` for implementation and integration status.

Repository cross configuration containing `pre-build`, `dockerfile` or `runner` hooks is rejected rather than executing shell commands from metadata. Custom cross images can still be selected through cross's normal configuration/environment interface.

### Tested host compatibility

On the development host, cross 0.2.5's default images failed with GLIBC version errors against Rust nightly 1.100. The same project built for all three targets using newer images, following [cross's image configuration](https://github.com/cross-rs/cross/blob/main/docs/config_file.md):

```bash
export CROSS_TARGET_X86_64_UNKNOWN_LINUX_MUSL_IMAGE=ghcr.io/cross-rs/x86_64-unknown-linux-musl:main
export CROSS_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_IMAGE=ghcr.io/cross-rs/aarch64-unknown-linux-musl:main
export CROSS_TARGET_X86_64_PC_WINDOWS_GNU_IMAGE=ghcr.io/cross-rs/x86_64-pc-windows-gnu:main
```

These are explicit compatibility overrides, not publisher defaults. Pin image digests in your own build environment when reproducibility across image updates matters.

To exercise actual cargo-binstall downloads and installations against a temporary local HTTP server:

```bash
PUBLISHER_INTEGRATION=1 python -m unittest discover -s tests -v
```

This opt-in test installs fixture files for all three targets into disposable directories and never executes them.

For `epoch_forge`'s build scripts, which query `rustup` unless a toolchain is supplied, the successful test additionally used:

```bash
export RUSTUP_TOOLCHAIN=nightly
export CROSS_CONTAINER_OPTS='--env RUSTUP_TOOLCHAIN=nightly'
```

This passes the selected toolchain name to the containers without requiring `rustup` inside them. Use the toolchain appropriate to your project. Cross documents container options and environment forwarding in its [environment-variable reference](https://github.com/cross-rs/cross/blob/main/docs/environment_variables.md).

Release builds now always use a new `target/binstall-publisher/build-*/` directory (under Cargo's configured target directory). This prevents reused build-script output from embedding an old Git revision, as happened in the epoch_forge test. The directories are retained with the checkout for inspection; fresh builds take longer and use more disk space than reusing Cargo's release cache. Corrected epoch_forge assets have been replaced and verified on the live v0.1.0 release.
