import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from worker.build import build, verify_source
from worker.config import load_config
from worker.manifest import inspect_project, metadata_issues, repair_metadata


class WorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.root_manifest = self.root / 'Cargo.toml'
        self.root_manifest.write_text('[workspace]\nmembers=["cli", "library"]\n[workspace.package]\nversion="1.2.3"\n')
        self.packages = []
        for name, kind in [('cli', 'bin'), ('library', 'lib')]:
            folder = self.root / name
            folder.mkdir()
            manifest = folder / 'Cargo.toml'
            manifest.write_text(f'# Preserve member comment\n[package]\nname="{name}"\nversion.workspace=true\n')
            self.packages.append({'id': name, 'name': name, 'version': '1.2.3',
                                  'repository': 'https://example.org/team/tools',
                                  'manifest_path': str(manifest),
                                  'targets': [{'name': name, 'kind': [kind]}]})
        self.metadata = {'packages': self.packages, 'workspace_members': ['cli', 'library'],
                         'workspace_root': str(self.root), 'target_directory': str(self.root / 'target')}

    def inspect(self, name=None, root=None):
        with patch('worker.manifest.run', side_effect=[json.dumps(self.metadata), 'a' * 40]):
            return inspect_project(root or self.root, name)

    def test_explicit_member_and_inherited_version(self):
        project = self.inspect('cli')
        self.assertEqual(project.manifest, self.root / 'cli/Cargo.toml')
        self.assertEqual(project.root, self.root)
        self.assertEqual(project.version, '1.2.3')
        self.assertEqual(project.repository, 'https://example.org/team/tools')
        self.assertEqual(project.target_directory, self.root / 'target')

    def test_ambiguous_root_and_member_path_require_selection(self):
        for source in (self.root, self.root / 'cli'):
            with self.assertRaisesRegex(ValueError, '--package NAME; discovered: cli, library'):
                self.inspect(root=source)

    def test_unknown_library_and_multi_binary_rejected(self):
        with self.assertRaisesRegex(ValueError, 'not found uniquely; discovered: cli, library'):
            self.inspect('unknown')
        with self.assertRaisesRegex(ValueError, 'exactly one binary; discovered: none'):
            self.inspect('library')
        self.packages[0]['targets'].append({'name': 'second', 'kind': ['bin']})
        with self.assertRaisesRegex(ValueError, 'cli, second'):
            self.inspect('cli')

    def test_nonmembers_are_not_selectable(self):
        self.metadata['workspace_members'] = ['cli']
        with self.assertRaisesRegex(ValueError, 'not found uniquely; discovered: cli'):
            self.inspect('library')

    def test_only_selected_manifest_is_repaired(self):
        root_before = self.root_manifest.read_bytes()
        sibling = self.root / 'library/Cargo.toml'
        sibling_before = sibling.read_bytes()
        project = self.inspect('cli')
        repair_metadata(project.manifest, metadata_issues(project.manifest, project.repository))
        self.assertEqual(self.root_manifest.read_bytes(), root_before)
        self.assertEqual(sibling.read_bytes(), sibling_before)
        self.assertIn('version.workspace=true', project.manifest.read_text())
        self.assertIn('# Preserve member comment', project.manifest.read_text())
        self.assertEqual(metadata_issues(project.manifest, project.repository), [])

    def test_build_check_and_test_select_package_at_workspace_root(self):
        project = self.inspect('cli', self.root / 'cli')
        settings = load_config(targets=['x86_64-unknown-linux-musl'])
        def fake_run(*args, **kwargs):
            if args[:2] == ('cross', 'build'):
                target_dir = Path(args[args.index('--target-dir') + 1])
                binary = target_dir / settings.targets[0] / 'release/cli'
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b'fixture')
            return ''
        with patch('worker.build.run', side_effect=fake_run) as run, patch('worker.build.shutil.which', return_value=None):
            verify_source(project, settings, False)
            outputs = build(project, settings, 'podman')
        self.assertTrue(outputs[settings.targets[0]].is_file())
        self.assertIn('binstall-publisher', outputs[settings.targets[0]].parts)
        self.assertEqual([c.args[:2] for c in run.call_args_list],
                         [('cargo', 'check'), ('cargo', 'test'), ('cross', 'build')])
        for call in run.call_args_list:
            self.assertEqual(call.args[call.args.index('--package') + 1], 'cli')
            self.assertEqual(call.kwargs['cwd'], self.root)
            self.assertIn('--locked', call.args)
        self.assertIn('--bin', run.call_args.args)

    def test_single_package_still_works_without_selection(self):
        self.metadata['workspace_members'] = ['cli']
        project = self.inspect(root=self.root / 'cli')
        self.assertEqual(project.package_name, 'cli')
