#!/bin/sh
set -eu

usage() {
    echo "usage: $0 <line-coverage-percent> <output.svg>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
percent=$1
output=$2

if [ "$percent" = "-" ]; then
    rounded=-
    color='#9f9f9f'
else
    if ! awk -v value="$percent" 'BEGIN {
        valid = value ~ /^[0-9]+([.][0-9]+)?$/ && value >= 0 && value <= 100
        exit(valid ? 0 : 1)
    }'; then
        echo "coverage percentage must be - or a number from 0 through 100: $percent" >&2
        exit 2
    fi

    rounded=$(awk -v value="$percent" 'BEGIN { printf "%.0f", value }')
    if [ "$rounded" -ge 90 ]; then
        color='#4c1'
    elif [ "$rounded" -ge 70 ]; then
        color='#dfb317'
    else
        color='#e05d44'
    fi
fi

output_dir=${output%/*}
if [ "$output_dir" != "$output" ]; then
    mkdir -p "$output_dir"
fi

temporary="${output}.tmp.$$"
trap 'rm -f "$temporary"' EXIT HUP INT TERM

sed \
    -e "s/@PERCENT@/${rounded}/g" \
    -e "s/@COLOR@/${color}/g" \
    >"$temporary" <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="112" height="20" role="img" aria-label="coverage: @PERCENT@%">
  <title>coverage: @PERCENT@%</title>
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#fff" stop-opacity=".7"/>
    <stop offset=".1" stop-color="#aaa" stop-opacity=".1"/>
    <stop offset=".9" stop-opacity=".3"/>
    <stop offset="1" stop-opacity=".5"/>
  </linearGradient>
  <clipPath id="r"><rect width="112" height="20" rx="3"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="75" height="20" fill="#555"/>
    <rect x="75" width="37" height="20" fill="@COLOR@"/>
    <rect width="112" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" dominant-baseline="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="37.5" y="10">coverage</text>
    <text x="93.5" y="10">@PERCENT@%</text>
  </g>
</svg>
EOF

mv "$temporary" "$output"
trap - EXIT HUP INT TERM
