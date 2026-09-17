"""Opt-in Python/Rust smoke parity check for the tiny local fixture.

Run with the Python environment used by the old worker:
    .venv/bin/python tests/parity_smoke.py
"""

import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "target_test_project"
RUST = ROOT / "target" / "debug" / "rust_realease"


def invoke(command):
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT)
    env["NO_COLOR"] = "1"
    return subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True)


def field(output, name):
    match = re.search(rf"(?m)^{re.escape(name)}: ([^\n]+)", output)
    if match is None:
        raise AssertionError(f"missing {name} field:\n{output}")
    return match.group(1).strip()


def main():
    if not FIXTURE.is_dir():
        raise SystemExit("Initialize target_test_project submodule first.")
    subprocess.run(["cargo", "build", "--locked", "--quiet"], cwd=ROOT, check=True)
    commands = {
        "Python": [sys.executable, "-P", "-m", "worker"],
        "Rust": [str(RUST)],
    }
    for stage in ("inspect", "metadata"):
        results = {
            name: invoke(base + [str(FIXTURE), "--dry-run", "--stop-after", stage])
            for name, base in commands.items()
        }
        for name, result in results.items():
            if result.returncode != 0:
                raise AssertionError(f"{name} {stage} failed:\n{result.stdout}\n{result.stderr}")
        for key in ("Package", "Version", "Binary", "Tag"):
            values = {name: field(result.stdout, key) for name, result in results.items()}
            if len(set(values.values())) != 1:
                raise AssertionError(f"{stage} {key} differs: {values}")
        if stage == "metadata":
            for name, result in results.items():
                if field(result.stdout, "Binstall metadata") != "valid":
                    raise AssertionError(f"{name} binstall metadata differs")
    tag = field(results["Rust"].stdout, "Tag")
    present = subprocess.run(
        ["git", "tag", "--list", tag], cwd=FIXTURE, text=True, capture_output=True, check=True
    ).stdout.strip()
    if not present:
        for name, base in commands.items():
            result = invoke(base + [str(FIXTURE), "--dry-run", "--stop-after", "verify"])
            if result.returncode == 0 or "tag" not in result.stdout.lower():
                raise AssertionError(f"{name} dry run missed the local tag:\n{result.stdout}")
    with tempfile.TemporaryDirectory(prefix="rust-release-parity-") as folder:
        root = Path(folder)
        (root / "Cargo.toml").write_text(
            "[workspace]\nmembers=['one','two']\nresolver='2'\n", encoding="utf-8"
        )
        for member in ("one", "two"):
            package = root / member
            (package / "src").mkdir(parents=True)
            (package / "Cargo.toml").write_text(
                f"[package]\nname='{member}'\nversion='0.1.0'\nedition='2021'\n",
                encoding="utf-8",
            )
            (package / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        for label, extra, expected in (
            ("ambiguous workspace", [], False),
            ("selected workspace member", ["--package", "two"], True),
            ("unknown workspace member", ["--package", "missing"], False),
        ):
            results = {
                name: invoke(base + [str(root), "--stop-after", "inspect"] + extra)
                for name, base in commands.items()
            }
            for name, result in results.items():
                if (result.returncode == 0) != expected:
                    raise AssertionError(
                        f"{name} {label} success differs:\n{result.stdout}\n{result.stderr}"
                    )
                if expected and field(result.stdout, "Package") != "two":
                    raise AssertionError(f"{name} selected the wrong workspace package")
        for kind in ("library-only", "multiple binaries"):
            source = root / kind
            (source / "src").mkdir(parents=True)
            (source / "Cargo.toml").write_text(
                "[package]\nname='shape'\nversion='0.1.0'\nedition='2021'\n",
                encoding="utf-8",
            )
            if kind == "library-only":
                (source / "src" / "lib.rs").write_text("pub fn value() {}\n", encoding="utf-8")
            else:
                (source / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
                bins = source / "src" / "bin"
                bins.mkdir()
                (bins / "other.rs").write_text("fn main() {}\n", encoding="utf-8")
            for name, base in commands.items():
                result = invoke(base + [str(source), "--stop-after", "inspect"])
                if result.returncode == 0:
                    raise AssertionError(f"{name} accepted {kind} unexpectedly")
    print("Python/Rust fixture and Cargo package-shape smoke parity passed.")


if __name__ == "__main__":
    main()
