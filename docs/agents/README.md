# Supported agents

This table is the single source of truth for which agents drive the glyph, how,
and what is known to be missing. Blank cells link to the upstream doc or issue
that says the event does not exist.

| Agent | Shape | Drop-in file | Needs enabling | Subagent events | Multi-session per pane | `error` event | `waiting` repeats | Stdout parsed | Payload on stdin | `TMUX_PANE` inherited | Session start | Session end | Verified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [Claude Code](claude-code.md) | A (plugin) | yes, via plugin | no | no | no | `StopFailure` | `Notification` | yes | yes | yes | yes | yes | plugin API |
| [Codex CLI](codex.md) | A | yes | no | `SubagentStart`/`SubagentStop` | unknown | inferred | `PermissionRequest` | yes | yes | unknown | yes | yes | 2026-09-09 |
| [GitHub Copilot CLI](copilot.md) | A | yes | no | yes | unknown | `errorOccurred` | `notification` | yes | unknown | unknown | yes | yes | 2026-09-09 |
| [Droid](droid.md) | A | yes | no | `SubagentStop` only | unknown | inferred | `Notification` | yes | yes | unknown | yes | yes | 2026-09-09 |
| [Cursor](cursor.md) | A | yes | no | yes | unknown | no | unknown | yes | yes | unknown | yes | yes | 2026-09-09 |
| [Grok CLI](grok.md) | A | yes | project hooks need `/hooks-trust` | inferred from tool | unknown | no | unknown | lenient | yes | unknown | yes | yes | 2026-09-09 |
| [Kiro](kiro.md) | A | yes | no | no stop event | yes | no | no | no | yes | unknown | CLI: `AgentSpawn` | no | 2026-09-09 |
| [Mistral Vibe](mistral-vibe.md) | B | yes (TOML) | trusted-folder gate | no distinct signal | unknown | no | no | strict | yes | unknown | no | no | 2026-09-09 |
| [Gemini CLI](gemini.md) | B | manual settings.json merge | manual merge | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | 2026-09-09 |
| OpenCode | C | N/A | N/A | unknown | yes | `session.error` | `permission.asked` | N/A | N/A | N/A | yes | inferred | deferred |
| Antigravity | C | N/A | N/A | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unsupported |

## Shapes

The shape is *what the agent invokes*, not how the config gets installed:

- **A**: hook config with one command per event. The existing CLI is the adapter.
- **B**: one callback receives a JSON payload. Requires the `notify` subcommand.
- **C**: in-process adapter loaded by the agent (agents often call this a
"plugin"). Deferred until shapes A and B are in.

Delivery is a separate axis: a manual merge into the agent's settings, a drop-in
file, or a plugin package that ships the config in its own directory. Claude Code
is shape A delivered as a plugin, with the same hook set also shipped as a
drop-in - hence `A (plugin)` in the table.

## Reading the table

- **Drop-in file**: a file the user copies verbatim into a hooks directory; the
tool never edits the user's hand-maintained settings.
- **Needs enabling**: a feature flag or trust step required before the hooks fire.
- **Subagent events**: whether start/stop events exist and whether a stop should
map to `done` or stay `working`.
- **Multi-session per pane**: whether the agent can run several sessions in one
pane. Shape A agents cannot roll them up; the matrix records the limitation.
- **`error` event**: a published turn-abort event, or "inferred" if it must be
derived from the absence of a clean stop. A failing tool call is not one: the
turn is still running, so tool-failure events map to `working` and the agent's
cell reads "no".
- **`waiting` repeats**: a blocked-on-you event that fires repeatedly, including
idle nags. Without the repeat, `waiting` is rarely useful.
- **Stdout parsed**: whether the agent reads the hook's stdout as JSON. Strict
parsers require the documented `printf '{}'` wrapper.
- **Payload on stdin**: whether the event payload arrives on stdin instead of argv.
- **`TMUX_PANE` inherited**: whether the hook runs as a child of the pane. Unknown
across the surveyed agents means `--pane` / `TMUX_AGENT_STATUS_PANE` should be used
defensively.
- **Session start / end**: whether events map onto `reset` and `finish`. Agents
without both keep the known limit that a crashed agent can strand `working`.

## Common setup steps

Every agent page repeats:

1. Where to place the drop-in file (or what to merge into the agent's config).
2. How to enable the hook system if it needs enabling.
3. How to prove a hook fired.
4. `TMUX_AGENT_STATUS_DISABLED=1` and `TMUX_AGENT_STATUS_DEBUG=1`.
5. Quirks specific to that agent, including stdout parsing and subagent rules.
