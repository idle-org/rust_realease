# Portable application bundles (protocol 1)

Named bundles add application resources and runtime executables to the release
workflow. They are **additional artifacts**: `package.format`,
`--artifact-format`, canonical tgz filenames, and cargo-binstall metadata retain
their existing meanings. No release was published to test this feature.

## Agent workflow

1. Keep `[package] format = "tgz"` for the existing cargo-binstall distribution.
2. Add `[[package.bundles]]` for each additional distribution. Declare every
   static inclusion; select Cargo binaries; configure preparation if necessary.
3. Run `rust_realease . --package PACKAGE --dry-run`. This validates static
   inputs, configuration, names, and selected targets, and prints commands and
   required outputs. Dynamic results remain unresolved. No preparation, dependency
   downloads, cache directories, or archives are created. Existing metadata/tag/
   prerequisite diagnostics still apply.
4. Run `rust_realease . --package PACKAGE --compile-only`. This needs no Gitea
   token and creates no tag, push, release, or upload. `--stop-after package`
   does the same packaging work. `--skip-tests` only skips source tests;
   preparation and bundle validation still run.
5. Read the stable local `dist/latest-artifacts.json`. Its `manifest` and
   `checksums` paths are relative to `dist/` and identify the latest successful
   bundle invocation; a failed invocation leaves the previous locator intact.
   Open that `artifact-manifest.json` (beside `SHA256SUMS`). It contains schema
   version 1, relative asset filenames,
   asset digests, and bundle file destinations, digests, modes, and check statuses.
   Resolve filenames relative to this directory; do not guess build paths.
6. Extract and perform native application acceptance tests. Then use the existing
   explicit publication workflow. All artifacts are assembled and validated
   before release creation. Upload collision handling, `--replace-assets`, and
   download/checksum verification include bundles and the artifact manifest.
   Unrelated release assets remain untouched. Uploads are not transactional.

`--target TRIPLE` limits builds and bundle preparation to that target. A bundle's
`targets` filters further; omitted/empty means all selected build targets. Every
selected target must have an entry in `formats`. Supported formats are `zip` and
`tgz` (gzip-compressed tar). Extensions are appended automatically.

## Configuration contract

```toml
[package]
format = "tgz"

[[package.bundles]]
name = "portable"
targets = ["x86_64-pc-windows-gnu", "x86_64-unknown-linux-gnu"]
formats = { x86_64-pc-windows-gnu = "zip", x86_64-unknown-linux-gnu = "tgz" }
archive-name = "{name}-{bundle}-{target}-v{version}"
root = "{name}-{bundle}-{target}-v{version}"
cargo-binaries = "all" # Or ["app", "app-gui"], using declared Cargo target names.
files = [
  { source = "assets", destination = "resources" },
  { source = "LICENSE", destination = "LICENSES/app.txt", workspace-relative = true },
  { source = "runtime/helper.dll", destination = "helper.dll" },
]

[package.bundles.prepare]
command = ["python3", "prepare-release.py", "--context", "{context-file}", "--manifest", "{manifest-file}"]
inputs = ["prepare-release.py", "dependency-pins.json", "workspace:scripts/downloader.py"]
timeout-seconds = 600
required-destinations = ["helper{binary-ext}", "LICENSES/helper/LICENSE.txt"]
```

Static sources are relative to the selected Cargo package's directory, even when
configuration is at the workspace root. Set `workspace-relative = true` for
workspace files. Directories are recursively copied as explicitly declared file
trees; empty directories are not included. DLLs/shared libraries/resources use
`kind = "file"` (the default); runnable programs use `kind = "executable"`.
Executable trees apply that kind to every file, so declare mixed trees separately.

`inputs` lists regular preparation/smoke-test input files and validates their
existence. Strings are package-relative; `workspace:PATH` explicitly selects the
workspace. The hook may read these inputs but is responsible for interpreting
pins. Hooks and bundles are never cached, so each invocation resolves dependency
identity again. A verified Rust build cache may still be reused. Changing a
license, adapter, pin, or downloaded dependency cannot cause a stale bundle hit.
A new source commit can still invalidate the existing Rust build cache.

Template substitutions in `archive-name`, `root`, static destinations and required
destinations are `{name}` (Cargo package), `{version}`, `{bundle}`, `{target}` and
`{binary-ext}` (`.exe` on Windows, otherwise empty). Archive name and root must
be single components; neither is a source path. Names must not collide with
canonical artifacts or `SHA256SUMS`/`artifact-manifest.json`. Unknown config
fields, formats, binary selections and template placeholders are errors. Layered
configuration replaces the entire bundle list when a later layer specifies one.

## Host preparation contract

Preparation runs once per selected bundle/target after compilation or build-cache
reuse, before archive creation. The working directory is the selected package
root. `command` is an argv array, with no shell interpretation. Explicit shell
commands can use `["sh", "-c", "..."]`. The only command substitutions are
`{context-file}`, `{manifest-file}`, and `{bundle-directory}`; paths with spaces
and Unicode stay single arguments.

The context JSON has these exact keys:

