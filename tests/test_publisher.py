import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import httpx
from typer.testing import CliRunner
from worker.cli import app
from worker.config import load_config
from worker.gitea import Gitea
from worker.manifest import inspect_project, metadata_issues, repair_metadata
from worker.models import RustProject, BuildTarget
from worker.package import package, checksum
from worker.policy import archive_name, binstall_metadata, expected_tag
from worker.project import parse_repository, acquire_project, validate_checkout_tag
from worker.runner import run, CommandError
from worker.verify import verify_archive, verify_published, verify_binstall


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.manifest = self.root / 'Cargo.toml'
        self.manifest.write_text('# Keep this comment\n[package]\nname = "demo"\nversion = "0.1.0"\n# Another comment\n')
        self.project = RustProject(self.root, self.manifest, 'demo', '0.1.0',
                                   'https://host/gitea/org/demo', ['tool'], 'a' * 40)

    def artifacts(self):
        binary = self.root / 'tool'
        binary.write_bytes(b'example binary\x00')
        binary.chmod(0o755)
        return package(self.project, {'x86_64-unknown-linux-musl': binary,
                                      'x86_64-pc-windows-gnu': binary})


class PolicyTests(Fixture):
    def test_repository_urls(self):
        for url in ('https://host/gitea/org/demo.git', 'https://host/gitea/org/demo/',
                    'git@host:gitea/org/demo.git', 'ssh://git@host/gitea/org/demo.git'):
            repo = parse_repository(url)
            self.assertEqual(repo.url, 'https://host/gitea/org/demo')
            self.assertEqual(repo.base_url, 'https://host/gitea')
        self.assertEqual(parse_repository('https://host:3000/org/demo').base_url, 'https://host:3000')
        for bad in ('--upload-pack=evil', 'file:///tmp/repo', 'https://token@host/org/repo',
                    'https://host/org/repo?token=secret', 'https://host/../repo', 'https://host/org'):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                parse_repository(bad)

    def test_naming(self):
        self.assertEqual(expected_tag('0.1.0'), 'v0.1.0')
        self.assertEqual(expected_tag('0.1.0', 'release-'), 'release-0.1.0')
        self.assertEqual(archive_name('demo', 'target', '0.1.0'), 'demo-target-v0.1.0.tgz')
        self.assertEqual(BuildTarget('x86_64-pc-windows-gnu').binary_filename('tool'), 'tool.exe')
        self.assertEqual(BuildTarget('aarch64-unknown-linux-musl').binary_filename('tool'), 'tool')
        self.assertIn('/release-{ version }/', binstall_metadata('release-')['pkg-url'])
        self.assertEqual(binstall_metadata()['bin-dir'], '{ name }-{ target }-v{ version }/{ bin }{ binary-ext }')

    def test_manifest_repair_preserves_comments_and_is_idempotent(self):
        issues = metadata_issues(self.manifest, self.project.repository)
        self.assertEqual(len(issues), 4)
        repair_metadata(self.manifest, issues)
        first = self.manifest.read_text()
        self.assertIn('# Keep this comment', first)
        self.assertIn('# Another comment', first)
        self.assertEqual(metadata_issues(self.manifest, self.project.repository), [])
        repair_metadata(self.manifest, [])
        self.assertEqual(self.manifest.read_text(), first)

    def test_conflicting_overrides_removed(self):
        self.manifest.write_text(self.manifest.read_text() + '\n[package.metadata.binstall.overrides.x86_64-pc-windows-gnu]\npkg-fmt="zip"\n')
        issues = metadata_issues(self.manifest, self.project.repository)
        self.assertIn('package.metadata.binstall.overrides', [i.key for i in issues])
        repair_metadata(self.manifest, issues)
        self.assertEqual(metadata_issues(self.manifest, self.project.repository), [])

    def test_workspace_and_multiple_binaries_rejected(self):
        data = {'packages': [{'id': 'demo', 'name': 'demo', 'targets': [{'kind': ['bin'], 'name': 'one'},
                                                        {'kind': ['bin'], 'name': 'two'}]}],
                'workspace_members': ['demo']}
        with patch('worker.manifest.run', return_value=json.dumps(data)), self.assertRaisesRegex(ValueError, 'one, two'):
            inspect_project(self.root)
        self.manifest.write_text('[workspace]\nmembers=[]\n')
        with patch('worker.manifest.run', return_value=json.dumps(data)), self.assertRaisesRegex(ValueError, '--package'):
            inspect_project(self.root)

    def test_symlink_manifest_cannot_repair_unrelated_file(self):
        other = self.root / 'other.toml'
        original = self.manifest.read_bytes()
        self.manifest.rename(other)
        self.manifest.symlink_to(other)
        with self.assertRaisesRegex(ValueError, 'symlinked'):
            repair_metadata(self.manifest, metadata_issues(self.manifest, self.project.repository))
        self.assertEqual(other.read_bytes(), original)

    def test_cross_metadata_hooks_rejected(self):
        from worker.build import validate_cross_config
        self.manifest.write_text(self.manifest.read_text() + '\n[workspace.metadata.cross.target.x86_64-unknown-linux-musl]\npre-build=["echo unsafe"]\n')
        with self.assertRaisesRegex(ValueError, 'executable option'):
            validate_cross_config(self.root)

    def test_config_overrides_and_validation(self):
        config = load_config(targets=['x86_64-pc-windows-gnu'], locked=False, profile='dev')
        self.assertFalse(config.locked)
        self.assertEqual(config.profile, 'dev')
        for targets in ([], ['../../oops'], ['--target']):
            with self.assertRaises(ValueError):
                load_config(targets=targets)

    def test_reproducible_archives_and_checksums(self):
        artifacts, sums = self.artifacts()
        again, _ = self.artifacts()
        self.assertEqual([a.sha256 for a in artifacts], [a.sha256 for a in again])
        for artifact in artifacts:
            verify_archive(artifact.path, artifact)
            self.assertEqual(artifact.sha256, hashlib.sha256(artifact.path.read_bytes()).hexdigest())
            self.assertIn(f'{artifact.sha256}  {artifact.path.name}\n', sums.read_text())
        self.assertEqual(len(list(sums.parent.iterdir())), 3)

    def test_malformed_archive_rejected_without_extraction(self):
        artifacts, _ = self.artifacts()
        path = self.root / 'malformed.tgz'
        with tarfile.open(path, 'w:gz') as archive:
            entry = tarfile.TarInfo('../../outside')
            entry.size = 1
            archive.addfile(entry, io.BytesIO(b'x'))
        with self.assertRaises(ValueError):
            verify_archive(path, artifacts[0])
        self.assertFalse((self.root.parent / 'outside').exists())

    def test_published_bytes_and_checksum_file(self):
        artifacts, sums = self.artifacts()
        paths = {p.name: p.read_bytes() for p in [a.path for a in artifacts] + [sums]}
        def handler(request):
            self.assertNotIn('authorization', request.headers)
            return httpx.Response(200, content=paths[request.url.path.rsplit('/', 1)[1]])
        verify_published(self.project.repository, 'v0.1.0', artifacts, sums, httpx.MockTransport(handler))
        with self.assertRaisesRegex(ValueError, 'SHA-256'):
            verify_published(self.project.repository, 'v0.1.0', artifacts, sums,
                             httpx.MockTransport(lambda r: httpx.Response(200, content=b'bad')))

    def test_runner_failure_and_secret_redaction(self):
        with patch.dict(os.environ, {'GITEA_TOKEN': 'super-secret'}):
            with self.assertRaises(CommandError) as failure:
                run('python3', '-c', 'import sys; print("super-secret", file=sys.stderr); sys.exit(3)')
            self.assertNotIn('super-secret', str(failure.exception))
            self.assertIn('(3)', str(failure.exception))
            self.assertIn('[REDACTED]', str(failure.exception))
            self.assertEqual(run('python3', '-c', 'import os; print(os.getenv("GITEA_TOKEN", "missing"))'), 'missing')


