# Mistral Vibe probe support for probe-lifecycle.sh.
#
# Vibe reads project-local hooks from <workspace>/.vibe/hooks.toml, which only
# loads while the launch trusts the working directory - hence --trust rather
# than an edit to the user's trusted_folders.toml. Vibe knows exactly three
# hook types, so each [[hooks]] entry is strict: the logger writes nothing to
# stdout and exits 0, which stays in the passthrough lane, and anything that
# did reach stdout would surface as a denial instead of passing silently.
# The hook log lives at $SCRATCH/hooks.jsonl.
#
# To slow one hook type - for the ordering scenarios - set
# TAS_PROBE_HOOK_SLEEP_<TYPE>=<seconds> in the harness environment, where
# <TYPE> is the type name upper-cased (for example
# TAS_PROBE_HOOK_SLEEP_POST_TOOL=3).

probe_binary() {
    printf 'vibe\n'
}

probe_install_hooks() {
    scratch=$1
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    logger="$PROBE_DIR/log-hook.sh"
    out="$ws/.vibe/hooks.toml"
    mkdir -p "$ws/.vibe"
    : >"$out"

    for event in pre_tool post_tool post_agent; do
        var="TAS_PROBE_HOOK_SLEEP_$(printf '%s' "$event" | tr '[:lower:]' '[:upper:]')"
        delay=0
        eval "delay=\${$var:-0}"
        # Hook commands run through a shell, so paths are quoted for one. A
        # TOML basic string takes JSON's escaping for every character a path
        # can carry, so the command is quoted through jq.
        command=$(printf '%q %q %q' "$logger" "$event" "$log")
        if [ "$delay" != "0" ]; then
            command="TAS_HOOK_SLEEP=$(printf %q "$delay") $command"
        fi
        json=$(printf '%s' "$command" | jq -R .)
        {
            printf '[[hooks]]\nname = "tas-probe-%s"\ntype = "%s"\n' "$event" "$event"
            case "$event" in
                pre_tool | post_tool) printf 'match = "*"\n' ;;
            esac
            printf 'strict = true\ncommand = %s\n\n' "$json"
        } >>"$out"
    done
}

probe_launch_command() {
    # --trust covers the folder-trust gate for this session only; VIBE_HOME
    # stays unset so the user's API key and model config remain in effect.
    printf 'vibe --workdir %q --trust; exec bash' "$1/workspace"
}

probe_scenarios() {
    cat <<'EOF'

Mistral Vibe scenarios (one fresh probe each unless the log is enough).

Vibe's contract is pre_tool, post_tool and post_agent only: no session
start/end, no permission event, and no per-child lifecycle signal. A
subagent.spawn returns immediately and its ID lives inside tool_input /
tool_output, while parent_session_id is the only child marker on later tool
events - the hook payloads do not even carry the childSessionId the UI
shows. S1's permission half, S5, S6, S8, S9 and S12 therefore cannot be
expressed; where a raw sequence is still worth capturing the prompt is kept.

  S1  "Run the shell command `echo hello`, then stop." There is no
      permission hook; record pre_tool/post_tool then post_agent.

  S2  "Spawn a subagent that runs `sleep 4` with subagent.spawn, then end
       your turn without waiting for it."
      Watch pre_tool/post_tool(subagent.spawn) for the spawned ID, post_agent
      while the child may still run, and whether any later event reports the
      child finishing.

  S3  Like S2 but two spawned children, `sleep 5` and `sleep 25`. The second
      finishes during the first's wake turn, if a wake turn exists.

  S4  "Spawn a subagent that runs `sleep 3`, and in the same turn keep
       working: count from 1 to 30 slowly with a shell loop that sleeps 1
       between numbers."

  S5  Not expressible: hook payloads carry no child ID, so a model-driven
      cancel cannot be correlated with a spawn. Run "spawn a subagent that
      runs `sleep 60`, then cancel it" anyway and record raw events.

  S6  Not expressible for the same reason. Spawn a child (sleep 60) and kill
      it from the UI if Vibe offers that; record the raw sequence.

  S7  "Spawn a subagent that runs `echo marker` and exits." Child tool calls
      should surface as pre_tool/post_tool with parent_session_id set;
      record whether post_agent also fires for the child's own turn end.

  S8  Not expressible: no session-end hook type exists. Quit Vibe with a
      child running and record what, if anything, logs.

  S9  Not expressible: no session-start or compact/resume events exist.
      Record the process-level sequence only.

  S10 "Spawn a subagent that runs `sleep 60`", then abort the parent turn.
      There is no abort event; post_tool's tool_status may report
      `cancelled`. Record the sequence.

  S11 "Spawn a subagent that runs `sleep 5`, then ask me a question." The
      question is an ordinary tool call; record its pre_tool/post_tool
      order against the child's events.

  S12 Not expressible: there are no per-child start/stop hooks, so a
      sleeping start hook has no Vibe equivalent. Skip.

EOF
}
