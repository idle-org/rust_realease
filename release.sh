#!/bin/sh
set -eu

usage() {
    echo "usage: $0 <gitea-token-file> [rust_realease options...]" >&2
    exit 2
}

[ "$#" -ge 1 ] || usage
token_file=$1
shift

if [ ! -f "$token_file" ] || [ ! -r "$token_file" ]; then
    echo "token file is not a readable regular file: $token_file" >&2
    exit 2
fi

if ! command -v rust_realease >/dev/null 2>&1; then
    echo "rust_realease is not installed or not available in PATH" >&2
    exit 2
fi

repository_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
manifest_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$repository_dir/Cargo.toml" | head -n 1)

set -- \
    "$repository_dir" \
    --package rust_realease \
    --artifact-format both \
    --token-file "$token_file" \
    "$@"
case "$manifest_version" in
    *-*) set -- "$@" --prerelease ;;
esac

exec rust_realease "$@"
