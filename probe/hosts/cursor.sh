# Cursor probe support for probe-lifecycle.sh.
#
# Cursor reads project-local hooks from <workspace>/.cursor/hooks.json in its
# {hooks: {event: [{command}]}} shape. The hook log lives at
# $SCRATCH/hooks.jsonl.
#
# Cursor parses hook stdout as JSON, so every command logs the event and
# then prints `{}`. To slow one event's hook - for the ordering scenarios -
# set TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment,
# where <EVENT> is the event name upper-cased.

probe_binary() {
    printf 'cursor-agent\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    logger="$PROBE_DIR/log-hook.sh"
    # Cursor only discovers repo-scoped hooks from a Git worktree root.
    git -C "$ws" init --quiet
    mkdir -p "$ws/.cursor"

    events="sessionStart sessionEnd beforeSubmitPrompt subagentStart
        subagentStop postToolUseFailure stop"

    hooks='{}'
    for event in $events; do
        var="TAS_PROBE_HOOK_SLEEP_$(printf '%s' "$event" | tr '[:lower:]' '[:upper:]')"
        delay=0
        eval "delay=\${$var:-0}"
        # Hook commands run through a shell, so paths are quoted for one. The
        # trailing `{}` keeps the stdout JSON parser fed.
        command="$(printf '%q %q %q' "$logger" "$event" "$log"); printf '{}'"
        if [ "$delay" != "0" ]; then
            command="TAS_HOOK_SLEEP=$(printf %q "$delay") $command"
        fi
        hooks=$(jq --arg e "$event" --arg c "$command" \
            '. + {($e): [{command: $c}]}' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '{hooks: $hooks}' >"$ws/.cursor/hooks.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'cd %q && cursor-agent; exec bash' "$1/workspace"
}

probe_scenarios() {
    cat <<'EOF'

Cursor scenarios (one fresh probe each unless the log is enough).

Cursor documents paired subagentStart/subagentStop events, but reports that
background children never emit subagentStop are unsettled - a missing event
is a result, not an assumption, so record what fires before claiming the
pair works. No permission or question event is registered.

  S1  "Run the shell command `echo hello`, then stop." There is no
      permission hook; record beforeSubmitPrompt, any tool-adjacent events,
      then stop.

  S2  "Launch a background subagent that runs `sleep 4`, then end your turn
       now. Do not wait for it."
      Watch for subagentStart, the early parent stop, then whether
      subagentStop arrives and whether a wake turn follows.

  S3  Like S2 but two background children, `sleep 5` and `sleep 25`. The
      second finishes during the first's automatic parent turn.

  S4  "Launch a background subagent that runs `sleep 3`, and in the same
       turn keep working: count from 1 to 30 slowly with a shell loop that
       sleeps 1 between numbers." The child finishes while the parent turn
      still runs.

  S5  "Launch a background subagent that runs `sleep 60`, then immediately
       cancel it." Record which event, if any, carries the cancelled
      child's ID.

  S6  Launch a background child (sleep 60), then kill it from the UI if
      Cursor offers that. Record which stop event fires and whether a
      parent turn follows.

  S7  "Launch a background subagent that runs `echo marker` and exits."
      postToolUseFailure is the only tool event registered; record which
      child signals reach the hooks and which fields name the child.

  S8  Launch a background child (sleep 60), then quit Cursor while it runs.
      Record sessionEnd, whether the child dies, and any event after exit.

  S9  Launch a background child (sleep 60), then clear, compact and resume
      in a fresh probe. Record the session IDs and whether the work
      survives.

  S10 Launch a background child (sleep 60), then abort the parent turn.
      Record whether stop, postToolUseFailure or nothing fires and what
      happens to the work.

  S11 "Launch a background subagent that runs `sleep 5`, then ask me a
       question." No question event is registered; record the raw event
      order around the prompt.

  S12 Relaunch with TAS_PROBE_HOOK_SLEEP_SUBAGENTSTART=3:
      (a) a foreground child that does no work, and
      (b) a background child the parent cancels immediately.
      Compare each stop hook's ts_enter against the start hook's ts_exit -
      only meaningful if subagentStart actually fires.

EOF
}