class GiteaTests(Fixture):
    def client(self, handler):
        env = patch.dict(os.environ, {'GITEA_TOKEN': 'secret-token'})
        env.start()
        self.addCleanup(env.stop)
        client = Gitea(parse_repository(self.project.repository), 'GITEA_TOKEN', httpx.MockTransport(handler))
        self.addCleanup(client.client.close)
        return client

    def test_create_and_reuse_release(self):
        for exists in (False, True):
            calls = []
            def handler(request):
                calls.append(request)
                self.assertTrue(request.url.path.startswith('/gitea/api/v1/repos/org/demo/'))
                if '/tags/v0.1.0' in request.url.path and '/releases/' not in request.url.path:
                    return httpx.Response(200, json={'commit': {'sha': self.project.commit}})
                if request.method == 'POST':
                    body = json.loads(request.content)
                    self.assertTrue(body['prerelease'])
                    self.assertEqual(body['tag_name'], 'v0.1.0')
                    return httpx.Response(201, json={'id': 1})
                return httpx.Response(200 if exists else 404, json={'id': 1})
            self.assertEqual(self.client(handler).ensure_release('v0.1.0', self.project.commit, prerelease=True)['id'], 1)
            self.assertEqual(sum(r.method == 'POST' for r in calls), int(not exists))

    def test_collision_preflight_and_replace(self):
        paths = [self.root / 'one.tgz', self.root / 'two.tgz']
        for path in paths:
            path.write_bytes(b'archive')
        for replace in (False, True):
            calls = []
            def handler(request):
                calls.append(request.method)
                if request.method == 'GET':
                    return httpx.Response(200, json=[{'id': 9, 'name': 'two.tgz'}])
                return httpx.Response(204) if request.method == 'DELETE' else httpx.Response(201, json={'id': 10})
            client = self.client(handler)
            if replace:
                client.upload(1, paths, True)
                self.assertEqual(calls, ['GET', 'POST', 'DELETE', 'POST'])
            else:
                with self.assertRaisesRegex(ValueError, 'already exist'):
                    client.upload(1, paths)
                self.assertEqual(calls, ['GET'])

    def test_auth_failure_does_not_echo_body(self):
        client = self.client(lambda r: httpx.Response(401, text='secret-token'))
        with self.assertRaises(RuntimeError) as failure:
            client.ensure_release('v0.1.0', self.project.commit)
        self.assertNotIn('secret-token', str(failure.exception))
        self.assertIn('401', str(failure.exception))

    def test_wrong_remote_commit_blocks_creation(self):
        client = self.client(lambda r: httpx.Response(200, json={'commit': {'sha': 'wrong'}}))
        with self.assertRaisesRegex(ValueError, 'differs'):
            client.ensure_release('v0.1.0', self.project.commit)


