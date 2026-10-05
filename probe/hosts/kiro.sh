# Kiro probe support for probe-lifecycle.sh.
#
# Kiro 2.x embeds hooks in project-local agent configuration. The probe writes
# $SCRATCH/workspace/.kiro/agents/tas-probe.json and launches that agent. The
# hook log lives at $SCRATCH/hooks.jsonl.
#
# Kiro judges hooks by exit code and feeds stdout to the agent as text, so
# the commands only log. To slow one trigger's hook - for the ordering
# scenarios - set TAS_PROBE_HOOK_SLEEP_<TRIGGER>=<seconds> in the harness
# environment, where <TRIGGER> is the trigger name upper-cased.

# shellcheck source=/dev/null
. "$PROBE_DIR/hook-command.sh"

probe_binary() {
    printf 'kiro-cli\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    mkdir -p "$ws/.kiro/agents"

    events="agentSpawn userPromptSubmit preToolUse postToolUse stop"

    hooks='{}'
    for event in $events; do
        command=$(probe_hook_command "$event" "$log")
        hooks=$(jq --arg e "$event" --arg c "$command" \
            '. + {($e): [
                if ($e == "preToolUse" or $e == "postToolUse")
                then {command: $c, matcher: ".*"}
                else {command: $c}
                end
            ]}' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '{
        name: "tas-probe",
        description: "Disposable lifecycle probe",
        prompt: "You are running a disposable lifecycle probe.",
        tools: ["execute_bash"],
        hooks: $hooks
    }' >"$ws/.kiro/agents/tas-probe.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'cd %q && kiro-cli chat --agent tas-probe --model qwen3-coder-next; exec bash' \
        "$1/workspace"
}

probe_scenarios() {
    cat <<'EOF'

Kiro scenarios (one fresh probe each unless the log is enough).

Kiro's native subagents are synchronous - the parent waits for all children -
and the trigger table has no per-child pair, so the outliving-parent scenarios
S2-S6, S8 and the ordering scenario S12 have nothing to observe. The
turn-level scenarios still apply.

  S1  "Run the shell command `echo hello`, then stop." Approve any prompts
      in the TUI; record preToolUse/postToolUse then stop.

  S2  Not expressible: children are synchronous and there is no per-child
      start trigger. A "background" request either blocks the turn or does
      not spawn; record what happens.

  S3  Not expressible for the same reason.

  S4  Not expressible for the same reason.

  S5  Not expressible: no child ID exists in any trigger. Run the cancel
      only if the TUI offers it; record raw events.

  S6  Not expressible for the same reason.

  S7  "Run a subagent that runs `echo marker` and exits" - if the TUI
      offers one, its tool events may arrive as ordinary
      preToolUse/postToolUse; record which fields, if any, name the child.

  S8  Not expressible: no session-end trigger exists. Quit Kiro with work
      running and record what, if anything, logs.

  S9  Record agentSpawn across a fresh launch and a resume; there is no
      compact or clear trigger.

  S10 Abort a running turn; record whether stop fires or the turn ends
      silently.

  S11 "Ask me a question mid-turn." No question trigger is documented;
      record the event order around the prompt.

  S12 Not expressible: no per-child start/stop hooks exist, so a sleeping
      start hook has no Kiro equivalent. Skip.

EOF
}
