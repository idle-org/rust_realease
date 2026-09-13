import json
import re
from collections.abc import Mapping
from pathlib import Path
import tomlkit
from .models import RustProject, MetadataIssue
from .policy import binstall_metadata
from .runner import run, CommandError


def read_manifest(path: Path):
    document = tomlkit.parse(path.read_text(encoding='utf-8'))
    if not isinstance(document.get('package'), Mapping):
        raise ValueError('Cargo.toml contains no [package] table. Select a package manifest or use --package NAME at the workspace root.')
    return document


def inspect_project(root: Path, package_name: str | None = None) -> RustProject:
    source_manifest = root / 'Cargo.toml'
    document = tomlkit.parse(source_manifest.read_text(encoding='utf-8'))
    metadata = json.loads(run('cargo', 'metadata', '--format-version=1', '--no-deps', cwd=root))
    member_ids = set(metadata.get('workspace_members', []))
    packages = [p for p in metadata.get('packages', []) if p['id'] in member_ids]
    discovered = ', '.join(sorted(p['name'] for p in packages)) or 'none'
    if package_name is None:
        if not isinstance(document.get('package'), Mapping) or len(packages) != 1:
            raise ValueError('Workspace selection requires --package NAME; discovered: ' + discovered)
        pkg = packages[0]
    else:
        matches = [p for p in packages if p['name'] == package_name]
        if len(matches) != 1:
            raise ValueError(f'Workspace package {package_name!r} was not found uniquely; discovered: {discovered}')
        pkg = matches[0]
    binaries = [t['name'] for t in pkg['targets'] if 'bin' in t['kind']]
    if len(binaries) != 1:
        raise ValueError(f'Package {pkg["name"]!r} must have exactly one binary; discovered: ' +
                         (', '.join(binaries) or 'none'))
    for value in (pkg['name'], binaries[0], pkg['version']):
        if not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.+-]*', value):
            raise ValueError('Unsafe package, version or binary filename.')
    workspace_root = Path(metadata['workspace_root']).resolve()
    manifest = Path(pkg['manifest_path'])
    if not manifest.resolve().is_relative_to(workspace_root):
        raise ValueError('Selected package manifest is outside the workspace root.')
    read_manifest(manifest)
    try:
        commit = run('git', 'rev-parse', 'HEAD', cwd=root)
    except CommandError:
        commit = None
    return RustProject(workspace_root, manifest, pkg['name'], pkg['version'], pkg.get('repository'),
                       binaries, commit, target_directory=Path(metadata['target_directory']))


def metadata_issues(path: Path, repository: str, prefix='v') -> list[MetadataIssue]:
    package = read_manifest(path)['package']
    issues = []
    if package.get('repository') != repository:
        issues.append(MetadataIssue('package.repository', package.get('repository'), repository))
    metadata = package.get('metadata', {})
    if not isinstance(metadata, Mapping):
        raise ValueError('package.metadata must be a table.')
    binstall = metadata.get('binstall', {})
    if not isinstance(binstall, Mapping):
        raise ValueError('package.metadata.binstall must be a table.')
    for key, value in binstall_metadata(prefix).items():
        if binstall.get(key) != value:
            issues.append(MetadataIssue('package.metadata.binstall.' + key, binstall.get(key), value))
    # Overrides and other custom options can change URL resolution or prevent installation.
    for key in binstall:
        if key not in binstall_metadata(prefix):
            issues.append(MetadataIssue('package.metadata.binstall.' + key, binstall[key], None))
    return issues


def repair_metadata(path: Path, issues: list[MetadataIssue], *, write: bool = True) -> str:
    if path.is_symlink():
        raise ValueError('Refusing to repair a symlinked Cargo.toml.')
    document = read_manifest(path)
    for issue in issues:
        keys = issue.key.split('.')
        # Custom override names can contain dots; remove the top-level custom option.
        if issue.expected is None:
            document['package']['metadata']['binstall'].pop(
                issue.key.removeprefix('package.metadata.binstall.'), None
            )
            continue
        table = document
        for key in keys[:-1]:
            if key not in table:
                table[key] = tomlkit.table()
            table = table[key]
        table[keys[-1]] = issue.expected
    rendered = tomlkit.dumps(document)
    if write:
        path.write_text(rendered, encoding='utf-8')
    return rendered
