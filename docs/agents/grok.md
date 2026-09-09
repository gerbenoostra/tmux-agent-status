# Grok CLI

Shape A agent with a drop-in JSON hooks directory. Grok Build supports both a
Claude-compatible `~/.grok/hooks/*.json` directory and an inline TOML `[[hooks]]`
table; the drop-in directory is the self-contained route.

## Supported states

| State | Grok event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` | `tmux-agent-status reset` | |
| working | `PreToolUse`, `PostToolUse` | `tmux-agent-status set working` | also refreshes the glyph |
| done | `Stop` | `tmux-agent-status finish` | |
| waiting | — | — | no blocked-on-user event documented |
| error | — | — | inferred from `PostToolUse` `tool_status`; no published error event |

## Drop-in file

Copy `share/agents/grok/tmux-agent-status.json` to `~/.grok/hooks/tmux-agent-status.json`
for a user-wide hook, or to `<project>/.grok/hooks/tmux-agent-status.json` for a
project hook. Project hooks require running `/hooks-trust` the first time the
project is opened.

```sh
mkdir -p ~/.grok/hooks
cp /path/to/share/agents/grok/tmux-agent-status.json ~/.grok/hooks/
```

## Prove it fired

Start a Grok session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After the first tool use it should read `working`; after a clean stop it should
read `done` or be empty.

## Quirks

- **No `error` event.** A failed tool call can be inferred from `PostToolUse`
  payload (`tool_status`/`toolResult`), but the drop-in file stays conservative
  and maps only the confirmed lifecycle events.
- **No `waiting` event.** Grok does not publish a blocked-on-user hook, so
  permission prompts or idle nags cannot drive the glyph.
- **Subagents.** There is no dedicated subagent stop event. A `spawn_subagent`
  tool call is visible only as a `PreToolUse`/`PostToolUse` with `toolName`
  `spawn_subagent`; the parent's own `Stop` is the correct `done` signal.
- **Project hook trust.** Project-scoped hooks are disabled until the user runs
  `/hooks-trust` in that project.
- **Stdout is lenient.** Grok treats non-JSON stdout as informational for
  passive hook types, so the commands can print nothing safely.
- **`TMUX_PANE` inheritance is undocumented.** Use `--pane #{pane_id}` or set
  `TMUX_AGENT_STATUS_PANE` if the hook runner is not a child of the pane.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
