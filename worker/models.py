from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path


class Stage(str, Enum):
    inspect = 'inspect'
    metadata = 'metadata'
    verify = 'verify'
    build = 'build'
    package = 'package'
    release = 'release'
    upload = 'upload'
    binstall = 'binstall'


@dataclass
class RustProject:
    root: Path
    manifest: Path
    package_name: str
    version: str
    repository: str | None = None
    binaries: list[str] = field(default_factory=list)
    commit: str | None = None
    tag: str | None = None
    target_directory: Path | None = None


@dataclass(frozen=True)
class BuildTarget:
    triple: str

    @property
    def is_windows(self):
        return 'windows' in self.triple.split('-')

    def binary_filename(self, binary: str) -> str:
        return binary + ('.exe' if self.is_windows else '')


@dataclass
class MetadataIssue:
    key: str
    actual: object
    expected: object


@dataclass
class Artifact:
    target: str
    path: Path
    root: str
    binary: str
    sha256: str
