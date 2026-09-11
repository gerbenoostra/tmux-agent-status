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
| waiting | `Notification` (`permission_prompt\|idle_prompt`) | `tmux-agent-status set waiting` | the idle nag repeats while Droid is blocked |
| error | — | — | no published error event; a cancelled turn emits `Notification` instead of `Stop` |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

`SubagentStop` is deliberately **not** mapped to `done`; a subagent stopping does
not end the parent turn.

## Drop-in file

Copy [`share/agents/droid/hooks.json`](../../share/agents/droid/hooks.json) to `~/.factory/hooks.json` for a user-wide hook,
or to `.factory/hooks.json` for a project-local hook. Droid's file holds the event
names at the top level, with no wrapping `hooks` key; if you already have a
`hooks.json`, merge these event entries into it at the top level. Nesting them
under a `hooks` key gives a config Droid ignores without an error.

```sh
cp /path/to/share/agents/droid/hooks.json ~/.factory/hooks.json
```


## Quirks

- **Hooks are on by default.** No enable step.
- **No published `error` event.** A cancelled/aborted turn emits `Notification`
  instead of `Stop`, so it lands on `waiting` (💬) rather than leaving 🤖
  stranded. The drop-in maps `permission_prompt` and `idle_prompt` and no other
  notification type; it never maps a notification to `error`.
- **The idle nag repeats.** `idle_prompt` fires each time Droid is waiting on
  you, including immediately after you cancel a turn, which is what makes
  `waiting` useful here at all.
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