class CliTests(Fixture):
    def test_dry_run_never_repairs_or_builds(self):
        original = self.manifest.read_bytes()
        with patch('worker.cli.inspect_project', return_value=self.project), \
             patch('worker.cli.prerequisites', return_value=([], 'podman')), \
             patch('worker.cli.validate_checkout_tag', return_value=True), \
             patch('worker.cli.build') as build, patch('worker.cli.Gitea') as api:
            result = CliRunner().invoke(app, [str(self.root), '--dry-run', '--fix-metadata'])
        self.assertEqual(result.exit_code, 1, result.output)
        self.assertIn('Dry run complete', result.output)
        self.assertEqual(self.manifest.read_bytes(), original)
        build.assert_not_called()
        api.assert_not_called()

    def test_stop_metadata_and_tag_mismatch(self):
        with patch('worker.cli.inspect_project', return_value=self.project), patch('worker.cli.prerequisites') as prereq:
            result = CliRunner().invoke(app, [str(self.root), '--fix-metadata', '--stop-after', 'metadata'])
            self.assertEqual(result.exit_code, 0, result.output)
            prereq.assert_not_called()
        with patch('worker.cli.inspect_project', return_value=self.project):
            result = CliRunner().invoke(app, [str(self.root), '--tag', 'v9.9.9'])
            self.assertNotEqual(result.exit_code, 0)

    def test_stop_package_never_contacts_gitea(self):
        repair_metadata(self.manifest, metadata_issues(self.manifest, self.project.repository))
        binary = self.root / 'binary'
        binary.write_bytes(b'fake')
        with patch('worker.cli.inspect_project', return_value=self.project), \
             patch('worker.cli.prerequisites', return_value=([], 'podman')), \
             patch('worker.cli.validate_checkout_tag', return_value=False), \
             patch('worker.cli.verify_source'), \
             patch('worker.cli.build', return_value={'x86_64-unknown-linux-musl': binary}), \
             patch('worker.cli.Gitea') as api:
            result = CliRunner().invoke(app, [str(self.root), '--stop-after', 'package'])
        self.assertEqual(result.exit_code, 0, result.output)
        api.assert_not_called()
        self.assertEqual(len(list((self.root / 'dist').glob('*/SHA256SUMS'))), 1)

    def test_local_source_never_removed(self):
        with acquire_project(str(self.root)) as root:
            self.assertEqual(root, self.root)
        self.assertTrue(self.manifest.exists())


