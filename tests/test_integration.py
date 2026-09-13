"""Opt-in real cargo-binstall test against a disposable local asset server.

Run: PUBLISHER_INTEGRATION=1 python -m unittest discover -s tests -v
No Gitea token or cross toolchain is required.
"""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import shutil
import tempfile
from threading import Thread
import unittest
from unittest.mock import patch

from worker.build import verify_source
from worker.config import load_config
from worker.manifest import inspect_project, metadata_issues, repair_metadata
from worker.models import RustProject
from worker.package import package
from worker.runner import run
from worker.verify import verify_published, verify_binstall


@unittest.skipUnless(os.environ.get('PUBLISHER_INTEGRATION') == '1' and shutil.which('cargo-binstall'),
                     'set PUBLISHER_INTEGRATION=1 to exercise real cargo-binstall')
class BinstallIntegration(unittest.TestCase):
    def test_install_every_default_target_from_manifest(self):
        self.install_fixture(workspace=False)

    def test_install_workspace_member_with_inherited_version(self):
        self.install_fixture(workspace=True)

    def install_fixture(self, workspace):
        class QuietHandler(SimpleHTTPRequestHandler):
            def log_message(self, *args):
                pass
        with tempfile.TemporaryDirectory(prefix='publisher-integration-') as folder:
            root = Path(folder)
            with ThreadingHTTPServer(('127.0.0.1', 0), partial(QuietHandler, directory=folder)) as server:
                thread = Thread(target=server.serve_forever, daemon=True)
                thread.start()
                try:
                    repository = f'http://127.0.0.1:{server.server_port}/org/publisher-test'
                    manifest = root / 'Cargo.toml'
                    manifest.write_text('[package]\nname="publisher-test"\nversion="0.1.0"\nedition="2021"\n\n[[bin]]\nname="publisher-test"\npath="main.rs"\n')
                    (root / 'main.rs').write_text('fn main() {}\n')
                    if workspace:
                        member = root / 'cli'
                        member.mkdir()
                        (root / 'main.rs').rename(member / 'main.rs')
                        manifest.rename(member / 'Cargo.toml')
                        manifest.write_text('[workspace]\nmembers=["cli", "unused"]\n[workspace.package]\nversion="0.1.0"\n')
                        unused = root / 'unused'
                        (unused / 'src').mkdir(parents=True)
                        (unused / 'Cargo.toml').write_text('[package]\nname="unused"\nversion="0.1.0"\n')
                        (unused / 'src/lib.rs').write_text('compile_error!("unselected member must not be compiled");\n')
                        manifest = member / 'Cargo.toml'
                        manifest.write_text(manifest.read_text().replace('version="0.1.0"', 'version.workspace=true'))
                    repair_metadata(manifest, metadata_issues(manifest, repository))
                    project = RustProject(root, manifest, 'publisher-test', '0.1.0', repository, ['publisher-test'])
                    if workspace:
                        project = inspect_project(root, 'publisher-test')
                        verify_source(project, load_config(locked=False), skip_tests=False)
                    binary = root / 'fake-binary'
                    binary.write_bytes(b'not executed: publisher contract fixture\n')
                    binary.chmod(0o755)
                    targets = ['x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl', 'x86_64-pc-windows-gnu']
                    artifacts, sums = package(project, dict.fromkeys(targets, binary))
                    download = root / 'org/publisher-test/releases/download/v0.1.0'
                    download.mkdir(parents=True)
                    for path in [a.path for a in artifacts] + [sums]:
                        shutil.copy(path, download / path.name)
                    verify_published(repository, 'v0.1.0', artifacts, sums)
                    # Plain HTTP is allowed only for this loopback test server.
                    def local_run(*args, **kwargs):
                        return run(*args, '--allow-insecure-http', **kwargs)
                    with patch('worker.verify.run', side_effect=local_run):
                        verify_binstall(project, targets)
                finally:
                    server.shutdown()
                    thread.join()
