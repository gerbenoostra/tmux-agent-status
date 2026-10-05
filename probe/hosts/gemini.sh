# Gemini CLI probe support for probe-lifecycle.sh.
#
# Gemini has no drop-in hook file; hooks live only inside settings.json, so
# the probe writes a project-scope <workspace>/.gemini/settings.json. Current
# Gemini hook definitions nest {hooks: [{type, command}]} under each event
# name inside the top-level "hooks" object. The hook log lives at
# $SCRATCH/hooks.jsonl.
#
# Gemini parses hook stdout as JSON on exit 0 and forbids non-JSON output,
# so every command logs the event and then prints `{}`. To slow one event's
# hook - for the ordering scenarios - set TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds>
# in the harness environment, where <EVENT> is the event name upper-cased.

# shellcheck source=/dev/null
. "$PROBE_DIR/hook-command.sh"

probe_binary() {
    printf 'gemini\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    mkdir -p "$ws/.gemini"

    events="SessionStart SessionEnd BeforeAgent AfterAgent BeforeModel
        AfterModel BeforeToolSelection BeforeTool AfterTool PreCompress
        Notification"

    hooks='{}'
    for event in $events; do
        command=$(probe_hook_command "$event" "$log" json)
        hooks=$(jq --arg e "$event" --arg c "$command" \
            '. + {($e): [{hooks: [{type: "command", command: $c}]}]}' \
            <<<"$hooks")
    done
    jq -n --argjson hooks "$hooks" '{hooks: $hooks}' \
        >"$ws/.gemini/settings.json"
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and a resumed session can be driven by hand.
    printf 'cd %q && gemini; exec bash' "$1/workspace"
}

probe_scenarios() {
    cat <<'EOF'

Gemini scenarios (one fresh probe each unless the log is enough).

Gemini has no subagent start/end event - BeforeAgent/AfterAgent bracket the
agent loop, not individual children - so S2-S6 and S12 cannot attribute
anything to a child. Approve the workspace trust prompt if Gemini shows
one; a project settings.json may not load until the folder is trusted.

  S1  "Run the shell command `echo hello`, then stop." Record BeforeAgent,
      BeforeTool/AfterTool around the command, then AfterAgent.

  S2  Not expressible: no child start event exists. Ask for background work
      anyway ("run `sleep 4` in the background, then end your turn") and
      record the event stream - child activity can only appear as
      unattributed tool events.

  S3  Not expressible for the same reason.

  S4  Not expressible for the same reason.

  S5  Not expressible: no child ID exists in any event. Run the cancel if
      the UI offers it; record raw events.

  S6  Not expressible for the same reason.

  S7  "Run `echo marker` with whatever child mechanism Gemini offers."
      Child tool calls may surface as BeforeTool/AfterTool; record which
      fields, if any, name the child.

  S8  Quit Gemini with work running. Record SessionEnd and any event after
      exit.

  S9  Clear/compact with a session active. SessionStart is documented on
      startup, resume and /clear, and PreCompress on compaction; record
      session IDs and whether the work survives.

  S10 Abort the parent turn; record whether AfterAgent still fires or the
      turn ends silently.

  S11 "Ask me a question mid-turn." Notification is the only candidate
      blocked-on-you event; record its kind and the event order.

  S12 Not expressible: no per-child start/stop hooks exist, so a sleeping
      start hook has no Gemini equivalent. Skip.

EOF
}