class WrapperTests(Fixture):
    def setUp(self):
        super().setUp()
        self.wrapper = self.root / 'release.sh'
        shutil.copy(Path(__file__).resolve().parents[1] / 'release.sh', self.wrapper)

    def invoke(self, env):
        clean = os.environ.copy()
        clean.pop('VIRTUAL_ENV', None)
        clean.update(env)
        return subprocess.run(['bash', str(self.wrapper), '--help'], stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, env=clean, cwd='/tmp')

    def test_no_venv_noninteractive(self):
        result = self.invoke({})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('non-interactive', result.stderr)

    def test_existing_venv_needs_activation(self):
        (self.root / '.venv').mkdir()
        result = self.invoke({})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(str(self.root / '.venv/bin/activate'), result.stdout)

    def test_wrong_venv_even_without_project_venv(self):
        result = self.invoke({'VIRTUAL_ENV': '/tmp'})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('non-interactive', result.stderr)

    def test_correct_venv_preserves_cwd(self):
        bin_dir = self.root / '.venv' / 'bin'
        bin_dir.mkdir(parents=True)
        interpreter = bin_dir / 'python'
        interpreter.write_text('#!/bin/sh\npwd\nprintf "%s\\n" "$@"\n')
        interpreter.chmod(0o755)
        result = self.invoke({'VIRTUAL_ENV': str(bin_dir.parent)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ['/tmp', '-P', '-m', 'worker', '--help'])


class PipelineTests(Fixture):
    def test_full_pipeline_and_every_stage_boundary(self):
        from contextlib import ExitStack
        stages = ['inspect', 'metadata', 'verify', 'build', 'package', 'release', 'upload', 'binstall']
        repair_metadata(self.manifest, metadata_issues(self.manifest, self.project.repository))
        binary = self.root / 'tool'
        binary.write_bytes(b'release binary')
        for index, stage in enumerate(stages):
            calls = []
            with self.subTest(stage=stage), ExitStack() as stack:
                stack.enter_context(patch.dict(os.environ, {'GITEA_TOKEN': 'test-token'}))
                stack.enter_context(patch('worker.cli.inspect_project', return_value=self.project))
                stack.enter_context(patch('worker.cli.runner.run', return_value=''))
                stack.enter_context(patch('worker.cli.validate_checkout_tag', return_value=True))
                stack.enter_context(patch('worker.cli.prerequisites', return_value=([], 'podman')))
                stack.enter_context(patch('worker.cli.verify_source', side_effect=lambda *a: calls.append('verify')))
                def fake_build(*args):
                    calls.append('build')
                    return {'x86_64-unknown-linux-musl': binary}
                stack.enter_context(patch('worker.cli.build', side_effect=fake_build))
                def fake_package(*args):
                    calls.append('package')
                    return package(*args)
                stack.enter_context(patch('worker.cli.package', side_effect=fake_package))
                api = stack.enter_context(patch('worker.cli.Gitea')).return_value.__enter__.return_value
                def fake_release(*args):
                    calls.append('release')
                    return {'id': 1, 'draft': False}
                api.ensure_release.side_effect = fake_release
                api.upload.side_effect = lambda *a: calls.append('upload')
                stack.enter_context(patch('worker.cli.verify_published', side_effect=lambda *a: calls.append('download')))
                stack.enter_context(patch('worker.cli.verify_binstall', side_effect=lambda *a: calls.append('binstall')))
                result = CliRunner().invoke(app, [str(self.root), '--stop-after', stage])
                self.assertEqual(result.exit_code, 0, result.output)
                expected = stages[2:index + 1]
                if stage == 'binstall':
                    expected = expected[:-1] + ['download', 'binstall']
                self.assertEqual(calls, expected)

    def test_git_local_tag_policy_and_clone_cleanup(self):
        run('git', 'init', str(self.root))
        run('git', 'add', 'Cargo.toml', cwd=self.root)
        run('git', '-c', 'commit.gpgsign=false', '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'fixture', cwd=self.root)
        run('git', '-c', 'tag.gpgsign=false', 'tag', 'v0.1.0', cwd=self.root)
        self.assertTrue(validate_checkout_tag(self.root, 'v0.1.0'))
        self.assertFalse(validate_checkout_tag(self.root, 'v0.2.0', required=False))
        self.manifest.write_text(self.manifest.read_text() + '\n# newer commit\n')
        run('git', 'add', 'Cargo.toml', cwd=self.root)
        run('git', '-c', 'commit.gpgsign=false', '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'second', cwd=self.root)
        with self.assertRaisesRegex(ValueError, 'does not point'):
            validate_checkout_tag(self.root, 'v0.1.0')
        managed = self.root / 'managed'
        managed.mkdir()
        with patch('worker.project.tempfile.mkdtemp', return_value=str(managed)), \
             patch('worker.project.run', side_effect=CommandError('clone failed')):
            with self.assertRaises(CommandError):
                with acquire_project('https://host/org/demo'):
                    pass
        self.assertFalse(managed.exists())
        self.assertTrue(self.manifest.exists())


if __name__ == '__main__':
    unittest.main()
