#!/bin/sh
set -eu

usage() {
    echo "usage: $0 <passing|failing> <output.svg>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
status=$1
output=$2
case "$status" in
    passing|failing) ;;
    *)
        echo "test badge status must be passing or failing: $status" >&2
        exit 2
        ;;
esac

script_dir=$(CDPATH= cd -P "$(dirname "$0")" && pwd)
exec "$script_dir/make-badge.sh" test \
    --value "$status" \
    --label-width 45 \
    --value-width 55 \
    --output "$output"
