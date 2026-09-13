"""Reviewable, opt-in repository preparation; never rewrite Rust source or existing tags."""
from collections.abc import Mapping
from difflib import unified_diff
from pathlib import Path
import re
import tomlkit

from .config import Config
from .manifest import metadata_issues, repair_metadata
from .models import RustProject
from .project import Repository, push_preparation, validate_checkout_tag
from .runner import run, console, redact


def preparation_files(project: RustProject, config: Config, repository: Repository,
                      image_tag: str | None = None) -> dict[Path, str]:
    if image_tag is not None and not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.-]*', image_tag):
        raise ValueError('--cross-image-tag must be a container image tag such as main or 0.2.5.')
    paths = [project.manifest, project.root / '.gitignore', project.root / 'Cross.toml',
             project.root / 'Cargo.lock']
    if any(p.is_symlink() for p in paths):
        raise ValueError('Repository preparation refuses symlinked managed files.')
    issues = metadata_issues(project.manifest, repository.url, config.tag_prefix)
    files = {project.manifest: repair_metadata(project.manifest, issues, write=False)}
    ignore = project.root / '.gitignore'
    contents = ignore.read_text() if ignore.exists() else ''
    missing = [line for line in ('/dist/', '/target/', '!/Cargo.lock') if line not in contents.splitlines()]
    if missing:
        contents = contents.rstrip('\n') + '\n\n# Release publisher\n' + '\n'.join(missing) + '\n'
    files[ignore] = contents
    cross_path = project.root / 'Cross.toml'
    cross = tomlkit.parse(cross_path.read_text()) if cross_path.exists() else tomlkit.document()

    def table(parent, key):
        if key not in parent:
            parent[key] = tomlkit.table()
        if not isinstance(parent[key], Mapping):
            raise ValueError(f'Cross.toml {key!r} must be a table.')
        return parent[key]

    env = table(table(cross, 'build'), 'env')
    passthrough = env.get('passthrough', [])
    if not isinstance(passthrough, list) or any(not isinstance(v, str) for v in passthrough):
        raise ValueError('Cross.toml build.env.passthrough must be a list of strings.')
    if 'RUSTUP_TOOLCHAIN' not in passthrough:
        env['passthrough'] = [*passthrough, 'RUSTUP_TOOLCHAIN']
    if image_tag:
        targets = table(cross, 'target')
        for triple in config.targets:
            table(targets, triple)['image'] = f'ghcr.io/cross-rs/{triple}:{image_tag}'
    files[cross_path] = tomlkit.dumps(cross)
    return files


def prepare_repository(project: RustProject, config: Config, repository: Repository, *,
                       image_tag: str | None = None, dry_run=False, commit=False,
                       create_tag=False, push=False):
    """Prepare known release inputs. Git writes happen only through explicit flags."""
    if push and not (commit and create_tag):
        raise ValueError('--push-preparation requires --commit-preparation and --create-tag.')
    if create_tag and not commit:
        raise ValueError('--create-tag requires --commit-preparation.')
    if not project.tag:
        raise ValueError('Preparation requires a resolved release tag.')
    files = preparation_files(project, config, repository, image_tag)
    changed = {path: text for path, text in files.items()
               if not path.exists() or path.read_text() != text}
    lock = project.root / 'Cargo.lock'
    generate_lock = not lock.exists()
    branch = None
    if commit:
        if run('git', 'status', '--porcelain', '--untracked-files=normal', cwd=project.root):
            raise ValueError('Start --commit-preparation from a clean checkout, including the index. '
                             'Review and commit earlier local edits separately.')
        if create_tag:
            exists = run('git', 'tag', '--list', project.tag, cwd=project.root)
            untracked_managed = any(
                path.exists() and not run('git', 'ls-files', '--', str(path), cwd=project.root)
                for path in [*files, lock]
            )
            if exists and (changed or generate_lock or untracked_managed):
                raise ValueError(f'Tag {project.tag!r} already exists; preparation will not move it. '
                                 'Use a new package version or prepare without --create-tag.')
            if exists:
                validate_checkout_tag(project.root, project.tag)
        if push:
            branch = run('git', 'symbolic-ref', '--quiet', '--short', 'HEAD', cwd=project.root)
            if not branch:
                raise ValueError('Pushing preparation requires a checked-out branch.')
    for path, text in changed.items():
        old = path.read_text() if path.exists() else ''
        label = str(path.relative_to(project.root))
        console.print(redact(''.join(unified_diff(old.splitlines(True), text.splitlines(True),
                                               fromfile=label, tofile=label))))
    if generate_lock:
        console.print('Cargo.lock: generate with cargo generate-lockfile (may download registry metadata).')
    if commit:
        console.print('Git: commit only the prepared files using the configured author and signing settings.')
    if create_tag:
        console.print(f'Git: create or validate annotated tag {project.tag}. Existing tags are never moved.')
    if push:
        console.print(f'Git: atomically push branch {branch} and tag {project.tag} to origin; no force push.')
    if dry_run:
        console.print('Preparation dry run complete; no files, commits, tags, or remote refs changed.')
        return
    for path, text in changed.items():
        path.write_text(text, encoding='utf-8')
    if generate_lock:
        run('cargo', 'generate-lockfile', cwd=project.root, capture=False)
    if commit:
        # Only these files can enter the commit. The initially clean index prevents accidental inclusion.
        paths = [str(path) for path in files] + [str(lock)]
        run('git', 'add', '--force', '--', *paths, cwd=project.root)
        if run('git', 'diff', '--cached', '--name-only', cwd=project.root):
            run('git', 'commit', '-m', f'Prepare {project.package_name} {project.version} binary release',
                cwd=project.root, capture=False)
        project.commit = run('git', 'rev-parse', 'HEAD', cwd=project.root)
    if create_tag:
        if not run('git', 'tag', '--list', project.tag, cwd=project.root):
            run('git', 'tag', '-a', project.tag, '-m', f'{project.package_name} {project.version}', cwd=project.root)
        validate_checkout_tag(project.root, project.tag)
    if push:
        push_preparation(project.root, branch, project.tag, repository, config.token_env)
    console.print('Repository preparation complete. Review the changes, then run the publisher without --prepare-repo.')
