# Claude Code probe support for probe-lifecycle.sh.
#
# The probe runs Claude with CLAUDE_CONFIG_DIR pointed at the scratch
# directory, so its settings, credentials copy, session history and trust
# state are all disposable. The hook log lives at $SCRATCH/hooks.jsonl.
#
# Every hook event Claude Code documents gets a command hook that appends to
# the log. To slow one event's hook - for the ordering scenarios - set
# TAS_PROBE_HOOK_SLEEP_<EVENT>=<seconds> in the harness environment, where
# <EVENT> is the event name upper-cased (for example
# TAS_PROBE_HOOK_SLEEP_SUBAGENTSTART=3).

# shellcheck source=/dev/null
. "$PROBE_DIR/hook-command.sh"

probe_binary() {
    printf 'claude\n'
}

probe_install_hooks() {
    scratch=$1
    cfg="$scratch/claude-config"
    ws="$scratch/workspace"
    log="$scratch/hooks.jsonl"
    mkdir -p "$cfg"

    # Every hook event the host documents, command form.
    events="SessionStart Setup UserPromptSubmit UserPromptExpansion
        PreToolUse PermissionRequest PermissionDenied PostToolUse
        PostToolUseFailure PostToolBatch Notification MessageDisplay
        SubagentStart SubagentStop TaskCreated TaskCompleted Stop StopFailure
        TeammateIdle InstructionsLoaded ConfigChange CwdChanged
        DirectoryAdded FileChanged WorktreeCreate WorktreeRemove PreCompact PostCompact
        PreModelSwitch PostModelSwitch Elicitation ElicitationResult
        SessionEnd"

    hooks='{}'
    for event in $events; do
        command=$(probe_hook_command "$event" "$log")
        # FileChanged's matcher is a `|`-list of literal basenames to watch,
        # not a pattern, and the file must exist at session start; the
        # workspace's `probe_file_changed` is created below - modify it to
        # fire the event (on 2.1.287 the watcher arms roughly ten seconds
        # after session start). The others match everything.
        case "$event" in
            FileChanged)
                hooks=$(jq --arg e "$event" --arg c "$command" \
                    '. + {($e): [{matcher: "probe_file_changed", hooks: [{type: "command", command: $c}]}]}' \
                    <<<"$hooks")
                ;;
            # The worktree events are not passive - a configured hook
            # replaces the host's default worktree handling, so they get a
            # responder that logs and still answers.
            WorktreeCreate | WorktreeRemove)
                command=$(printf '%q %q %q %q' "$PROBE_DIR/worktree-hook.sh" "$event" \
                    "$log" "$scratch/worktrees")
                hooks=$(jq --arg e "$event" --arg c "$command" \
                    '. + {($e): [{hooks: [{type: "command", command: $c}]}]}' \
                    <<<"$hooks")
                ;;
            *)
                hooks=$(jq --arg e "$event" --arg c "$command" \
                    '. + {($e): [{hooks: [{type: "command", command: $c}]}]}' \
                    <<<"$hooks")
                ;;
        esac
    done
    jq -n --argjson hooks "$hooks" '{hooks: $hooks}' >"$cfg/settings.json"
    : >"$ws/probe_file_changed"

    # Credentials: on Linux Claude keeps OAuth material in
    # ~/.claude/.credentials.json; on macOS in the login keychain. An
    # ANTHROPIC_API_KEY in the environment works too and needs no copy.
    if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
        echo "probe: using ANTHROPIC_API_KEY from the environment" >&2
    elif [ -f "$HOME/.claude/.credentials.json" ]; then
        cp "$HOME/.claude/.credentials.json" "$cfg/.credentials.json"
        chmod 600 "$cfg/.credentials.json"
    elif command -v security >/dev/null 2>&1 &&
        security find-generic-password -s "Claude Code-credentials" -w \
            >"$cfg/.credentials.json" 2>/dev/null; then
        chmod 600 "$cfg/.credentials.json"
    else
        rm -f "$cfg/.credentials.json"
        echo "probe: no Claude credentials found for the scratch config;" >&2
        echo "probe: run /login inside the probe session before driving it" >&2
    fi

    # .claude.json: onboarding and the workspace trust dialog answered in
    # advance, plus the account metadata Claude expects beside the OAuth blob.
    ws_real=$(cd "$ws" && pwd -P)
    if [ -f "$HOME/.claude.json" ]; then
        jq --arg ws "$ws_real" '{
                hasCompletedOnboarding: true,
                oauthAccount, userID, machineID, installMethod,
                lastOnboardingVersion,
                projects: {($ws): {
                    hasTrustDialogAccepted: true,
                    hasClaudeMdExternalIncludesApproved: true,
                    hasClaudeMdExternalIncludesWarningShown: true
                }}
            }' "$HOME/.claude.json" >"$cfg/.claude.json"
    else
        jq -n --arg ws "$ws_real" '{
                hasCompletedOnboarding: true,
                projects: {($ws): {hasTrustDialogAccepted: true}}
            }' >"$cfg/.claude.json"
    fi
}

