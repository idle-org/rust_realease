import os
import shutil
import tempfile
from collections.abc import Mapping
import tomlkit
from pathlib import Path
from .config import Config
from .models import RustProject, BuildTarget
from .runner import run, CommandError, console



def validate_cross_config(root: Path):
    """Do not execute shell hooks supplied through repository cross metadata."""
    def inspect(value):
        if isinstance(value, Mapping):
            for key, child in value.items():
                if key in ('pre-build', 'dockerfile', 'runner'):
                    raise ValueError(f'Repository cross configuration contains unsupported executable option {key!r}.')
                inspect(child)
    manifest = tomlkit.parse((root / 'Cargo.toml').read_text())
    workspace = manifest.get('workspace', {})
    if isinstance(workspace, Mapping):
        metadata = workspace.get('metadata', {})
        if isinstance(metadata, Mapping):
            inspect(metadata.get('cross', {}))
    path = root / 'Cross.toml'
    if path.exists():
        inspect(tomlkit.parse(path.read_text()))


def prerequisites(project: RustProject, config: Config, binstall: bool = True) -> tuple[list[str], str | None]:
    problems = []
    try:
        validate_cross_config(project.root)
    except ValueError as exc:
        problems.append(str(exc))
    for tool in ['git', 'cargo', 'cross'] + (['cargo-binstall'] if binstall else []):
        found = shutil.which(tool)
        console.print(f'{tool}: {found or "missing"}')
        if not found:
            problems.append(f'{tool} is missing')
    requested = os.environ.get('CROSS_CONTAINER_ENGINE')
    engine = requested or next((t for t in ('docker', 'podman') if shutil.which(t)), None)
    if engine not in ('docker', 'podman'):
        problems.append('Set CROSS_CONTAINER_ENGINE to docker or podman with a usable backend')
        engine = None
    else:
        try:
            run(engine, 'info', timeout=20)
            console.print(f'{engine}: usable')
        except CommandError as exc:
            problems.append(str(exc))
    if config.locked and not (project.root / 'Cargo.lock').is_file():
        problems.append('Cargo.lock is missing; generate it explicitly or use --unlocked')
    return problems, engine


def verify_source(project: RustProject, config: Config, skip_tests: bool):
    flags = ['--package', project.package_name]
    if config.locked:
        flags.append('--locked')
    run('cargo', 'check', *flags, cwd=project.root, capture=False)
    if not skip_tests:
        run('cargo', 'test', *flags, cwd=project.root, capture=False)


def build(project: RustProject, config: Config, engine: str) -> dict[str, Path]:
    flags = ['--release'] if config.profile == 'release' else ['--profile', config.profile]
    if config.locked:
        flags += ['--locked']
    # Explicit directory makes Cargo config / CARGO_TARGET_DIR overrides unambiguous.
    target_base = (project.target_directory or project.root / 'target') / 'binstall-publisher'
    target_base.mkdir(parents=True, exist_ok=True)
    target_dir = Path(tempfile.mkdtemp(prefix='build-', dir=target_base))
    console.print(f'Fresh build directory: {target_dir}')
    environment = {'CROSS_CONTAINER_ENGINE': engine}
    if shutil.which('rustup'):
        selected = run('rustup', 'show', 'active-toolchain', cwd=project.root).split()
        if selected:
            environment['RUSTUP_TOOLCHAIN'] = selected[0]
    profile_dir = 'debug' if config.profile == 'dev' else config.profile
    outputs = {}
    for target in config.targets:
        console.print(f'Building {target}')
        run('cross', 'build', *flags, '--target', target, '--target-dir', str(target_dir),
            '--package', project.package_name, '--bin', project.binaries[0], cwd=project.root, capture=False,
            env=environment)
        binary = target_dir / target / profile_dir / BuildTarget(target).binary_filename(project.binaries[0])
        if not binary.is_file() or binary.is_symlink():
            raise ValueError(f'Build did not produce a regular binary: {binary}')
        outputs[target] = binary
    return outputs
