from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
import re
import shutil
import tempfile
from urllib.parse import urlsplit
from .runner import run, CommandError, console, register_secret


@dataclass(frozen=True)
class Repository:
    base_url: str
    owner: str
    name: str

    @property
    def url(self):
        return f'{self.base_url}/{self.owner}/{self.name}'


def parse_repository(value: str) -> Repository:
    scp = re.fullmatch(r'[^/@\s]+@([^/:\s]+):(.+)', value)
    if scp:
        value = f'https://{scp[1]}/{scp[2]}'
    parsed = urlsplit(value)
    if parsed.scheme not in ('http', 'https', 'ssh') or not parsed.hostname:
        raise ValueError('Repository must be an HTTP(S) or SSH Git URL; use --repository-url.')
    if parsed.password or (parsed.scheme != 'ssh' and parsed.username) or parsed.query or parsed.fragment:
        raise ValueError('Repository URLs must not contain credentials, query strings or fragments.')
    parts = parsed.path.strip('/').split('/')
    if len(parts) < 2 or any(not re.fullmatch(r'[A-Za-z0-9_.-]+', p) or p in ('.', '..') for p in parts):
        raise ValueError('Repository URL must end in owner/repository.')
    name = parts[-1].removesuffix('.git')
    if not name or name in ('.', '..'):
        raise ValueError('Repository name is empty.')
    authority = parsed.netloc if parsed.scheme != 'ssh' else parsed.hostname
    base = ('https' if parsed.scheme == 'ssh' else parsed.scheme) + '://' + authority
    if len(parts) > 2:
        base += '/' + '/'.join(parts[:-2])
    return Repository(base, parts[-2], name)


@contextmanager
def acquire_project(source: str, tag: str | None = None, keep_workdir: bool = False):
    local = Path(source).expanduser()
    temporary = None
    try:
        if local.exists():
            root = local.resolve()
            if not root.is_dir():
                raise ValueError('Local source must be a directory.')
        else:
            parse_repository(source)  # Reject options and credential-bearing URLs before Git logging.
            temporary = Path(tempfile.mkdtemp(prefix='binstall-publisher-'))
            root = temporary / 'source'
            run('git', 'clone', '--', source, str(root))
            if tag:
                run('git', 'checkout', '--detach', f'refs/tags/{tag}', cwd=root)
        if tag and not temporary:
            validate_checkout_tag(root, tag)
        yield root
    finally:
        if temporary:
            if keep_workdir:
                console.print(f'Temporary workdir kept at {temporary}')
            else:
                shutil.rmtree(temporary)


def validate_checkout_tag(root: Path, tag: str, *, required: bool = True):
    try:
        commit = run('git', 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}', cwd=root)
    except CommandError:
        if not required:
            return False
        raise ValueError(f'Git tag {tag!r} does not exist. Create the version tag explicitly first.') from None
    if commit != run('git', 'rev-parse', 'HEAD', cwd=root):
        raise ValueError(f'Git tag {tag!r} does not point to this checkout. Check it out explicitly.')
    return True


def origin_url(root: Path):
    try:
        return run('git', 'remote', 'get-url', 'origin', cwd=root)
    except CommandError:
        return None


def push_preparation(root: Path, branch: str, tag: str, repository: Repository, token_env: str):
    """Push explicit refs atomically; pass a matching Gitea token only to Git's askpass."""
    import os
    origin = run('git', 'remote', 'get-url', '--push', 'origin', cwd=root)
    # Never send a publishing token to a different host/repository selected by Git config.
    if parse_repository(origin).url != repository.url:
        raise ValueError('origin push URL differs from the release repository. Fix origin explicitly before pushing.')
    command = ('git', 'push', '--atomic', 'origin', f'HEAD:refs/heads/{branch}', f'refs/tags/{tag}')
    register_secret(token_env)
    secret = os.environ.get(token_env)
    if not secret or not origin.startswith('https://'):
        run(*command, cwd=root, capture=False, env={'GIT_TERMINAL_PROMPT': '0'})
        return
    with tempfile.TemporaryDirectory(prefix='publisher-git-auth-') as folder:
        helper = Path(folder) / 'askpass'
        helper.write_text('#!/usr/bin/env python3\nimport os, sys\n'
                          'print("token" if "username" in sys.argv[1].lower() '
                          'else os.environ["PUBLISHER_GIT_TOKEN"])\n')
        helper.chmod(0o700)
        environment = {'GIT_ASKPASS': str(helper), 'GIT_TERMINAL_PROMPT': '0',
                       'PUBLISHER_GIT_TOKEN': secret, 'GIT_TRACE': '0', 'GIT_TRACE_CURL': '0',
                       'GIT_TRACE_CURL_NO_DATA': '1', 'GIT_CURL_VERBOSE': '0',
                       'GIT_TRACE_PACKET': '0', 'GIT_TRACE2': '0', 'GIT_TRACE2_EVENT': '0',
                       'GIT_TRACE2_PERF': '0'}
        run('git', '-c', 'credential.helper=', *command[1:], cwd=root, env=environment, capture=False)
