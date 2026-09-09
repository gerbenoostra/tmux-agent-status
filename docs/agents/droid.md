# Droid (Factory)

Shape A agent with a drop-in JSON hook file. Droid reads `~/.factory/hooks.json`
(user scope) or `.factory/hooks.json` (project scope). A legacy `.factory/hooks/hooks.json`
path is still supported and auto-migrated.

## Supported states

| State | Droid event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` | `tmux-agent-status reset` | |
| working | `UserPromptSubmit`, `PostToolUse` | `tmux-agent-status set working` | |
| done | `Stop` | `tmux-agent-status set done` | |
| waiting | `Notification` (`permission_prompt`) | `tmux-agent-status set waiting` | |
| error | — | — | inferred from a cancelled turn emitting `Notification` instead of `Stop` |

`SubagentStop` is deliberately **not** mapped to `done`; a subagent stopping does
not end the parent turn.

## Drop-in file

Copy `share/agents/droid/hooks.json` to `~/.factory/hooks.json` for a user-wide hook,
or to `.factory/hooks.json` for a project-local hook. The `hooks` object merges with
any existing one.

```sh
cp /path/to/share/agents/droid/hooks.json ~/.factory/hooks.json
```

## Prove it fired

Start a Droid session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After submitting a prompt it should read `working`; after the turn stops it should
read `done` or be empty.

## Quirks

- **Hooks are on by default.** No enable step.
- **No published `error` event.** A cancelled/aborted turn emits `Notification`
  instead of `Stop`. The drop-in only maps `Notification` with matcher
  `permission_prompt` to `waiting`; it does not map all notifications to `error`.
- **`SubagentStop` only.** Droid documents a stop event for task-launched
  sub-droids but no explicit start; the parent keeps `working` regardless.
- **Stdout is JSON-aware.** Droid honors `hookSpecificOutput`/`continue` shapes;
  the status commands print nothing, which is safe.
- **`TMUX_PANE` inheritance is undocumented.** Use `--pane #{pane_id}` or set
  `TMUX_AGENT_STATUS_PANE` if the hook runner is not a child of the pane.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
