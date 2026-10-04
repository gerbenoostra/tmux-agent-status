# Kiro probe support for probe-lifecycle.sh.
#
# Project-local hook discovery is not documented for the CLI, so the probe
# keeps every byte of Kiro state under the scratch directory: it writes
# $SCRATCH/kiro-home/.kiro/hooks/tas-probe.json in the v1 shape
# {version: "v1", hooks: [{name, trigger, action: {type, command}}]} and
# launches Kiro with HOME pointed there. The hook log lives at
# $SCRATCH/hooks.jsonl.
#
# Kiro judges hooks by exit code and feeds stdout to the agent as text, so
# the commands only log. To slow one trigger's hook - for the ordering
# scenarios - set TAS_PROBE_HOOK_SLEEP_<TRIGGER>=<seconds> in the harness
# environment, where <TRIGGER> is the trigger name upper-cased.

probe_binary() {
    printf 'kiro-cli\n'
}

probe_install_hooks() {
    scratch=$1
    log="$scratch/hooks.jsonl"
    logger="$PROBE_DIR/log-hook.sh"
    mkdir -p "$scratch/kiro-home/.kiro/hooks"

    events="agentSpawn userPromptSubmit preToolUse postToolUse stop"

    hooks='[]'
    for event in $events; do
        var="TAS_PROBE_HOOK_SLEEP_$(printf '%s' "$event" | tr '[:lower:]' '[:upper:]')"
        delay=0
        eval "delay=\${$var:-0}"
        # Hook commands run through a shell, so paths are quoted for one.
        command=$(printf '%q %q %q' "$logger" "$event" "$log")
        if [ "$delay" != "0" ]; then
            command="TAS_HOOK_SLEEP=$(printf %q "$delay") $command"
        fi
        hooks=$(jq --arg e "$event" --arg n "tas-probe-$event" --arg c "$command" \
            '. + [{name: $n, trigger: $e, action: {type: "command", command: $c}}]' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '{version: "v1", hooks: $hooks}' \
        >"$scratch/kiro-home/.kiro/hooks/tas-probe.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'cd %q && env HOME=%q kiro-cli; exec bash' \
        "$1/workspace" "$1/kiro-home"
}

probe_scenarios() {
    cat <<'EOF'

Kiro scenarios (one fresh probe each unless the log is enough).

The scratch HOME means a fresh login may be needed first. Kiro's native
subagents are synchronous - the parent waits for all children - and the
trigger table has no per-child pair, so the outliving-parent scenarios
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
