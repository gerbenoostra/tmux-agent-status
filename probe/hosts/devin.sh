# Devin CLI probe support for probe-lifecycle.sh.
#
# Devin reads project-local hooks from <workspace>/.devin/hooks.v1.json, whose
# top-level object *is* the event map - there is no wrapping "hooks" key, and
# one event name Devin does not know discards the whole map, so only the eight
# documented events are written. The hook log lives at $SCRATCH/hooks.jsonl.
#
# Devin parses hook stdout as JSON, so every command logs the event and then
# prints `{}`. To slow one event's hook - for the ordering scenarios - set
# TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment, where
# <EVENT> is the event name upper-cased.

# shellcheck source=/dev/null
. "$PROBE_DIR/hook-command.sh"

probe_binary() {
    printf 'devin\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    mkdir -p "$ws/.devin"

    events="SessionStart SessionEnd UserPromptSubmit PreToolUse PostToolUse
        PermissionRequest Stop PostCompaction"

    hooks='{}'
    for event in $events; do
        command=$(probe_hook_command "$event" "$log" json)
        hooks=$(jq --arg e "$event" --arg c "$command" \
            '. + {($e): [{hooks: [{type: "command", command: $c}]}]}' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '$hooks' >"$ws/.devin/hooks.v1.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'cd %q && devin --model %q --permission-mode auto; exec bash' \
        "$1/workspace" "${TAS_PROBE_MODEL:-swe-2-medium}"
}

probe_scenarios() {
    cat <<'EOF'

Devin CLI scenarios (one fresh probe each unless the log is enough).

Devin's contract has no per-child lifecycle event: a background run_subagent
is visible only as PostToolUse carrying `agent_id` in tool_response.output,
its completion arrives as an unattributed root Stop, and the automatic wake
turn emits no UserPromptSubmit. Without a paired child start/stop signal, S5,
S6 and S12 cannot be expressed; they record the raw sequence at most.

  S1  "Run the shell command `echo hello`, then stop."
      --permission-mode auto may answer the prompt itself; record whether
      PermissionRequest fires, then the tool events and Stop.

  S2  "Launch a background subagent with run_subagent that runs `sleep 4`,
       then end your turn now."
      Watch for PostToolUse(run_subagent) with tool_input.is_background and
      the agent_id in tool_response.output, the early parent Stop, then the
      worker's unattributed Stop and the wake turn's Stop.

  S3  Like S2 but two background children, `sleep 5` and `sleep 25`. The
      second finishes during the first's automatic parent turn.

  S4  "Launch a background subagent that runs `sleep 3`, and in the same turn
       keep working: count from 1 to 30 slowly with a shell loop that sleeps
       1 between numbers." The child finishes while the parent turn still
      runs.

  S5  Not expressible: no child-stop event exists to match a cancellation
      against. Run "launch a background subagent that runs `sleep 60`, then
      cancel it" anyway and record which events the cancel produces.

  S6  Not expressible for the same reason. Launch a background child
      (sleep 60) and kill it from the UI if Devin offers that; record the
      raw sequence.

  S7  "Launch a background subagent that runs `echo marker` and exits."
      Record which of the child's tool and permission events reach the
      hooks and which fields, if any, name the child.

  S8  Launch a background child (sleep 60), then quit Devin while it runs.
      Record SessionEnd, whether the child dies, and any event after exit.

  S9  Launch a background child (sleep 60), then compact the session and
      resume it in a fresh probe. Record PostCompaction, the session IDs,
      and whether the work survives.

  S10 Launch a background child (sleep 60), then abort the parent turn.
      Devin publishes no abort event; record whether Stop or SessionEnd
      fires and what happens to the work.

  S11 "Launch a background subagent that runs `sleep 5`, then ask me a
       question." Let the work finish, then answer. The question surfaces
      as PreToolUse(ask_user_question) or PermissionRequest; record the
      event order across it.

  S12 Not expressible: there is no per-child start/stop hook pair, so a
      sleeping SubagentStart has no Devin equivalent. Skip.

EOF
}
