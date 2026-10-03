#!/bin/sh
set -eu

usage() {
    echo "usage: $0 <line-coverage-percent> <output.svg>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
percent=$1
output=$2

script_dir=$(CDPATH= cd -P "$(dirname "$0")" && pwd)
exec "$script_dir/make-badge.sh" coverage \
    --value "$percent" \
    --label-width 75 \
    --value-width 37 \
    --output "$output"
