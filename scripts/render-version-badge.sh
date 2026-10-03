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

value_width=$(awk -v value="$version" 'BEGIN {
    width = length(value) * 7 + 10
    print (width < 45 ? 45 : width)
}')
badge_width=$((59 + value_width))
value_center=$(awk -v width="$value_width" 'BEGIN {
    printf "%.1f", 59 + width / 2
}')

output_dir=${output%/*}
if [ "$output_dir" != "$output" ]; then
    mkdir -p "$output_dir"
fi

temporary="${output}.tmp.$$"
trap 'rm -f "$temporary"' EXIT HUP INT TERM

sed \
    -e "s/@VERSION@/${version}/g" \
    -e "s/@BADGE_WIDTH@/${badge_width}/g" \
    -e "s/@VALUE_WIDTH@/${value_width}/g" \
    -e "s/@VALUE_CENTER@/${value_center}/g" \
    >"$temporary" <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="@BADGE_WIDTH@" height="20" role="img" aria-label="version: @VERSION@">
  <title>version: @VERSION@</title>
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#fff" stop-opacity=".7"/>
    <stop offset=".1" stop-color="#aaa" stop-opacity=".1"/>
    <stop offset=".9" stop-opacity=".3"/>
    <stop offset="1" stop-opacity=".5"/>
  </linearGradient>
  <clipPath id="r"><rect width="@BADGE_WIDTH@" height="20" rx="3"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="59" height="20" fill="#555"/>
    <rect x="59" width="@VALUE_WIDTH@" height="20" fill="#007ec6"/>
    <rect width="@BADGE_WIDTH@" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" dominant-baseline="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="29.5" y="10">version</text>
    <text x="@VALUE_CENTER@" y="10">@VERSION@</text>
  </g>
</svg>
EOF

mv "$temporary" "$output"
trap - EXIT HUP INT TERM
