# Grok CLI probe support for probe-lifecycle.sh.
#
# Grok reads project-local hooks from <workspace>/.grok/hooks/*.json in the
# nested {hooks: {Event: [{hooks: [...]}]}} shape. Project hooks stay inert
# until the workspace is trusted inside the session with /hooks-trust - the
# scenario list says so before S1, because every scratch workspace is new.
# The hook log lives at $SCRATCH/hooks.jsonl.
#
# Grok's stdout handling is lenient, so the commands only log. To slow one
# event's hook - for the ordering scenarios - set
# TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment, where
# <EVENT> is the event name upper-cased.

probe_binary() {
    printf 'grok\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    logger="$PROBE_DIR/log-hook.sh"
    mkdir -p "$ws/.grok/hooks"

    events="SessionStart UserPromptSubmit PreToolUse PostToolUse
        PostToolUseFailure PermissionRequest Notification SubagentStart
        SubagentStop Stop SessionEnd"

    hooks='{}'
    for event in $events; do
        var="TAS_PROBE_HOOK_SLEEP_$(printf '%s' "$event" | tr '[:lower:]' '[:upper:]')"
        delay=0
        eval "delay=\${$var:-0}"
        # Hook commands run through a shell, so paths are quoted for one.
        command=$(printf '%q %q %q' "$logger" "$event" "$log")
        if [ "$delay" != "0" ]; then
            command="TAS_HOOK_SLEEP=$(printf %q "$delay") $command"
        fi
        hooks=$(jq --arg e "$event" --arg c "$command" \
            '. + {($e): [{hooks: [{type: "command", command: $c}]}]}' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '{hooks: $hooks}' \
        >"$ws/.grok/hooks/tas-probe.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'cd %q && grok --oauth --permission-mode default; exec bash' \
        "$1/workspace"
}

probe_scenarios() {
    cat <<'EOF'

Grok scenarios (one fresh probe each unless the log is enough).

Run `/hooks-trust` before S1: project hooks under .grok/hooks/ are inert
until the workspace is trusted, and every scratch workspace is new.

  S1  "Run the shell command `echo hello`, then stop."
      Approve the permission prompt. Expect PermissionRequest, the tool
      events, then Stop.

  S2  "Launch a background subagent that runs `sleep 4`, then end your turn
       now. Do not wait for it."
      Watch for SubagentStart, the early parent Stop, then SubagentStop and
      whatever the automatic parent turn emits. Compare the IDs the start
      and stop events carry, and the task IDs Stop's backgroundTasks lists.

  S3  Like S2 but two background children, `sleep 5` and `sleep 25`. The
      second finishes during the first's automatic parent turn.

  S4  "Launch a background subagent that runs `sleep 3`, and in the same turn
       keep working: count from 1 to 30 slowly with a shell loop that sleeps
       1 between numbers." The child finishes while the parent turn still
      runs.

  S5  "Launch a background subagent that runs `sleep 60`, then immediately
       cancel it." Record whether SubagentStop fires, or whether a
      PostToolUse on the cancelling tool carries the child's ID.

  S6  Launch a background child (sleep 60), then kill it from the UI if Grok
      offers that. Record which stop event fires and whether a parent turn
      and Stop follow.

  S7  "Launch a background subagent that runs `echo marker` and exits."
      Record which of the child's tool, permission and question events
      reach the parent hooks and which fields name the child.

  S8  Launch a background child (sleep 60), then quit Grok while it runs.
      Record SessionEnd, whether the child dies, and any event after exit.

  S9  Launch a background child (sleep 60), then /clear, /compact and resume
      in a fresh probe. Record the session IDs and whether the work
      survives.

  S10 Launch a background child (sleep 60), then abort the parent turn.
      Record whether Stop, PostToolUseFailure or nothing fires and what
      happens to the work.

  S11 "Launch a background subagent that runs `sleep 5`, then ask me a
       question." Let the work finish, then answer. Record whether the
      question surfaces as PermissionRequest or Notification and the event
      order across it.

  S12 Relaunch with TAS_PROBE_HOOK_SLEEP_SUBAGENTSTART=3:
      (a) a foreground child that does no work, and
      (b) a background child the parent cancels immediately.
      Compare each stop hook's ts_enter against the start hook's ts_exit.

EOF
}
