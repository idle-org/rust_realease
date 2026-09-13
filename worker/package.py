import gzip
import hashlib
from pathlib import Path
import tarfile
import tempfile
from .models import Artifact, BuildTarget, RustProject
from .policy import archive_name, archive_root


def checksum(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def package(project: RustProject, outputs: dict[str, Path]) -> tuple[list[Artifact], Path]:
    dist = project.root / 'dist'
    if dist.is_symlink():
        raise ValueError('Refusing a symlinked dist directory.')
    dist.mkdir(exist_ok=True)
    # A fresh managed directory avoids deleting or overwriting unrelated dist contents.
    staging = Path(tempfile.mkdtemp(prefix='release-', dir=dist))
    artifacts = []
    for target, binary_path in outputs.items():
        root = archive_root(project.package_name, target, project.version)
        binary = BuildTarget(target).binary_filename(project.binaries[0])
        path = staging / archive_name(project.package_name, target, project.version)
        with path.open('wb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as gz:
            with tarfile.open(fileobj=gz, mode='w', format=tarfile.USTAR_FORMAT) as archive:
                directory = tarfile.TarInfo(root)
                directory.type, directory.mode = tarfile.DIRTYPE, 0o755
                archive.addfile(directory)
                info = tarfile.TarInfo(root + '/' + binary)
                info.size = binary_path.stat().st_size
                info.mode = 0o644 if BuildTarget(target).is_windows else (binary_path.stat().st_mode & 0o777) | 0o111
                with binary_path.open('rb') as content:
                    archive.addfile(info, content)
        artifacts.append(Artifact(target, path, root, binary, checksum(path)))
    sums = staging / 'SHA256SUMS'
    sums.write_text(''.join(f'{a.sha256}  {a.path.name}\n' for a in sorted(artifacts, key=lambda a: a.path.name)))
    return artifacts, sums
