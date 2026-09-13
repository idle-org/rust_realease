"""Single source of truth for archive and cargo-binstall conventions."""


def expected_tag(version: str, prefix: str = 'v') -> str:
    return prefix + version


def archive_root(name: str, target: str, version: str) -> str:
    return f'{name}-{target}-v{version}'


def archive_name(name: str, target: str, version: str) -> str:
    return archive_root(name, target, version) + '.tgz'


def binstall_metadata(prefix: str = 'v') -> dict[str, str]:
    root = archive_root('{ name }', '{ target }', '{ version }')
    return {
        'pkg-url': '{ repo }/releases/download/' + prefix + '{ version }/' + root + '.tgz',
        'pkg-fmt': 'tgz',
        'bin-dir': root + '/{ bin }{ binary-ext }',
    }


def asset_url(repository: str, tag: str, filename: str) -> str:
    from urllib.parse import quote
    return f'{repository}/releases/download/{quote(tag, safe="")}/{quote(filename, safe="")}'
