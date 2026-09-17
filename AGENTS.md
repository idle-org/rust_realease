# AGENTS.md

## Mission

Maintain `rust_realease` as a self-contained Rust CLI for local binary archives and Gitea releases. The Python migration is complete as an implementation cutover; historical migration instructions and parity notes remain in Git history. Preserve the documented CLI, release format, and safety behavior.

## Product behavior

- Accept a local Rust project or Git URL. Select exactly one package and one binary; workspaces require exact `--package NAME` selection when ambiguous.
- Resolve packages with `cargo metadata`. Edit only the selected member manifest and preserve its formatting with `toml_edit`.
- Keep version/tag consistency, the default `v` prefix, default three-target matrix, archive names and layout, and binstall metadata templates.
- `--stop-after package` produces local `.tgz` files and `SHA256SUMS`. Ordinary publication verifies the local and remote tag, builds in a fresh target directory, creates or reuses a Gitea release, uploads assets, and verifies published downloads and isolated cargo-binstall installs.
- `--prepare-repo` may repair metadata, `.gitignore`, `Cross.toml`, and a missing lockfile. It exits before building. Commit, tag, and push require their separate explicit flags. It may create an additional fix commit by design.
- Dry runs only inspect and report; they never edit files, build, commit, tag, push, or publish.
- Jenkins runs Cargo tests only. Cross and live Gitea tests are opt-in local checks using the disposable `target_test_project` submodule.

## Safety

- Never print, log, commit, or place Gitea tokens in URLs. Do not forward secrets into build subprocesses. Use `GITEA_TOKEN` or the configured token environment variable for API calls.
- Never move an existing tag, force push, replace colliding assets without `--replace-assets`, or delete a user's local source tree.
- Keep Git and Cargo command execution argv-based; do not interpret repository content as shell instructions. Treat remote repositories, manifests, and archive paths as untrusted input.
- Require a clean checkout for publication and a clean checkout/index before an authorized preparation commit. Verify the push destination matches the release repository; push branch and tag atomically.
- Public assets require anonymous post-upload download checks. Private assets require authenticated download checks and `--skip-binstall-test`, since cargo-binstall cannot fetch the private attachment URLs.

## Quality and workflow

- Track current work in `AGENT_TASKS.md`. Historical Python comparison tasks are preserved in Git history, not required to run from the current Rust-only tree.
- Make coherent signed commits using the configured Git identity and signing settings. Respect the user's instruction to leave the parent branch for them to push.
- Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all` for Rust changes. Keep real Cross and Gitea tests opt-in; never point them at arbitrary repositories by default.
- Document user-visible behavior in `README.md` and advanced details in `docs/advanced-usage.md`. Prefer concrete errors and tests for behavior and safety changes.
