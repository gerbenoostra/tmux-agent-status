# Grok CLI

Shape A agent with a drop-in JSON hooks directory. Grok Build supports both a
Claude-compatible `~/.grok/hooks/*.json` directory and an inline TOML `[[hooks]]`
table; the drop-in directory is the self-contained route.

## Supported states

| State | Grok event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` | `tmux-agent-status reset` | |
| working | `PreToolUse`, `PostToolUse` | `tmux-agent-status set working` | also refreshes the glyph |
| done | `Stop` | `tmux-agent-status set done` | rings the bell; `finish` never does |
| waiting | — | — | no blocked-on-user event documented |
| error | — | — | no published error event; a failed tool call is not a turn abort |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

## Drop-in file

Copy `share/agents/grok/tmux-agent-status.json` to `~/.grok/hooks/tmux-agent-status.json`
for a user-wide hook, or to `<project>/.grok/hooks/tmux-agent-status.json` for a
project hook. Project hooks require running `/hooks-trust` the first time the
project is opened.

```sh
mkdir -p ~/.grok/hooks
cp /path/to/share/agents/grok/tmux-agent-status.json ~/.grok/hooks/
```

## The Claude Code plugin (untested)

xAI documents Grok as "fully compatible with Claude Code with zero
configuration needed", reading Claude Code marketplaces, plugins and hooks
alongside `.grok/`, so `/plugin marketplace add gerbenoostra/tmux-agent-status`
may well work here too.

**Nobody has run it.** The plugin's hook set is written in Claude's event
vocabulary and which of those names Grok honours is unverified: `StopFailure`
and `Notification` have no Grok equivalent, and `UserPromptSubmit` is not in the
event list above, so `working` might arrive only on `PostToolUse`. The drop-in
file is the route this project stands behind for Grok.

## Prove it fired

Start a Grok session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After the first tool use it should read `working`; after a clean stop it should
read `done` or be empty.

## Quirks

- **No `error` event.** `PostToolUse` carries `tool_status`/`toolResult`, but a
  failed tool call is an ordinary part of a turn that is still running, not an
  aborted turn, so it maps to `working` like any other tool event. Grok's only
  real abort signal is `stopReason` in the headless `--output-format json`
  output, which no hook sees.
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
