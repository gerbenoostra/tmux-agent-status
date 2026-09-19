# Kiro

Shape A agent with a drop-in JSON hook file. Kiro reads hook files from
`~/.kiro/hooks/` on the **v3 engine**; the installed 2.x engine embeds
camelCase hooks in per-agent config instead, and that route is unverified.

## Supported states

| State | Kiro trigger | Command | Notes |
| --- | --- | --- | --- |
| reset | `agentSpawn` | `tmux-agent-status reset` | confirmed camelCase, from the shipped binary's own trigger set |
| start | `userPromptSubmit` | `tmux-agent-status start` | a turn begins; replaces whatever the last turn left |
| working | `preToolUse`, `postToolUse` | `tmux-agent-status set working` | every tool call refreshes the glyph |
| done | `stop` | `tmux-agent-status set done` | rings the bell; the CLI has no session-end event, so there is no `finish` row |
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
