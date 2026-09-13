from dataclasses import dataclass
from pathlib import Path
import re
from collections.abc import Mapping
import tomlkit

DEFAULT_CONFIG = Path(__file__).resolve().parent.parent / 'config.toml'


@dataclass
class Config:
    targets: list[str]
    profile: str
    locked: bool
    tag_prefix: str
    token_env: str


def load_config(path: Path = DEFAULT_CONFIG, *, targets=None, profile=None, locked=None, tag_prefix=None) -> Config:
    data = tomlkit.parse(path.read_text())
    for section in ('build', 'release', 'gitea', 'package'):
        if not isinstance(data.get(section, {}), Mapping):
            raise ValueError(f'Configuration [{section}] must be a table.')
    build, release, gitea = (data.get(s, {}) for s in ('build', 'release', 'gitea'))
    result = Config(targets if targets is not None else build.get('targets', []),
                    profile if profile is not None else build.get('profile', 'release'),
                    locked if locked is not None else build.get('locked', True),
                    tag_prefix if tag_prefix is not None else release.get('tag-prefix', 'v'), gitea.get('token-env', 'GITEA_TOKEN'))
    if not isinstance(result.targets, list) or not result.targets or any(
        not isinstance(t, str) or not re.fullmatch(r'[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+){2,}', t)
        for t in result.targets
    ):
        raise ValueError('build.targets must be a nonempty list of Rust target triples.')
    result.targets = list(dict.fromkeys(result.targets))
    if not isinstance(result.profile, str) or not re.fullmatch(r'[A-Za-z][A-Za-z0-9_-]*', result.profile):
        raise ValueError('build.profile must be a Cargo profile name.')
    if not isinstance(result.locked, bool):
        raise ValueError('build.locked must be a boolean.')
    if not isinstance(result.tag_prefix, str) or not re.fullmatch(r'[A-Za-z0-9._-]*', result.tag_prefix):
        raise ValueError('release.tag-prefix must contain only letters, digits, dots, underscores or hyphens.')
    if not isinstance(result.token_env, str) or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', result.token_env):
        raise ValueError('gitea.token-env must be an environment variable name.')
    if data.get('package', {}).get('format', 'tgz') != 'tgz':
        raise ValueError('Only package.format = "tgz" is supported.')
    return result