probe_launch_command() {
    # The pane falls back to a shell when the host exits, so the probe server
    # survives session end and `claude --resume` can be driven by hand.
    printf 'cd %q && env CLAUDE_CONFIG_DIR=%q claude --model %q; exec bash' \
        "$1/workspace" "$1/claude-config" "${TAS_PROBE_MODEL:-haiku}"
}

probe_scenarios() {
    cat <<'EOF'

Claude Code scenarios (one fresh probe each unless the log is enough):

  S1  "Run the Bash command `echo hello`, then stop."
      Approve the permission prompt. Record the permission-related events.

  S2  "Launch a subagent with the Task tool and run_in_background set to true
       that runs `sleep 4`, then ends. Do not wait for it; end your turn now."
      Watch for SubagentStart, the early parent Stop, then SubagentStop and
      the automatic turn's Stop.

  S3  Like S2 but two background agents, `sleep 5` and `sleep 25`. The second
      finishes during the first's automatic parent turn.

  S4  "Launch a background Task that runs `sleep 3`, and in the same turn keep
       working: count from 1 to 30 slowly with a Bash loop that sleeps 1
       between numbers." The child finishes while the parent turn still runs.

  S5  "Launch a background Task that runs `sleep 60`, then immediately cancel
       it with TaskStop." Compare tool_input.tool_response task_id with the
       SubagentStart agent_id.

  S6  Launch a background Task (sleep 60), then kill it from the UI (open the
      task list with ctrl-t or /tasks and kill it). Record which stop event
      fires and whether a parent turn and Stop follow.

  S7  "Launch a background Task that runs `echo marker` with Bash and exits."
      Check which of the child's tool/permission events reach the parent
      hooks and which fields name the child.

  S8  Launch a background Task (sleep 60), then quit the host (ctrl-d or
      /exit) while it runs. Record SessionEnd, whether the child dies, and
      any event after exit.

  S9  Launch a background Task (sleep 60), then in the same session run
      /compact and /clear, then exit and `claude --resume <session-id>` in a
      fresh probe pane. Record session IDs and whether the work survives.

  S10 Launch a background Task (sleep 60), then abort the parent turn (Esc
      while it runs, or make the model call fail). Record whether StopFailure
      or Stop fires and what happens to the work.

  S11 "Launch a background Task that runs `sleep 5`, then ask me a question
       with AskUserQuestion." Let the work finish, then answer. Record the
      event order across the open question.

  S12 Relaunch with TAS_PROBE_HOOK_SLEEP_SUBAGENTSTART=3:
      (a) a foreground Task that does no work, and
      (b) a background Task the parent cancels with TaskStop immediately.
      Compare each stop hook's ts_enter against the start hook's ts_exit.

EOF
}
