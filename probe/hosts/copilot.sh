# GitHub Copilot CLI probe support for probe-lifecycle.sh.
#
# Copilot reads repo-scope hooks from <workspace>/.github/hooks/*.json in its
# version-1 shape {version: 1, hooks: {event: [{type: "command", ...}]}}.
# Repo hooks are gated behind the same folder-trust prompt every repo session
# needs, so approving that prompt is part of driving S1. The hook log lives
# at $SCRATCH/hooks.jsonl.
#
# Copilot parses hook stdout as JSON, so every command logs the event and
# then prints `{}`. To slow one event's hook - for the ordering scenarios -
# set TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment,
# where <EVENT> is the event name upper-cased (for example
# TAS_PROBE_HOOK_SLEEP_SUBAGENTSTART=3).

probe_binary() {
    printf 'copilot\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    logger="$PROBE_DIR/log-hook.sh"
    # Copilot only discovers repo-scoped hooks from a Git worktree root.
    git -C "$ws" init --quiet
    mkdir -p "$ws/.github/hooks"

    events="sessionStart sessionEnd userPromptSubmitted notification agentStop
        errorOccurred subagentStart subagentStop"

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
            '. + {($e): [{type: "command", command: $c}]}' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '{version: 1, hooks: $hooks}' \
        >"$ws/.github/hooks/tas-probe.json"
}

probe_launch_command() {
    # COPILOT_HOME stays unset so the OAuth login remains in effect; -C only
    # points the session at the scratch workspace.
    printf 'copilot -C %q --model %q; exec bash' \
        "$1/workspace" "${TAS_PROBE_MODEL:-auto}"
}

probe_scenarios() {
    cat <<'EOF'

GitHub Copilot scenarios (one fresh probe each unless the log is enough).

Copilot documents that its built-in general-purpose agent does not emit
subagentStart/subagentStop - check which agent type the model actually
spawns before reading a missing event as a negative result.

  S1  "Run the shell command `echo hello`, then stop."
      The folder-trust and permission prompts surface as notification
      (permission_prompt). Approve them; record through agentStop.

  S2  "Launch a subagent that runs `sleep 4` in the background, then end
       your turn now. Do not wait for it."
      Watch for subagentStart, the early agentStop, then subagentStop and
      whether a wake turn follows. Compare the shared agentId.

  S3  Like S2 but two background children, `sleep 5` and `sleep 25`. The
      second finishes during the first's automatic parent turn.

  S4  "Launch a background subagent that runs `sleep 3`, and in the same
       turn keep working: count from 1 to 30 slowly with a shell loop that
       sleeps 1 between numbers." The child finishes while the parent turn
      still runs.

  S5  "Launch a background subagent that runs `sleep 60`, then immediately
       cancel it." Record which event carries the cancelled child's
      agentId.

  S6  Launch a background child (sleep 60), then kill it from the UI if
      Copilot offers that. Record which stop event fires and whether a
      parent turn follows.

  S7  "Launch a background subagent that runs `echo marker` and exits."
      No per-tool events are registered here; record which child signals,
      if any, reach the hooks.

  S8  Launch a background child (sleep 60), then quit Copilot while it runs.
      Record sessionEnd, whether the child dies, and any event after exit.

  S9  Launch a background child (sleep 60), then clear, compact and resume
      in a fresh probe. Record the session IDs and whether the work
      survives.

  S10 Launch a background child (sleep 60), then abort the parent turn.
      Record whether errorOccurred or agentStop fires and what happens to
      the work.

  S11 "Launch a background subagent that runs `sleep 5`, then ask me a
       question." Let the work finish, then answer. Record the
      notification type and the event order across the open question.

  S12 Relaunch with TAS_PROBE_HOOK_SLEEP_SUBAGENTSTART=3:
      (a) a foreground child that does no work, and
      (b) a background child the parent cancels immediately.
      Compare each stop hook's ts_enter against the start hook's ts_exit.

EOF
}
