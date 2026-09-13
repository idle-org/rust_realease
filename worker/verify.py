from pathlib import Path
import tarfile
import tempfile
import httpx
from .models import Artifact, BuildTarget, RustProject
from .package import checksum
from .policy import asset_url
from .runner import run, console


def verify_archive(path: Path, artifact: Artifact):
    with tarfile.open(path, 'r:gz') as archive:
        members = archive.getmembers()
        if len(members) != 2:
            raise ValueError(f'{path.name}: expected exactly a root directory and binary.')
        root, binary = members
        if (root.name != artifact.root or not root.isdir() or
            binary.name != artifact.root + '/' + artifact.binary or not binary.isfile()):
            raise ValueError(f'{path.name}: archive layout does not match binstall metadata.')
        if 'windows' not in artifact.target and not binary.mode & 0o111:
            raise ValueError(f'{path.name}: binary is not executable.')


def verify_published(repository: str, tag: str, artifacts: list[Artifact], sums: Path, transport=None):
    # Anonymous downloads prove that the metadata works for ordinary consumers.
    with tempfile.TemporaryDirectory(prefix='binstall-download-') as folder, httpx.Client(
            follow_redirects=True, timeout=120, transport=transport) as client:
        for local in [a.path for a in artifacts] + [sums]:
            downloaded = Path(folder) / local.name
            try:
                with client.stream('GET', asset_url(repository, tag, local.name)) as response:
                    if response.status_code != 200:
                        raise ValueError(f'{local.name}: public download returned HTTP {response.status_code}.')
                    count = 0
                    with downloaded.open('wb') as stream:
                        for chunk in response.iter_bytes():
                            count += len(chunk)
                            if count > local.stat().st_size:
                                raise ValueError(f'{local.name}: download exceeds expected size.')
                            stream.write(chunk)
            except httpx.HTTPError:
                raise ValueError(f'{local.name}: public download failed.') from None
            if checksum(downloaded) != checksum(local):
                raise ValueError(f'{local.name}: published SHA-256 differs from local artifact.')
            for artifact in artifacts:
                if artifact.path == local:
                    verify_archive(downloaded, artifact)
                    console.print(f'Verified URL, checksum and archive: {artifact.target}')
        console.print('Verified published SHA256SUMS')


def verify_binstall(project: RustProject, targets: list[str]):
    # Installing a foreign-target binary is safe: nothing is executed.
    for target in targets:
        with tempfile.TemporaryDirectory(prefix='binstall-install-') as folder:
            run('cargo', 'binstall', project.package_name, '--manifest-path', str(project.manifest),
                '--version', '=' + project.version, '--targets', target,
                '--strategies', 'crate-meta-data', '--no-confirm', '--no-track',
                '--disable-telemetry', '--no-discover-github-token',
                '--install-path', folder, cwd=project.root, capture=False)
            installed = Path(folder) / BuildTarget(target).binary_filename(project.binaries[0])
            if not installed.is_file():
                raise ValueError(f'cargo-binstall did not install the expected binary for {target}.')
            console.print(f'cargo-binstall installation verified: {target}')
