# Advanced usage

## Sources and package selection

`SOURCE` can be a local directory, an HTTP(S) Git URL, an SSH URL, or an SCP-style URL such as `git@host:owner/repo.git`. Remote sources are cloned into a managed checkout. Use `--keep-workdir` to retain that checkout and its artifacts; preparation retains remote checkouts automatically. Local sources are never deleted.

Use `--package NAME` to select one workspace member by its exact Cargo package name. Selection uses `cargo metadata` and accepts inherited workspace version and repository values. Library-only packages and members with multiple binaries cannot be released. Checks, tests, and builds are scoped to the selected package.

`--repository-url https://host/owner/repo` selects the release destination when Cargo metadata or Git origin needs an explicit web URL. Materially conflicting origin and release destinations are refused. Repository URLs with embedded credentials are refused.

## Configuration

The bundled `config.toml` supplies defaults. Pass `--config PATH` to use another file; CLI values override its build and tag settings.

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

Use `--tag-prefix ""` for an unprefixed tag such as `0.3.1`. `--tag TAG` must match the selected package version and configured prefix. `--unlocked` permits builds without the usual locked dependency resolution; the default uses `--locked`.

## Metadata and preparation

The publisher checks the selected member's repository and cargo-binstall metadata. `--fix-metadata` repairs supported entries in that member's `Cargo.toml` while preserving comments where practical. It does not commit, tag, or push. To preview the full preparation diff, use `--prepare-repo --dry-run`.

The required binstall contract is:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz"
pkg-fmt = "tgz"
bin-dir = "{ name }-{ target }-v{ version }/{ bin }{ binary-ext }"
```

The generated `pkg-url` uses the configured tag prefix; the archive filename itself retains `-v{version}`. Unsupported binstall keys are reported as metadata conflicts. A preparation run may create one extra fix commit, as intended. `--commit-preparation` requires a clean worktree and index. `--create-tag` requires that commit option, and `--push-preparation` requires both. No option moves an existing tag or force pushes.

For repositories whose build scripts need a selected Rust toolchain inside Cross, preparation can add the supported `Cross.toml` environment setting. `--cross-image-tag TAG` explicitly selects `ghcr.io/cross-rs/TARGET:TAG` images. Existing unrelated Cross settings are preserved. Repository-provided Cross hooks such as `pre-build`, `dockerfile`, and `runner` are rejected by the release build.

## Pipeline stages

`--stop-after` can stop at `inspect`, `metadata`, `verify`, `build`, `package`, `release`, `upload`, or `binstall`. `verify` runs host `cargo check` and `cargo test`; `--skip-tests` skips only tests. Release builds use fresh target directories to avoid stale build-script output. `release` creates or reuses the Gitea release; `upload` uploads archives and checksums. The final stage downloads assets anonymously, checks their checksums and archive layout, and runs isolated `cargo-binstall` installs for each target. `--skip-binstall-test` skips that install check, while retaining download verification.

`--draft --stop-after upload` is useful when a release must stay private for review. Full verification needs public downloads. `--prerelease` and `--draft` set properties when a release is created; reusing a release preserves its existing settings. Asset replacement is not transactional: if an upload fails after a matching asset is deleted, retry the publication explicitly.

## Artifact contract

Each archive is named `{name}-{target}-v{version}.tgz` and contains exactly one root directory with one binary:

```text
my-tool-x86_64-unknown-linux-musl-v0.1.0.tgz
└── my-tool-x86_64-unknown-linux-musl-v0.1.0/
    └── my-tool
```

Windows archives contain `my-tool.exe`. `SHA256SUMS` lists the final archive checksums. Archive ownership and timestamps are normalized; identical binary bytes and modes produce identical archives. Separate builds can still produce different binaries.

## Authentication and security

Authenticated Gitea API publication requires HTTPS. The token comes from the configured environment variable, never from a positional argument or URL. Child Cargo and Cross processes do not receive it. An HTTPS preparation push to the matching release repository can use the token through a temporary askpass helper; SSH pushes use normal Git credentials. Grant the token account code read and release write access, plus code write when using `--push-preparation`.

Normal publication does not create a commit, tag, or push. Existing asset names cause an error unless `--replace-assets` is set; unrelated assets are not deleted. Draft or private assets cannot pass anonymous verification.

## Troubleshooting

- **Missing tag:** prepare and push the expected tag explicitly, or use `--prepare-repo --commit-preparation --create-tag --push-preparation` from a clean checkout.
- **Missing Cargo.lock:** run preparation or use `--unlocked` when unlocked resolution is intentional.
- **Container unavailable:** start Docker or Podman. `CROSS_CONTAINER_ENGINE` can select `docker` or `podman`.
- **Cross image or native library failure:** configure an appropriate Cross image and target libraries in the project. The publisher does not substitute an MSVC target or hide linker failures.
- **Private release verification failure:** publish the release before the final verification stage, or stop at `upload` for a draft workflow.
- **Asset collision:** inspect the existing release; use `--replace-assets` only when replacement is intended.

`cargo-binstall` verification installs into temporary directories and does not run foreign-target binaries. Plain registry based `cargo binstall my-tool` additionally requires a published crate with the release metadata; this tool does not publish crates.