| Key | Meaning |
| --- | --- |
| `schema_version` | Integer `1` |
| `package_name`, `package_version` | Selected Cargo package identity |
| `bundle`, `target`, `profile`, `binary_ext` | Effective bundle/build settings |
| `workspace_root`, `package_root` | Absolute source roots |
| `source_commit`, `source_dirty` | Git HEAD and pre-build checkout dirty status |
| `binaries` | Object mapping **all declared binary names** to absolute built executable paths |
| `output_directory` | Fresh, initially empty directory for prepared files |
| `cache_directory` | Persistent dependency cache, separated by bundle and target |
| `manifest_path` | Absolute path where the hook must write its result JSON |
| `bundle_directory` | Fresh tree populated after preparation, for optional smoke tests |

Bundle builds read executable paths from Cargo compiler-artifact JSON and remap
Cross container `/target` paths to the exact host build directory. Cached
builds use validated publisher cache paths. The hook must use `binaries`, never
construct `target/TRIPLE/release/NAME`; this supports workspace members, custom
profiles, `CARGO_TARGET_DIR` and fresh dirty-checkout builds.

The hook writes outputs only beneath `output_directory`, writes its result to
`manifest_path`, and may maintain the supplied persistent cache. It must not
construct archives, checksums, tags or releases. The host command is trusted
project code, **not an OS filesystem/network sandbox**. Gitea's configured token
variable and environment variables containing TOKEN, PASSWORD, SECRET or
CREDENTIAL are removed from its environment. Output is streamed with target/
bundle labels and existing secret redaction. Do not put credentials in command
arguments or public provenance files.

Example result (replace hashes with actual lowercase SHA-256 hex digests):

```json
{
  "schema_version": 1,
  "target": "x86_64-pc-windows-gnu",
  "files": [
    {"source": "helper.exe", "destination": "helper.exe", "kind": "executable", "sha256": "<64 hex characters>"},
    {"source": "notices/LICENSE.txt", "destination": "LICENSES/helper/LICENSE.txt", "kind": "file", "sha256": "<64 hex characters>"}
  ]
}
```

Every manifest source is a regular file relative to `output_directory`; enumerate
files in directories individually. Every destination is relative to the archive
root. Only declared files enter the archive. Required destinations must exist
exactly, including case. The publisher verifies hashes and target/schema identity.
Cargo executables cannot be replaced by static files or hook outputs.

Paths cannot be absolute, contain `..`, empty components, backslashes or colons.
Symlinks, nonregular files, and hardlinked files are rejected, including directory
traversal through symlinks. Windows destinations additionally reject reserved
names, invalid characters and case-insensitive collisions. File/directory-prefix
collisions also fail. Files are ordered deterministically; ownership and timestamps
are normalized; modes are 0755 for executables and 0644 for data. Identical input
bytes and settings produce identical archives. Each archive is written atomically
and reopened to check its declared contents, modes and hashes. Partial staging is
removed on failure; successful invocation directories are retained under `dist/`.

Executable validation currently supports little-endian Linux ELF (x86-64,
aarch64, i686, riscv64gc) and Windows PE (x86-64, aarch64, i686). Unsupported
executable targets fail explicitly. This checks format and architecture, not ABI,
DLL completeness, license suitability or runtime behavior. Foreign executables
are never executed automatically by the publisher. Trusted preparation scripts
must follow the same rule themselves.

Nonzero exit, timeout, SIGINT/SIGTERM cancellation on Unix, malformed JSON,
missing outputs, hash mismatch or architecture mismatch abort before publication.
Unix hooks run in their own process group; descendants are killed on completion,
failure, timeout or cancellation. Native Windows host process-tree cancellation
has not been validated; use a Unix release host for this workflow.

## Optional native smoke test

```toml
[package.bundles.smoke-test]
command = ["python3", "smoke-release.py", "{bundle-directory}"]
inputs = ["smoke-release.py"]
timeout-seconds = 60
```

The publisher runs this command only when the target exactly matches `rustc -vV`'s
host triple. The script should invoke programs by absolute path in the supplied
tree, set any necessary environment explicitly, and return nonzero on failure.
Checks are reported as `passed` or `skipped` with a reason in the artifact manifest;
a failed command is reported with its status and stops packaging, so no successful
artifact manifest is emitted for that invocation. A successful smoke test cannot
silently modify declared payload bytes. Preparation may separately record native
checks in its own provenance. Cross-compiling Windows is not Windows validation.

## Migration recipe: pptx-reporter

This recipe uses the existing `scripts/fetch_ffprobe.py` API:
`fetch(target, cache=None, release="latest", expected_sha256=None)` returns
`(binary_path, notices_directory)`. It retains that downloader's target selection,
verified archive digests, LGPL/non-shared provider selection and provenance. Do not
copy the host's `ffprobe` or change the publisher into a provider-specific downloader.

In the consumer workspace's `rust-release.toml`, keep the existing build targets,
release and repository settings. Replace the separate packager comments with:

