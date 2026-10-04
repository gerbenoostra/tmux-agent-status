# Codex CLI probe support for probe-lifecycle.sh.
#
# Codex reads project-local hooks from <workspace>/.codex/hooks.json in the
# nested {hooks: {Event: [{hooks: [...]}]}} shape. Hook execution is gated by
# a persisted trust prompt, which the launch bypasses with Codex's own
# --dangerously-bypass-hook-trust flag - it skips hook trust only, not tool
# approvals. The hook log lives at $SCRATCH/hooks.jsonl.
#
# Codex parses hook stdout as JSON, so every command logs the event and
# then prints `{}`. To slow one event's hook - for the ordering scenarios -
# set TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment,
# where <EVENT> is the event name upper-cased.

probe_binary() {
    printf 'codex\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    logger="$PROBE_DIR/log-hook.sh"
    # Codex only discovers repo-scoped hooks from a Git worktree root.
    git -C "$ws" init --quiet
    mkdir -p "$ws/.codex"

    events="SessionStart SessionEnd UserPromptSubmit PreToolUse PostToolUse
        PermissionRequest SubagentStart SubagentStop Stop"

    hooks='{}'
    for event in $events; do
        var="TAS_PROBE_HOOK_SLEEP_$(printf '%s' "$event" | tr '[:lower:]' '[:upper:]')"
        delay=0
        eval "delay=\${$var:-0}"
        # Hook commands run through a shell, so paths are quoted for one. The
        # trailing `{}` keeps the stdout JSON parser fed - a PermissionRequest
        # hook that returns nothing may be read as a decision.
        command="$(printf '%q %q %q' "$logger" "$event" "$log"); printf '{}'"
        if [ "$delay" != "0" ]; then
            command="TAS_HOOK_SLEEP=$(printf %q "$delay") $command"
        fi
        hooks=$(jq --arg e "$event" --arg c "$command" \
            '. + {($e): [{hooks: [{type: "command", command: $c}]}]}' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '{hooks: $hooks}' >"$ws/.codex/hooks.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'cd %q && codex --dangerously-bypass-hook-trust; exec bash' \
        "$1/workspace"
}

probe_scenarios() {
    cat <<'EOF'

Codex scenarios (one fresh probe each unless the log is enough).

Codex documents SubagentStart/SubagentStop with a shared agent_id, but no
real run has observed them - a missing event is a result, not an
assumption, so record what fires before claiming the pair works.

  S1  "Run the shell command `echo hello`, then stop."
      Approve the permission prompt. Expect PermissionRequest, the tool
      events, then Stop.

  S2  "Launch a background subagent that runs `sleep 4`, then end your turn
       now. Do not wait for it."
      Watch for SubagentStart, the early parent Stop, then SubagentStop and
      whatever the automatic parent turn emits. Compare the shared
      agent_id on start and stop.

  S3  Like S2 but two background children, `sleep 5` and `sleep 25`. The
      second finishes during the first's automatic parent turn.

  S4  "Launch a background subagent that runs `sleep 3`, and in the same
       turn keep working: count from 1 to 30 slowly with a shell loop that
       sleeps 1 between numbers." The child finishes while the parent turn
      still runs.

  S5  "Launch a background subagent that runs `sleep 60`, then immediately
       cancel it." Record whether SubagentStop fires, or whether a
      PostToolUse on the cancelling tool carries the child's agent_id.

  S6  Launch a background child (sleep 60), then kill it from the UI if
      Codex offers that. Record which stop event fires and whether a parent
      turn and Stop follow.

  S7  "Launch a background subagent that runs `echo marker` and exits."
      Record which of the child's tool and permission events reach the
      parent hooks and which fields name the child.

  S8  Launch a background child (sleep 60), then quit Codex while it runs.
      Record SessionEnd - note it can arrive up to 30 minutes late on a
      disconnect - whether the child dies, and any event after exit.

  S9  Launch a background child (sleep 60), then /clear, /compact and
      resume in a fresh probe. SessionStart sources include resume, clear
      and compact; record the session IDs and whether the work survives.

  S10 Launch a background child (sleep 60), then abort the parent turn.
      Record whether Stop fires or the turn ends silently and what happens
      to the work.

  S11 "Launch a background subagent that runs `sleep 5`, then ask me a
       question." Let the work finish, then answer. Record whether the
      question surfaces as PermissionRequest and the event order across it.

  S12 Relaunch with TAS_PROBE_HOOK_SLEEP_SUBAGENTSTART=3:
      (a) a foreground child that does no work, and
      (b) a background child the parent cancels immediately.
      Compare each stop hook's ts_enter against the start hook's ts_exit.

EOF
}
