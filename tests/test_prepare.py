from contextlib import redirect_stdout
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import tomlkit
from typer.testing import CliRunner
from worker.cli import app
from worker.build import build
from worker.config import load_config
from worker.models import RustProject
from worker.prepare import prepare_repository, preparation_files
from worker.project import parse_repository, push_preparation
from worker.runner import run


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.manifest = self.root / 'Cargo.toml'
        self.manifest.write_text('# Keep comments\n[package]\nname="demo"\nversion="0.1.0"\nedition="2021"\n')
        (self.root / 'Cargo.lock').write_text('# Fixture lockfile\nversion = 4\n')
        (self.root / '.gitignore').write_text('Cargo.lock\n')
        run('git', 'init', '-b', 'main', str(self.root))
        for key, value in [('user.name', 'Test'), ('user.email', 'test@example.invalid'),
                           ('commit.gpgsign', 'false'), ('tag.gpgsign', 'false')]:
            run('git', 'config', key, value, cwd=self.root)
        run('git', 'add', 'Cargo.toml', '.gitignore', cwd=self.root)
        run('git', 'commit', '-m', 'initial', cwd=self.root)
        self.repo = parse_repository('https://example.org/team/demo')
        self.project = RustProject(self.root, self.manifest, 'demo', '0.1.0',
                                   self.repo.url, ['demo'], tag='v0.1.0')
        self.config = load_config()

    def prepare(self, **kwargs):
        # Keep the large preview diffs out of normal test output.
        with patch('worker.prepare.console.print'):
            return prepare_repository(self.project, self.config, self.repo, **kwargs)

    def test_preview_does_not_mutate_files_git_or_run_cargo(self):
        before = self.manifest.read_bytes()
        head = run('git', 'rev-parse', 'HEAD', cwd=self.root)
        (self.root / 'Cargo.lock').unlink()
        with patch('worker.prepare.run', wraps=run) as commands:
            self.prepare(image_tag='main', dry_run=True, commit=True, create_tag=True)
        self.assertEqual(self.manifest.read_bytes(), before)
        self.assertFalse((self.root / 'Cross.toml').exists())
        self.assertFalse((self.root / 'Cargo.lock').exists())
        self.assertEqual(run('git', 'rev-parse', 'HEAD', cwd=self.root), head)
        self.assertFalse(run('git', 'tag', '--list', cwd=self.root))
        self.assertFalse(any(c.args[0] == 'cargo' for c in commands.call_args_list))

    def test_preparation_commits_only_managed_files_and_creates_tag(self):
        self.prepare(image_tag='main', commit=True, create_tag=True)
        tracked = set(run('git', 'ls-files', cwd=self.root).splitlines())
        self.assertEqual(tracked, {'Cargo.toml', 'Cargo.lock', 'Cross.toml', '.gitignore'})
        self.assertFalse(run('git', 'status', '--porcelain', cwd=self.root))
        head = run('git', 'rev-parse', 'HEAD', cwd=self.root)
        self.assertEqual(run('git', 'rev-parse', 'v0.1.0^{commit}', cwd=self.root), head)
        self.assertEqual(run('git', 'cat-file', '-t', 'v0.1.0', cwd=self.root), 'tag')
        self.assertIn('# Keep comments', self.manifest.read_text())
        cross = tomlkit.parse((self.root / 'Cross.toml').read_text())
        self.assertIn('RUSTUP_TOOLCHAIN', cross['build']['env']['passthrough'])
        self.assertEqual(len(cross['target']), 3)
        # A repeat of the same operation does not create another commit or move the tag.
        self.prepare(image_tag='main', commit=True, create_tag=True)
        self.assertEqual(run('git', 'rev-parse', 'HEAD', cwd=self.root), head)

    def test_commit_requires_clean_tree_and_index(self):
        (self.root / 'unrelated.txt').write_text('user work')
        for staged in (False, True):
            if staged:
                run('git', 'add', 'unrelated.txt', cwd=self.root)
            with self.assertRaisesRegex(ValueError, 'clean checkout'):
                self.prepare(commit=True)
        self.assertFalse((self.root / 'Cross.toml').exists())

    def test_existing_tag_blocks_changes_before_any_write(self):
        run('git', 'tag', 'v0.1.0', cwd=self.root)
        before = self.manifest.read_bytes()
        with self.assertRaisesRegex(ValueError, 'will not move'):
            self.prepare(image_tag='main', commit=True, create_tag=True)
        self.assertEqual(before, self.manifest.read_bytes())
        self.assertFalse((self.root / 'Cross.toml').exists())

    def test_missing_lock_is_generated_only_in_real_preparation(self):
        lock = self.root / 'Cargo.lock'
        lock.unlink()
        def fake_run(*args, **kwargs):
            self.assertEqual(args, ('cargo', 'generate-lockfile'))
            lock.write_text('version = 4\n')
        with patch('worker.prepare.run', side_effect=fake_run) as commands:
            self.prepare()
        commands.assert_called_once()
        self.assertTrue(lock.exists())

    def test_preserves_custom_cross_config_and_explicit_image_override(self):
        path = self.root / 'Cross.toml'
        path.write_text('# Custom settings\n[build.env]\npassthrough=["MY_BUILD_VAR"]\n'
                        '[target.x86_64-unknown-linux-musl]\nimage="my/custom:image"\n')
        plan = preparation_files(self.project, self.config, self.repo)
        result = tomlkit.parse(plan[path])
        self.assertEqual(result['target']['x86_64-unknown-linux-musl']['image'], 'my/custom:image')
        self.assertEqual(result['build']['env']['passthrough'], ['MY_BUILD_VAR', 'RUSTUP_TOOLCHAIN'])
        self.assertIn('# Custom settings', plan[path])
        with self.assertRaises(ValueError):
            preparation_files(self.project, self.config, self.repo, 'bad/tag')

    def test_no_git_writes_without_explicit_flags(self):
        head = run('git', 'rev-parse', 'HEAD', cwd=self.root)
        self.prepare()
        self.assertEqual(run('git', 'rev-parse', 'HEAD', cwd=self.root), head)
        self.assertFalse(run('git', 'tag', '--list', cwd=self.root))
        for flags in ({'create_tag': True}, {'push': True}, {'commit': True, 'push': True}):
            with self.assertRaises(ValueError):
                self.prepare(**flags)

    def test_cli_preparation_does_not_require_existing_tag_or_publish(self):
        with patch('worker.cli.inspect_project', return_value=self.project), \
             patch('worker.cli.Gitea') as api, patch('worker.cli.build') as build_mock:
            result = CliRunner().invoke(app, [str(self.root), '--prepare-repo', '--dry-run',
                                              '--tag', 'v0.1.0', '--cross-image-tag', 'main'])
        self.assertEqual(result.exit_code, 0, result.output)
        api.assert_not_called()
        build_mock.assert_not_called()
        result = CliRunner().invoke(app, [str(self.root), '--create-tag'])
        self.assertNotEqual(result.exit_code, 0)
        self.assertIn('require --prepare-repo', result.output)

    def test_fresh_builds_never_reuse_previous_output(self):
        old_dirs = []
        def fake_run(*args, **kwargs):
            if args[0] == 'rustup':
                return 'stable-x86_64-unknown-linux-gnu (default)'
            directory = Path(args[args.index('--target-dir') + 1])
            old_dirs.append(directory)
            binary = directory / 'x86_64-unknown-linux-musl/release/demo'
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b'fresh')
            self.assertEqual(kwargs['env']['RUSTUP_TOOLCHAIN'], 'stable-x86_64-unknown-linux-gnu')
            return ''
        settings = load_config(targets=['x86_64-unknown-linux-musl'])
        with patch('worker.build.run', side_effect=fake_run), patch('worker.build.shutil.which', return_value='/rustup'):
            first = build(self.project, settings, 'podman')
            second = build(self.project, settings, 'podman')
        self.assertNotEqual(old_dirs[0], old_dirs[1])
        self.assertTrue(next(iter(first.values())).exists())
        self.assertTrue(next(iter(second.values())).exists())

    def test_push_uses_atomic_refs_and_token_only_in_askpass_environment(self):
        def fake_run(*args, **kwargs):
            if args[:3] == ('git', 'remote', 'get-url'):
                return self.repo.url + '.git'
            self.assertIn('--atomic', args)
            self.assertIn('HEAD:refs/heads/main', args)
            self.assertIn('refs/tags/v0.1.0', args)
            self.assertNotIn('--force', args)
            self.assertNotIn('test-secret', ' '.join(args))
            env = kwargs['env']
            self.assertEqual(env['PUBLISHER_GIT_TOKEN'], 'test-secret')
            self.assertTrue(Path(env['GIT_ASKPASS']).exists())
            self.assertNotIn('test-secret', Path(env['GIT_ASKPASS']).read_text())
            return ''
        with patch.dict(os.environ, {'GITEA_TOKEN': 'test-secret'}), patch('worker.project.run', side_effect=fake_run) as commands:
            push_preparation(self.root, 'main', 'v0.1.0', self.repo, 'GITEA_TOKEN')
        self.assertEqual(commands.call_count, 2)
        with patch('worker.project.run', return_value='https://other.invalid/team/demo') as commands:
            with self.assertRaisesRegex(ValueError, 'differs'):
                push_preparation(self.root, 'main', 'v0.1.0', self.repo, 'GITEA_TOKEN')
        commands.assert_called_once()
