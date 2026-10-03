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

# Hooks can finish out of entry order, so fixture numbering follows entry
# time, not append order; equal millisecond timestamps retain append order.
# Everything is staged, and the output directory is touched only after the
# whole log sanitizes: a failed run leaves the committed fixtures alone.
stage=$(mktemp -d "${TMPDIR:-/tmp}/tas-sanitize.XXXXXX")
trap 'rm -rf "$stage"' EXIT
sorted="$stage/sorted.jsonl"
jq -sc 'to_entries | sort_by(.value.ts_enter, .key) | .[].value' "$raw" >"$sorted"

i=0
while IFS= read -r line; do
    i=$((i + 1))
    rec=$(jq -c -f "$here/sanitize.jq" <<<"$line")
    event=$(jq -r '.event' <<<"$rec" | tr '[:upper:]' '[:lower:]')
    printf -v n '%03d' "$i"
    printf '%s\n' "$rec" >"$stage/$n-$event.json"
done <"$sorted"

# A re-run on a shorter capture must not keep the old numbering's tail:
# replace exactly the generated NNN-*.json files at the top level, and
# nothing else - expected.tsv and any other file stay.
for stale in "$out"/[0-9][0-9][0-9]-*.json; do
    [ -e "$stale" ] || continue
    rm -f "$stale"
done
for staged in "$stage"/[0-9][0-9][0-9]-*.json; do
    [ -e "$staged" ] || continue
    mv "$staged" "$out/"
done

echo "wrote $i fixtures to $out"
