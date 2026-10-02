#!/usr/bin/env bash
# sanitize-fixtures.sh <raw-hooks.jsonl> <outdir>
#
# Turn a probe hook log into fixture files: one JSON record per line, named
# NNN-<event-name>.json, reduced by sanitize.jq to fields an adapter could
# read. Prompts, messages, paths and tool bodies never reach the fixtures.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
raw=$1
out=$2
mkdir -p "$out"

i=0
while IFS= read -r line; do
    i=$((i + 1))
    rec=$(jq -c -f "$here/sanitize.jq" <<<"$line")
    event=$(jq -r '.event' <<<"$rec" | tr '[:upper:]' '[:lower:]')
    printf -v n '%03d' "$i"
    printf '%s\n' "$rec" >"$out/$n-$event.json"
done <"$raw"

echo "wrote $i fixtures to $out"
