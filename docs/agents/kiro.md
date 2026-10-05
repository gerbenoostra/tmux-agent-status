# Kiro

Shape A agent with a drop-in JSON hook file. Kiro reads hook files from
`~/.kiro/hooks/` on the **v3 engine**; the installed 2.x engine embeds
camelCase hooks in per-agent config instead. The 2.x route was verified with
a disposable project agent, but the shipped standalone file targets v3.

## Supported states

| State | Kiro trigger | Command | Notes |
| --- | --- | --- | --- |
| reset | `agentSpawn` | `tmux-agent-status reset` | observed on 2.21.2 |
| start | `userPromptSubmit` | `tmux-agent-status start` | observed; a turn begins and replaces whatever the last turn left |
| working | `preToolUse`, `postToolUse` | `tmux-agent-status set working` | documented, but neither fired around the probed 2.21.2 shell call |
| done | `stop` | `tmux-agent-status set done` | observed; rings the bell; the CLI has no session-end event |
| waiting | — | — | no recurring blocked-on-user event confirmed |
| error | — | — | no published error event; inferred only from hook exit code |


## Drop-in file

Copy the drop-in file to Kiro's hooks directory:

```sh
mkdir -p ~/.kiro/hooks
cp share/agents/kiro/tmux-agent-status.json ~/.kiro/hooks/tmux-agent-status.json
```

## Quirks

- **No `SessionEnd` on the CLI.** `stop` ends the turn and is mapped to
  `set done`, which rings the bell. There is no session-end trigger to map to
  `finish`, so if Kiro crashes the last glyph may strand until the next
  `agentSpawn` in that pane.
- **Multi-subagent TUI.** Kiro can run several subagents in one pane, but the
  hook table has no per-subagent stop event. The last event wins, which is a
  known limit for shape A agents.
- **The installed 2.x hook route is project-agent configuration.** A
  disposable `.kiro/agents/tas-probe.json` produced `agentSpawn`,
  `userPromptSubmit` and `stop` on Kiro 2.21.2. The standalone shipped file is
  for the v3 engine.
- **Background work was not reproducible.** An S2 request on 2.21.2 did not
  create native background child work, and the hook table has no per-child
  start/stop pair. The S1 fixture and scalar replay live under
  [`tests/fixtures/kiro/lifecycle/s1-normal-turn/`](../../tests/fixtures/kiro/lifecycle/s1-normal-turn/) -
  see [Background work](README.md#background-work).
