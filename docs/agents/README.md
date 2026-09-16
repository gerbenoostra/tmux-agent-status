# Supported agents

The tool works for any agent that allows hooks on lifecycle events. These should be mapped to the following commands:
```
tmux-agent-status reset
tmux-agent-status set working
tmux-agent-status set waiting
tmux-agent-status set done
tmux-agent-status set error
tmux-agent-status finish
```

The `set` commands go on the agent's turn events. `reset` goes on session start and drops whatever
the previous agent left in the pane; `finish` goes on session end and resolves the session to done,
leaving an `error` alone. Neither of those two rings the bell. The cli allows `--json` if the agent
expects a json response.

The following table shows how this maps to common agents.
Blank cells link to the upstream doc or issue that says the event does not exist.

| Agent | Shape | Drop-in file | Needs enabling | Subagent events | Multi-session per pane | `error` event | `waiting` repeats | Stdout parsed | Payload on stdin | `TMUX_PANE` inherited | Session start | Session end | Verified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [Claude Code](claude-code.md) | A (plugin) | yes, via plugin | no | no | no | `StopFailure` | `Notification` | lenient | yes | yes | yes | yes | yes | plugin API |
| [Codex CLI](codex.md) | A | yes | hook trust (first run) | `SubagentStart`/`SubagentStop` | unknown | inferred | `PermissionRequest` | yes | yes | unknown | yes | yes | 2026-09-10 |
| [GitHub Copilot CLI](copilot.md) | A | yes | folder trust (repo scope) | yes | unknown | `errorOccurred` | `notification` | yes | unknown | unknown | yes | yes | 2026-09-10 |
| [Droid](droid.md) | A | yes | no | `SubagentStop` only | unknown | inferred | `Notification` | yes | yes | unknown | yes | yes | 2026-09-10 |
| [Cursor](cursor.md) | A | yes | no | yes | unknown | no | unknown | yes | yes | unknown | yes | yes | 2026-09-10 |
| [Devin CLI](devin.md) | A | yes (project only) | no | `run_subagent` tool only | unknown | no | unknown | yes | yes | unknown | yes | yes | 2026-09-10 |
| [Grok CLI](grok.md) | A | yes | project hooks need `/hooks-trust` | `SubagentStart`/`SubagentStop` -> `working` | unknown | no (`StopFailure` unverified) | no (`Notification` trigger undocumented) | lenient | yes | unknown | yes | yes | 2026-09-11 |
| [Kiro](kiro.md) | A | yes (v3 engine only) | no | no stop event | yes | no | no | unconfirmed | yes | unknown | CLI: `agentSpawn` | no | 2026-09-10 |
| [Mistral Vibe](mistral-vibe.md) | B | yes (TOML) | trusted-folder gate | no distinct signal | unknown | no | no | strict | yes | unknown | no | no | 2026-09-09 |
| [Gemini CLI](gemini.md) | B | manual settings.json merge | manual merge | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | 2026-09-09 |
| OpenCode | C | N/A | N/A | unknown | yes | `session.error` | `permission.asked` | N/A | N/A | N/A | yes | inferred | deferred |
| Antigravity | C | N/A | N/A | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unknown | unsupported |


## Shapes

There are different ways to hook onto lifecycle events, which is captured by "Shape".
The shape is *what the agent invokes*, not how the config gets installed:

- **A**: hook config with one command per event, calling the `tmux-agent-status` CLI as adapter.
- **B**: one callback receives a JSON payload, calling the `tmux-agent-status notify` subcommand.
- **C**: in-process adapter loaded by the agent (agents often call this a
"plugin"). Currently not implemented.

Delivery is a separate axis:
- a manual merge into the agent's settings
- a drop-in file
- a plugin package that ships the config in its own directory.

For example, Claude Code is shape A delivered as a plugin, with the same hook set also shipped as a
drop-in - hence `A (plugin)` in the table.

## Table legend

- **Drop-in file**: a file you can copy verbatim into a hooks directory
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
idle nags.
- **Stdout parsed**: whether the agent reads the hook's stdout as JSON. Strict
parsers require the `--json` flag.
- **Payload on stdin**: whether the event payload arrives on stdin instead of argv.
- **`TMUX_PANE` inherited**: whether the hook runs as a child of the pane. Unknown
across the surveyed agents means `--pane` / `TMUX_AGENT_STATUS_PANE` should be used
defensively.
- **Session start / end**: whether events map onto `reset` and `finish`. Agents
without both keep the known limit that a crashed agent can strand `working`.

## Shared hook behaviour

These apply to every agent page:

- **The tool rings the bell itself.** The `set` commands for end states (`done`,
  `waiting`, `error`) print a terminal bell, so do not add a separate `printf '\a'`
  hook for the same event. Agents that parse stdout still use `--json` so the
  parser sees valid JSON.
- **A prompt event starts a turn.** The event that means the human typed maps to
  `tmux-agent-status start`, not `set working`. It is the one write that replaces
  whatever the pane already holds, because typing into a pane is seeing it; a
  `set` deliberately will not, so a state the last turn left would otherwise
  outrank every state of this one.
- **A missing binary is silent.** If `tmux-agent-status` is not on the `PATH` the
  hook inherits, the command exits 0 and no error is raised anywhere; the only
  symptom is that no glyph ever appears.
- **`waiting` outranks a later `working`.** A state that means blocked on you is
  not replaced by the `working` of a sibling tool call that finishes while the
  prompt is still open, so a waiting event does not have to repeat to stay
  visible. Map the events that mean the agent is blocked on you, and leave out a
  nag that only fires once the turn has already ended: it cannot replace the ✅
  it arrives on, and it puts a 💬 up if you have already looked. The exception is
  an agent whose nag is the only event a cancelled turn emits, which is why
  Droid maps `idle_prompt` and Claude Code does not.

## Prove it fired

After configuring the hooks, (re)start an agent session in a tmux pane and check
`@agent_status` from any pane of the same window:

```sh
tmux display-message -p '#{@agent_status}'
```

`@agent_status` is the window rollup, so every pane in the window sees the same
value. After submitting a prompt it should read `🤖`; after the turn stops it
should read `✅` or be empty. If you have configured custom glyphs, the values
will match those instead.

If you've configured your tmux format string, it should update too.

## Common setup steps

Every agent page repeats:

1. Where to place the drop-in file (or what to merge into the agent's config).
2. How to enable the hook system if it needs enabling.
3. How to prove a hook fired.
4. `TMUX_AGENT_STATUS_DISABLED=1` and `TMUX_AGENT_STATUS_DEBUG=1`.
5. Quirks specific to that agent, including stdout parsing and subagent rules.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
