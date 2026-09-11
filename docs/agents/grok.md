# Grok CLI

Shape A agent with a drop-in JSON hooks directory. Grok Build reads a
Claude-compatible `~/.grok/hooks/*.json` directory (or `<project>/.grok/hooks/*.json`
for a project scope), confirmed against a real install (`grok inspect` shows the
drop-in loaded). xAI's own docs (`docs.x.ai/build/features/hooks`) document no inline
TOML alternative; an earlier version of this page claimed one existed and was wrong.

## Supported states

| State | Grok event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` | `tmux-agent-status reset` | |
| working | `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `SubagentStart`, `SubagentStop` | `tmux-agent-status set working` | a failed tool call is still a running turn |
| done | `Stop` | `tmux-agent-status set done` | rings the bell; `finish` never does |
| waiting | — | — | `Notification` is documented but not described as a blocked-on-user signal |
| error | — | — | `StopFailure` is documented as a turn ending with an API error; left unmapped until verified |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

## Drop-in file

Copy [`share/agents/grok/tmux-agent-status.json`](../../share/agents/grok/tmux-agent-status.json) to `~/.grok/hooks/tmux-agent-status.json`
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

## Quirks

- **No `error` mapping yet.** `StopFailure` is documented as a turn ending with
  an API error, but we have not verified it in a real session, so `error` is left
  unmapped rather than ship a guess. `PostToolUseFailure` is mapped to `working`
  because a failed tool call is an ordinary part of a turn that is still running,
  not a turn abort.
- **No `waiting` mapping.** `Notification` is documented as "the agent sends a
  notification", but xAI does not say what triggers it or whether it repeats, so
  it is not mapped to `waiting`.
- **Subagent stop is not `done`.** `SubagentStart` and `SubagentStop` are
  dedicated events; both map to `working`. The parent turn's own `Stop` is the
  `done` signal.
- **Project hook trust.** Project-scoped hooks require `/hooks-trust` the first
  time the project is opened.
- **Stdout is lenient.** Non-JSON stdout is informational for passive events.
