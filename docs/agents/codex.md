# Codex CLI

Shape A agent with a drop-in JSON hook file. Codex reads `hooks.json` from
`~/.codex/hooks.json` or `<repo>/.codex/hooks.json`; multiple sources merge.

## Supported states

| State | Codex event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` (`startup\|resume\|clear\|compact`) | `tmux-agent-status reset` | |
| working | `UserPromptSubmit`, `PostToolUse` | `tmux-agent-status set working` | |
| done | `Stop` | `tmux-agent-status set done` | |
| waiting | `PermissionRequest` | `tmux-agent-status set waiting` | permission prompt |
| error | — | — | no published error event; inferred from missing clean `Stop` |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

`SubagentStart` and `SubagentStop` are deliberately **not** mapped to `done`; a
subagent stopping does not end the parent turn.

## Drop-in file

Copy `share/agents/codex/hooks.json` to `~/.codex/hooks.json` for a user-wide hook,
or to `<repo>/.codex/hooks.json` for a project-local hook. The `hooks` object merges
with any existing one; do not duplicate the top-level `hooks` key.

```sh
cp /path/to/share/agents/codex/hooks.json ~/.codex/hooks.json
```

## Prove it fired

Start a Codex session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After submitting a prompt it should read `working`; after the turn stops it should
read `done` or be empty.

## Quirks

- **Hooks are on by default.** Disable with `[features] hooks = false`.
- **Stdout is parsed as JSON.** Codex reads a hook's stdout when it exits 0, so
  every entry in the drop-in file appends `printf '{}\n'`. The status commands
  themselves write nothing to stdout; the wrapper is what keeps an empty stdout
  from reaching the parser. `PermissionRequest` is the one that matters most: a
  permission hook that returns nothing may be read as a decision.
- **`SessionEnd` is delayed up to 30 minutes on idle disconnect.** A crashed session
  can leave the glyph stranded for a while; this is a known Codex limit.
- **No `error` event.** A turn that aborts leaves `working` until `SessionEnd`
  fires, or until you start a new session in that pane.
- **`TMUX_PANE` inheritance is undocumented.** Use `--pane #{pane_id}` or set
  `TMUX_AGENT_STATUS_PANE` if the hook runner is not a child of the pane.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