```toml
[package]
format = "tgz"

[[package.bundles]]
name = "portable"
targets = ["x86_64-pc-windows-gnu", "x86_64-unknown-linux-gnu"]
formats = { x86_64-pc-windows-gnu = "zip", x86_64-unknown-linux-gnu = "tgz" }
archive-name = "{name}-portable-{target}-v{version}"
root = "{name}-portable-{target}-v{version}"
cargo-binaries = "all"
files = [
  { source = "LICENSE", destination = "LICENSES/reporter-MIT.txt", workspace-relative = true },
  { source = "assets/fonts/LICENSE", destination = "LICENSES/DejaVu.txt" },
  { source = "docs/windows-quickstart.txt", destination = "README.txt", workspace-relative = true },
]

[package.bundles.prepare]
command = ["python3", "prepare-release.py", "--context", "{context-file}", "--manifest", "{manifest-file}"]
inputs = ["prepare-release.py", "ffprobe-pins.json", "workspace:scripts/fetch_ffprobe.py"]
timeout-seconds = 600
required-destinations = ["ffprobe{binary-ext}", "LICENSES/ffmpeg/LICENSE.txt", "LICENSES/ffmpeg/provenance.json"]
```

Create `crates/pptx-reporter/ffprobe-pins.json`, using actual immutable provider
release tags and reviewed SHA-256 archive digests for **each target**:

```json
{
  "x86_64-pc-windows-gnu": {"release": "<immutable release tag>", "sha256": "<Windows archive SHA-256>"},
  "x86_64-unknown-linux-gnu": {"release": "<immutable release tag>", "sha256": "<Linux archive SHA-256>"}
}
```

Create `crates/pptx-reporter/prepare-release.py`:

```python
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys

parser = argparse.ArgumentParser()
parser.add_argument("--context", type=Path, required=True)
parser.add_argument("--manifest", type=Path, required=True)
args = parser.parse_args()
context = json.loads(args.context.read_text())
assert context["schema_version"] == 1
sys.path.insert(0, str(Path(context["workspace_root"]) / "scripts"))
from fetch_ffprobe import fetch

pins = json.loads((Path(context["package_root"]) / "ffprobe-pins.json").read_text())
pin = pins[context["target"]]
binary, notices = fetch(
    context["target"], Path(context["cache_directory"]),
    pin["release"], pin["sha256"],
)
out = Path(context["output_directory"])
files = []

def include(source, relative, destination, kind="file"):
    if source.is_symlink() or not source.is_file():
        raise ValueError(f"Not a regular prepared input: {source}")
    copied = out / relative
    copied.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, copied)  # Copy bytes, not cache links.
    files.append({
        "source": relative, "destination": destination, "kind": kind,
        "sha256": hashlib.sha256(copied.read_bytes()).hexdigest(),
    })

name = "ffprobe" + context["binary_ext"]
include(binary, name, name, "executable")
for path in sorted(notices.rglob("*")):
    if path.is_symlink():
        raise ValueError(f"Symlinked notice: {path}")
    if path.is_file():
        relative = path.relative_to(notices).as_posix()
        include(path, "notices/" + relative, "LICENSES/ffmpeg/" + relative)
# The downloader preserves upstream archive directories, whose names vary.
# Retain them all and add a stable alias for the required license destination.
license_destination = "LICENSES/ffmpeg/LICENSE.txt"
if not any(f["destination"] == license_destination for f in files):
    licenses = sorted(p for p in notices.rglob("*") if p.is_file()
                      and p.name.lower().startswith(("license", "copying")))
    if not licenses:
        raise ValueError("Missing upstream license")
    include(licenses[0], "canonical-LICENSE.txt", license_destination)
args.manifest.write_text(json.dumps({
    "schema_version": 1, "target": context["target"], "files": files,
}) + "\n")
```

The adapter preserves upstream notices and `provenance.json`, including provider
URL, release/asset IDs, archive SHA-256, extracted hashes and the downloader's
native version check. A stable `LICENSE.txt` alias is added while retaining the
original upstream notice hierarchy. It never discovers Cargo output paths or creates archives.
Python is a build-host dependency only. Ignore the target/cache/dist directories
in Git; do not track downloaded executables or generated manifests.

Run:

```sh
rust_realease . --package pptx-reporter --dry-run
rust_realease . --package pptx-reporter --compile-only
# To prepare/build just Linux:
rust_realease . --package pptx-reporter --target x86_64-unknown-linux-gnu --compile-only
```

Verify checksums and both full portable trees. On Linux, extract the tgz and run
report generation using the adjacent FFprobe without relying on PATH. On a Windows
machine, extract the ZIP, launch the GUI and generate a report without Python,
Typst, PowerPoint or separately installed FFmpeg. These consumer application tests
and Windows native validation are **not performed by the fixture test suite**.
Remove `scripts/package-portable.py` only after completing that consumer migration.
Keep Jenkins as checks/coverage/badges only; publishing remains explicit.

The canonical cargo-binstall archives still install the declared Rust binaries.
They do **not** install the portable bundle's resources or FFprobe. Document the
external FFprobe requirement for cargo-binstall users. Portable-tree validation,
cargo-binstall installation checks and native-runtime checks are separate results.
