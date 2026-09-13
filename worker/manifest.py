from __future__ import annotations

import json

from pathlib import Path

import tomlkit

from .models import RustProject
from .runner import run


def inspect_project(root: Path) -> RustProject:
    manifest_path = root / "Cargo.toml"

    if not manifest_path.exists():
        raise RuntimeError(
            f"Cargo.toml not found: {manifest_path}"
        )

    raw = manifest_path.read_text(
        encoding="utf-8"
    )

    document = tomlkit.parse(raw)

    package = document.get("package")

    if package is None:
        raise RuntimeError(
            "workspace-only Cargo.toml is not supported yet"
        )

    name = package.get("name")
    version = package.get("version")

    if not name:
        raise RuntimeError(
            "package.name is missing"
        )

    if not version:
        raise RuntimeError(
            "package.version is missing"
        )

    repository = package.get("repository")

    metadata_raw = run(
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--no-deps",
        cwd=root,
    )

    metadata = json.loads(metadata_raw)

    packages = metadata.get("packages", [])

    matching = [
        pkg
        for pkg in packages
        if pkg["name"] == name
    ]

    if len(matching) != 1:
        raise RuntimeError(
            f"could not uniquely identify package {name!r}"
        )

    cargo_package = matching[0]

    binaries = [
        target["name"]
        for target in cargo_package["targets"]
        if "bin" in target["kind"]
    ]

    if not binaries:
        raise RuntimeError(
            "package contains no binary targets"
        )

    commit = None

    try:
        commit = run(
            "git",
            "rev-parse",
            "HEAD",
            cwd=root,
        )
    except Exception:
        pass

    return RustProject(
        root=root,
        manifest=manifest_path,
        package_name=name,
        version=str(version),
        repository=str(repository)
        if repository
        else None,
        binaries=binaries,
        commit=commit,
    )
