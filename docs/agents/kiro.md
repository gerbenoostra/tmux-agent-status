# Kiro

Shape A agent with a drop-in JSON hook file. Kiro's hook system is unified across
IDE, CLI, and Web on `.kiro/hooks/*.json`.

## Supported states

| State | Kiro trigger | Command | Notes |
| --- | --- | --- | --- |
| reset | `AgentSpawn` (CLI) / `SessionStart` (IDE) | `tmux-agent-status reset` | CLI uses `AgentSpawn`; no `SessionEnd` on CLI |
| working | `PreToolUse` | `tmux-agent-status set working` | also `PostToolUse` to refresh the glyph |
| done | `AgentStop` | `tmux-agent-status set done` | rings the bell; the CLI has no session-end event, so there is no `finish` row |
| waiting | — | — | no recurring blocked-on-user event |
| error | — | — | no published error event; inferred only from hook exit code |

## Drop-in file

Copy the drop-in file to `.kiro/hooks/tmux-agent-status.json` in your project,
or to `~/.config/kiro/hooks/tmux-agent-status.json` for the user scope. Kiro loads
any `.json` in that directory automatically; no enable step is required. See
[docs/install.md](../install.md) for the install-path of `share/agents/` for
Nix, prebuilt tarballs, and `cargo install`.

```sh
mkdir -p .kiro/hooks
cp /path/to/share/agents/kiro/tmux-agent-status.json .kiro/hooks/
```

## Prove it fired

Start a Kiro session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After the first tool use it should read `working`; after the session stops it
should read `done` or be empty.

## Quirks

- **No `SessionEnd` on the CLI.** `AgentStop` ends the turn and is mapped to
  `set done`, which rings the bell. There is no session-end trigger to map to
  `finish`, so if Kiro crashes the last glyph may strand until the next
  `AgentSpawn` in that pane.
- **Multi-subagent TUI.** Kiro can run several subagents in one pane, but the
  hook table has no per-subagent stop event. The last event wins, which is a
  known limit for shape A agents.
- **Stdout is not parsed as JSON.** The commands can print nothing safely.
- **`TMUX_PANE` inheritance is undocumented.** If a hook runner is not a child of
  the pane, use `--pane #{pane_id}` or set `TMUX_AGENT_STATUS_PANE`.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
