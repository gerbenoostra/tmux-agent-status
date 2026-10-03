#!/usr/bin/env bash
# worktree-hook.sh <WorktreeCreate|WorktreeRemove> <log-file> <dir-root>
#
# Probe hook for the two worktree events, which unlike every other event are
# not passive: a configured WorktreeCreate hook replaces the host's default
# worktree handling, and a hook that returns nothing makes the spawning call
# fail with "returned no worktree path". So besides logging, this hook
# creates a plain directory under <dir-root> named after the payload's `name`
# and prints it as the worktree path; WorktreeRemove deletes that directory.

event=$1
log=$2
root=$3
here=$(cd "$(dirname "$0")" && pwd)

payload=$(cat)
printf '%s' "$payload" | "$here/log-hook.sh" "$event" "$log"

name=$(printf '%s' "$payload" | jq -r '.name // empty' 2>/dev/null)
# Only a simple name is honoured; anything else is no directory of ours.
case "$name" in
    '' | *[!A-Za-z0-9_-]*) exit 0 ;;
esac

case "$event" in
    WorktreeCreate)
        mkdir -p "$root/$name"
        printf '%s' "$root/$name"
        ;;
    WorktreeRemove)
        rm -rf "${root:?}/$name"
        ;;
esac
exit 0
