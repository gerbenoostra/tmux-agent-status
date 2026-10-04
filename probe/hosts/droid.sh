# Droid (Factory) probe support for probe-lifecycle.sh.
#
# Droid reads project-local hooks from <workspace>/.factory/hooks.json, whose
# top-level object *is* the event map - nesting it under a "hooks" key is
# ignored silently. The hook log lives at $SCRATCH/hooks.jsonl.
#
# Droid's stdout handling is JSON-aware but tolerant of silence, so the
# commands only log. To slow one event's hook - for the ordering scenarios -
# set TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment,
# where <EVENT> is the event name upper-cased.

probe_binary() {
    printf 'droid\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    logger="$PROBE_DIR/log-hook.sh"
    mkdir -p "$ws/.factory"

    events="SessionStart SessionEnd UserPromptSubmit PreToolUse PostToolUse
        PermissionRequest Notification SubagentStop Stop"

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
    jq -n --argjson hooks "$hooks" '$hooks' >"$ws/.factory/hooks.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'droid --cwd %q; exec bash' "$1/workspace"
}

probe_scenarios() {
    cat <<'EOF'

Droid scenarios (one fresh probe each unless the log is enough).

Droid has no SubagentStart, and its SubagentStop carries result fields
(task_name, task_result, task_error) but no work ID - so no child can be
correlated and an unattributed stop must never be counted as a specific
child ending. S5, S6 and S12 cannot be expressed; S2-S4 and S7 record raw
sequences only.

  S1  "Run the shell command `echo hello`, then stop."
      Approve the permission prompt. Expect PermissionRequest or
      Notification(permission_prompt), the tool events, then Stop.

  S2  "Launch a background subagent that runs `sleep 4`, then end your turn
       now. Do not wait for it."
      Watch for the tool event that spawned it, the early parent Stop, and
      any SubagentStop - noting nothing ties it to a specific child.

  S3  Like S2 but two background children, `sleep 5` and `sleep 25`. Two
      SubagentStops with no IDs cannot be told apart; record the order.

  S4  "Launch a background subagent that runs `sleep 3`, and in the same
       turn keep working: count from 1 to 30 slowly with a shell loop that
       sleeps 1 between numbers."

  S5  Not expressible: no start event and no work ID exist to match a
      cancellation against. Run "launch a background subagent that runs
      `sleep 60`, then cancel it" anyway and record raw events.

  S6  Not expressible for the same reason. Kill a running child (sleep 60)
      from the UI if Droid offers that; record the raw sequence.

  S7  "Launch a background subagent that runs `echo marker` and exits."
      Record whether any child tool events reach the hooks and which
      fields name the child, if any.

  S8  Launch a background child (sleep 60), then quit Droid while it runs.
      Record SessionEnd, whether the child dies, and any event after exit.

  S9  Launch a background child (sleep 60), then clear, compact and resume
      in a fresh probe. Record the session IDs and whether the work
      survives.

  S10 Launch a background child (sleep 60), then abort the parent turn. A
      cancelled turn is documented to emit Notification instead of Stop;
      record which fires and what happens to the work.

  S11 "Launch a background subagent that runs `sleep 5`, then ask me a
       question." The blocked period should surface as
      Notification(permission_prompt) or Notification(idle_prompt); record
      the event order across it.

  S12 Not expressible: there is no child start hook to sleep, so the
      ordering pair has no Droid equivalent. Skip.

EOF
}
