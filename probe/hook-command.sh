# Shared by probe/hosts/*.sh, which source it; never run on its own.

# probe_hook_command <event> <log> [json]
#
# Print the shell command a probe hook entry runs for <event>: log-hook.sh
# appending to <log>, every path quoted for the shell the host runs hooks
# through. TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment,
# <EVENT> upper-cased, slows that event's hook for the ordering scenarios.
# With `json` the command then prints `{}`, for hosts that parse a hook's
# stdout as JSON and could read an empty reply as a decision.
probe_hook_command() {
    local event=$1 log=$2 var command delay=0
    var="TAS_PROBE_HOOK_SLEEP_$(printf '%s' "$event" | tr '[:lower:]' '[:upper:]')"
    eval "delay=\${$var:-0}"
    command=$(printf '%q %q %q' "$PROBE_DIR/log-hook.sh" "$event" "$log")
    if [ "$delay" != "0" ]; then
        command="TAS_HOOK_SLEEP=$(printf %q "$delay") $command"
    fi
    if [ "${3:-}" = json ]; then
        command="$command; printf '{}'"
    fi
    printf '%s\n' "$command"
}
