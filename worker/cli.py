"""CLI argument handling and ordered release orchestration."""
import os
from pathlib import Path
import traceback
import typer
from . import runner
from .build import build, prerequisites, verify_source
from .config import DEFAULT_CONFIG, load_config
from .gitea import Gitea
from .manifest import inspect_project, metadata_issues, repair_metadata
from .models import Stage
from .prepare import prepare_repository
from .package import package
from .policy import archive_name, expected_tag
from .project import acquire_project, origin_url, parse_repository, validate_checkout_tag
from .verify import verify_binstall, verify_published

app = typer.Typer(no_args_is_help=True, pretty_exceptions_enable=False)
console = runner.console


@app.command()
def release(
    source: str = typer.Argument(..., help='Local directory or HTTP(S)/SSH Git URL.'),
    package_name: str | None = typer.Option(None, '--package', help='Exact workspace package name; required for workspaces.'),
    tag: str | None = typer.Option(None, '--tag', '-t'),
    tag_prefix: str | None = typer.Option(None, '--tag-prefix', help='Override version tag prefix; use an empty string for unprefixed tags.'),
    target: list[str] | None = typer.Option(None, '--target', help='Rust target; repeatable.'),
    dry_run: bool = typer.Option(False, '--dry-run'),
    fix_metadata: bool = typer.Option(False, '--fix-metadata'),
    prepare_repo: bool = typer.Option(False, '--prepare-repo', help='Prepare release inputs, then exit without publishing.'),
    commit_preparation: bool = typer.Option(False, '--commit-preparation', help='Commit prepared files; requires a clean checkout.'),
    create_tag: bool = typer.Option(False, '--create-tag', help='Create the version tag after committing preparation; never move tags.'),
    push_preparation: bool = typer.Option(False, '--push-preparation', help='Atomically push prepared branch and tag to origin.'),
    cross_image_tag: str | None = typer.Option(None, '--cross-image-tag', help='Set cross image tags during preparation, e.g. main.'),
    skip_tests: bool = typer.Option(False, '--skip-tests', help='Skip cargo test; cargo check still runs.'),
    skip_binstall_test: bool = typer.Option(False, '--skip-binstall-test'),
    draft: bool = typer.Option(False, '--draft'),
    prerelease: bool = typer.Option(False, '--prerelease'),
    replace_assets: bool = typer.Option(False, '--replace-assets'),
    keep_workdir: bool = typer.Option(False, '--keep-workdir'),
    verbose: bool = typer.Option(False, '--verbose'),
    stop_after: Stage | None = typer.Option(None, '--stop-after'),
    config: Path = typer.Option(DEFAULT_CONFIG, '--config'),
    profile: str | None = typer.Option(None, '--profile'),
    unlocked: bool = typer.Option(False, '--unlocked'),
    repository_url: str | None = typer.Option(None, '--repository-url', help='Explicit Gitea web repository URL.'),
):
    runner.verbose = verbose
    try:
        settings = load_config(config, targets=target or None, profile=profile, locked=False if unlocked else None, tag_prefix=tag_prefix)
        runner.register_secret(settings.token_env)
        if (commit_preparation or create_tag or push_preparation or cross_image_tag is not None) and not prepare_repo:
            raise ValueError('Preparation options require --prepare-repo.')
        if prepare_repo and stop_after is not None:
            raise ValueError('--prepare-repo is a separate mode; omit --stop-after.')
        with acquire_project(source, None if prepare_repo else tag, keep_workdir or prepare_repo) as root:
            project = inspect_project(root, package_name)
            release_tag = expected_tag(project.version, settings.tag_prefix)
            if tag is not None and tag != release_tag:
                raise ValueError(f'Tag {tag!r} does not match package version; expected {release_tag!r}.')
            project.tag = release_tag
            publishing = not prepare_repo and not dry_run and stop_after not in (Stage.inspect, Stage.metadata, Stage.verify, Stage.build, Stage.package)
            if publishing and runner.run('git', 'status', '--porcelain', '--untracked-files=normal', cwd=root):
                raise ValueError('Publication requires a clean checkout before metadata repair.')
            displayed_repository = 'missing'
            if project.repository:
                try:
                    displayed_repository = parse_repository(project.repository).url
                except ValueError:
                    displayed_repository = 'invalid (use --repository-url to repair)'
            for key, value in [('Package', project.package_name), ('Version', project.version),
                               ('Repository', displayed_repository), ('Binary', project.binaries[0]),
                               ('Manifest', str(project.manifest)), ('Commit', project.commit or 'unknown'), ('Tag', release_tag)]:
                console.print(f'{key}: {value}')
            if stop_after == Stage.inspect:
                return
            candidate = repository_url or project.repository or origin_url(root)
            if not candidate:
                raise ValueError('package.repository is missing; provide --repository-url and --fix-metadata.')
            repository = parse_repository(candidate)
            if prepare_repo:
                prepare_repository(project, settings, repository, image_tag=cross_image_tag,
                                   dry_run=dry_run, commit=commit_preparation,
                                   create_tag=create_tag, push=push_preparation)
                return
            issues = metadata_issues(project.manifest, repository.url, settings.tag_prefix)
            for issue in issues:
                console.print(runner.redact(f'{issue.key}: {issue.actual!r} -> {issue.expected!r}'))
            if not issues:
                console.print('Binstall metadata: valid')
            if issues and fix_metadata and not dry_run:
                repair_metadata(project.manifest, issues)
                console.print('Metadata repaired locally. Commit and push explicitly if desired.')
            if issues and not fix_metadata and not dry_run:
                raise ValueError('Metadata requires repair. Rerun with --fix-metadata.')
            if stop_after == Stage.metadata:
                return
            problems, engine = prerequisites(project, settings, not skip_binstall_test and stop_after not in (Stage.verify, Stage.build, Stage.package, Stage.release, Stage.upload))
            tag_exists = validate_checkout_tag(root, release_tag, required=False)
            if not tag_exists:
                problems.append(f'Git tag {release_tag!r} is missing; create it explicitly before publishing')
            console.print(f'Release destination: {repository.url}/releases/tag/{release_tag}')
            for triple in settings.targets:
                console.print(f'{triple}: {archive_name(project.package_name, triple, project.version)}')
            for problem in problems:
                console.print('Prerequisite: ' + problem)
            if dry_run:
                console.print('Dry run complete: no manifest edits, builds or release mutations.')
                if issues or problems:
                    raise ValueError('Dry run found the metadata/prerequisite issues reported above.')
                return
            # Missing tags only prevent publishing; package-only work is useful before tagging.
            build_problems = [p for p in problems if not p.startswith('Git tag ')]
            if build_problems:
                raise ValueError('Resolve the prerequisite issues above before building.')
            if publishing:
                validate_checkout_tag(root, release_tag)
                if not os.environ.get(settings.token_env):
                    raise ValueError(f'Set {settings.token_env} before publishing.')
            verify_source(project, settings, skip_tests)
            if stop_after == Stage.verify:
                return
            outputs = build(project, settings, engine)
            if stop_after == Stage.build:
                return
            artifacts, sums = package(project, outputs)
            console.print(f'Artifacts: {sums.parent}')
            if stop_after == Stage.package:
                return
            with Gitea(repository, settings.token_env) as api:
                remote_release = api.ensure_release(release_tag, project.commit, draft, prerelease)
                console.print(f'Release ready: {release_tag}')
                if stop_after == Stage.release:
                    return
                api.upload(remote_release['id'], [a.path for a in artifacts] + [sums], replace_assets)
            if stop_after == Stage.upload:
                return
            if remote_release.get('draft'):
                raise ValueError('Draft uploaded. Public verification requires publishing the draft; use --stop-after upload for draft workflows.')
            verify_published(repository.url, release_tag, artifacts, sums)
            if not skip_binstall_test:
                verify_binstall(project, settings.targets)
            console.print('Release completed and published artifacts verified.')
    except Exception as exc:
        if verbose:
            console.print(runner.redact(traceback.format_exc()))
        console.print('error: ' + runner.redact(str(exc)))
        raise typer.Exit(1) from None
