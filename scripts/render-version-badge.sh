#!/bin/sh
set -eu

usage() {
    echo "usage: $0 <package-version> <output.svg>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
version=$1
output=$2
case "$version" in
    ''|*[!0-9A-Za-z.+-]*)
        echo "package version contains unsupported badge characters: $version" >&2
        exit 2
        ;;
esac

script_dir=$(CDPATH= cd -P "$(dirname "$0")" && pwd)
exec "$script_dir/make-badge.sh" version \
    --value "$version" \
    --label-width 59 \
    --output "$output"
