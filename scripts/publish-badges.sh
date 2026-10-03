#!/bin/sh
set -eu

usage() {
    echo "usage: $0 <badge-publication-root> <badge> [badge...]" >&2
    echo "badges: coverage test version" >&2
    exit 2
}

[ "$#" -ge 2 ] || usage
publication_root=$1
shift

case "$publication_root" in
    /*) ;;
    *)
        echo "badge publication root must be an absolute path: $publication_root" >&2
        exit 2
        ;;
esac

if [ ! -d "$publication_root" ] || [ ! -w "$publication_root" ]; then
    echo "badge publication root must be an existing writable directory: $publication_root" >&2
    exit 2
fi

selected=' '
for badge in "$@"; do
    case "$badge" in
        coverage|test|version) ;;
        *)
            echo "unsupported badge: $badge" >&2
            exit 2
            ;;
    esac
    case "$selected" in
        *" $badge "*)
            echo "badge selected more than once: $badge" >&2
            exit 2
            ;;
    esac
    selected="$selected$badge "

    source="coverage/$badge.svg"
    if [ ! -f "$source" ] || [ ! -s "$source" ] || [ -L "$source" ]; then
        echo "required badge must be a regular nonempty file, not a symlink: $source" >&2
        exit 2
    fi
done

owner_root="$publication_root/idle-org"
project_root="$owner_root/rust_realease"
badge_root="$project_root/main"

for directory in "$owner_root" "$project_root"; do
    if [ -L "$directory" ] || { [ -e "$directory" ] && [ ! -d "$directory" ]; }; then
        echo "badge destination component is not a directory: $directory" >&2
        exit 2
    fi
    mkdir -p "$directory"
    chmod 0755 "$directory"
done

if [ -L "$badge_root" ]; then
    unlink "$badge_root"
elif [ -e "$badge_root" ] && [ ! -d "$badge_root" ]; then
    echo "badge destination is not a directory: $badge_root" >&2
    exit 2
fi
mkdir -p "$badge_root"
chmod 0755 "$badge_root"

temporary=
trap 'if [ -n "$temporary" ]; then rm -f "$temporary"; fi' EXIT HUP INT TERM
for badge in "$@"; do
    temporary=$(mktemp "$badge_root/.$badge.svg.XXXXXX")
    cp "coverage/$badge.svg" "$temporary"
    chmod 0644 "$temporary"
    mv -f "$temporary" "$badge_root/$badge.svg"
    temporary=
    echo "published $badge badge to $badge_root/$badge.svg"
done
trap - EXIT HUP INT TERM
