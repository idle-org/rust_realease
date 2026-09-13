"""Gitea API operations, isolated for MockTransport tests."""
import os
from pathlib import Path
from urllib.parse import quote
import httpx
from .project import Repository
from .runner import console


class Gitea:
    def __init__(self, repository: Repository, token_env: str, transport=None):
        token = os.environ.get(token_env)
        if not token:
            raise ValueError(f'Set {token_env} before publishing.')
        if not repository.base_url.startswith('https://'):
            raise ValueError('Authenticated Gitea publishing requires HTTPS.')
        self.client = httpx.Client(
            base_url=repository.base_url + '/api/v1/',
            headers={'Authorization': 'token ' + token}, timeout=120,
            follow_redirects=False, transport=transport)
        self.path = f'repos/{quote(repository.owner, safe="")}/{quote(repository.name, safe="")}'

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.client.close()

    def request(self, method, path, *, missing_ok=False, **kwargs):
        try:
            response = self.client.request(method, self.path + path, **kwargs)
        except httpx.HTTPError:
            raise RuntimeError(f'Gitea {method} {path}: connection failed.') from None
        if missing_ok and response.status_code == 404:
            return None
        if not response.is_success:
            # Do not echo server response bodies: they can reflect credentials.
            raise RuntimeError(f'Gitea {method} {path}: HTTP {response.status_code}.')
        return response.json() if response.content else None

    def ensure_release(self, tag, commit, draft=False, prerelease=False):
        remote_tag = self.request('GET', '/tags/' + quote(tag, safe=''), missing_ok=True)
        if remote_tag is None:
            raise ValueError(f'Remote tag {tag!r} is missing. Push the tag explicitly before publishing.')
        if remote_tag.get('commit', {}).get('sha') != commit:
            raise ValueError('Remote tag commit differs from the checkout; refusing publication.')
        release = self.request('GET', '/releases/tags/' + quote(tag, safe=''), missing_ok=True)
        if release is None:
            release = self.request('POST', '/releases', json={
                'tag_name': tag, 'name': tag, 'target_commitish': commit,
                'draft': draft, 'prerelease': prerelease})
        return release

    def assets(self, release_id):
        # Gitea's release attachment endpoint returns the full list, not paginated results.
        return self.request('GET', f'/releases/{release_id}/assets')

    def upload(self, release_id, paths: list[Path], replace=False):
        existing = self.assets(release_id)
        names = {p.name for p in paths}
        collisions = [a for a in existing if a['name'] in names]
        if collisions and not replace:
            raise ValueError('Release assets already exist: ' + ', '.join(a['name'] for a in collisions) +
                             '. Use --replace-assets to replace them explicitly.')
        for path in paths:
            for asset in collisions:
                if asset['name'] == path.name:
                    self.request('DELETE', f'/releases/{release_id}/assets/{asset["id"]}')
            with path.open('rb') as stream:
                self.request('POST', f'/releases/{release_id}/assets',
                             params={'name': path.name}, files={'attachment': (path.name, stream, 'application/octet-stream')})
            console.print(f'Uploaded {path.name}')
