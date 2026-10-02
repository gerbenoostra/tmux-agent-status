#!/usr/bin/env bash
# log-hook.sh <event> <log-file>
#
# Lifecycle-probe hook: append one JSONL record for the event to <log-file>.
# The record holds the event name, the hook's entry and exit wall-clock
# milliseconds, and the raw stdin payload as a string. Entry is measured
# before stdin is read, exit after, so a sleeping hook (TAS_HOOK_SLEEP)
# shows up in the gap between them.
#
# A hook that fails can block or confuse the host under test, so this
# script ends with exit 0 no matter what went wrong.

event=$1
log=$2

now_ms() {
    perl -MTime::HiRes=time -e 'printf "%d", time() * 1000' 2>/dev/null \
        || python3 -c 'import time; print(int(time.time() * 1000))' 2>/dev/null \
        || { s=$(date +%s); echo "${s}000"; }
}

entered=$(now_ms)
payload=$(cat)
# A probe can slow one event's hook to expose ordering guarantees around it
# (for example whether a stop hook may run before the start hook exits).
[ "${TAS_HOOK_SLEEP:-0}" != "0" ] && sleep "$TAS_HOOK_SLEEP"
exited=$(now_ms)

record=$(jq -cn \
    --arg event "$event" \
    --argjson entered "$entered" \
    --argjson exited "$exited" \
    --arg stdin "$payload" \
    '{event: $event, ts_enter: $entered, ts_exit: $exited, stdin: $stdin}')

# Concurrent hooks append to one log. A single O_APPEND write is atomic only
# below a size limit payloads can exceed, so serialise the append on a mkdir
# lock. A holder that dies mid-write yields after a few seconds.
lock="$log.lock"
tries=0
while ! mkdir "$lock" 2>/dev/null; do
    tries=$((tries + 1))
    [ "$tries" -gt 200 ] && break
    sleep 0.05
done
printf '%s\n' "$record" >> "$log"
rmdir "$lock" 2>/dev/null

exit 0
