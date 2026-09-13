from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path


@dataclass(slots=True)
class RustProject:
    root: Path
    manifest: Path

    package_name: str
    version: str

    repository: str | None = None
    binaries: list[str] = field(default_factory=list)

    commit: str | None = None
    tag: str | None = None


@dataclass(slots=True)
class BuildTarget:
    triple: str

    @property
    def is_windows(self) -> bool:
        return "windows" in self.triple

    def binary_filename(self, binary: str) -> str:
        if self.is_windows:
            return f"{binary}.exe"

        return binary
