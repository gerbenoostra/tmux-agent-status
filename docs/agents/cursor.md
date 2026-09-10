# Cursor

Shape A agent with a drop-in JSON hook file. Cursor reads `~/.cursor/hooks.json`
or `<project>/.cursor/hooks.json` and auto-reloads on change. The hook system is
bidirectional JSON over stdio.

## Supported states

| State | Cursor event | Command | Notes |
| --- | --- | --- | --- |
| reset | `sessionStart` | `tmux-agent-status reset` | |
| working | `beforeSubmitPrompt`, `postToolUseFailure` | `tmux-agent-status set working` | also fires on tool execution |
| done | `stop` | `tmux-agent-status set done` | |
| waiting | — | — | no confirmed dedicated waiting event |
| error | — | — | `postToolUseFailure` is a tool result, not a turn abort |
| finish | `sessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

`subagentStart` and `subagentStop` are deliberately **not** mapped to `done`; a
subagent stopping does not end the parent turn.

## Drop-in file

Copy `share/agents/cursor/hooks.json` to `~/.cursor/hooks.json` for a user-wide hook,
or to `<project>/.cursor/hooks.json` for a project-local hook.

```sh
cp /path/to/share/agents/cursor/hooks.json ~/.cursor/hooks.json
```

## Prove it fired

Start a Cursor agent session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After submitting a prompt it should read `working`; after the turn stops it should
read `done` or be empty.

## Quirks

- **Verification is incomplete.** The Cursor hook documentation does not expose the
  full event vocabulary or payload schema. The shipped file uses only the events
  that clearly do not block Cursor's own flow. Events that ask for a decision
  (such as `beforeShellExecution`) are intentionally omitted so the status hook
  never denies a tool call.
- **Stdout is parsed as JSON.** Every hook entry uses `--json` so Cursor's parser
  does not choke on empty stdout; `--json` prints `{}` on success.
- **No confirmed `waiting` event.** If a future Cursor release adds a
  blocked-on-user hook, add it to `hooks.json` and update this page.
- **`postToolUseFailure` is not `error`.** A failed tool call is an ordinary
  part of a turn - a grep that matched nothing, a test run that failed - and the
  turn is still running, so it maps to `working`. Mapping it to `error` would
  paint ❗ and ring the bell several times during a healthy turn. Cursor
  publishes no turn-abort event, so its `error` column stays empty.
- **`TMUX_PANE` inheritance is undocumented.** Use `--pane #{pane_id}` or set
  `TMUX_AGENT_STATUS_PANE` if the hook runner is not a child of the pane.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
