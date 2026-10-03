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
    passing) color='#4c1' ;;
    failing) color='#e05d44' ;;
    *)
        echo "test badge status must be passing or failing: $status" >&2
        exit 2
        ;;
esac

output_dir=${output%/*}
if [ "$output_dir" != "$output" ]; then
    mkdir -p "$output_dir"
fi

temporary="${output}.tmp.$$"
trap 'rm -f "$temporary"' EXIT HUP INT TERM

sed \
    -e "s/@STATUS@/${status}/g" \
    -e "s/@COLOR@/${color}/g" \
    >"$temporary" <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20" role="img" aria-label="tests: @STATUS@">
  <title>tests: @STATUS@</title>
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#fff" stop-opacity=".7"/>
    <stop offset=".1" stop-color="#aaa" stop-opacity=".1"/>
    <stop offset=".9" stop-opacity=".3"/>
    <stop offset="1" stop-opacity=".5"/>
  </linearGradient>
  <clipPath id="r"><rect width="100" height="20" rx="3"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="45" height="20" fill="#555"/>
    <rect x="45" width="55" height="20" fill="@COLOR@"/>
    <rect width="100" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" dominant-baseline="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="22.5" y="10">tests</text>
    <text x="72.5" y="10">@STATUS@</text>
  </g>
</svg>
EOF

mv "$temporary" "$output"
trap - EXIT HUP INT TERM
