# GitHub Copilot CLI

Shape A agent with a drop-in JSON hook directory. Copilot CLI loads policy
files, then `.github/hooks/*.json` (repo scope), then `~/.copilot/hooks/*.json`
(user scope), then inline `hooks` blocks in `settings.json`.

## Supported states

| State | Copilot event | Command | Notes |
| --- | --- | --- | --- |
| reset | `sessionStart` | `tmux-agent-status reset` | |
| working | `userPromptSubmitted` | `tmux-agent-status set working` | |
| done | `agentStop` | `tmux-agent-status set done` | |
| waiting | `notification` | `tmux-agent-status set waiting` | captures `permission_prompt` and `agent_idle` |
| error | `errorOccurred` | `tmux-agent-status set error` | |
| finish | `sessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

## Drop-in file

Copy `share/agents/copilot/tmux-agent-status.json` to `.github/hooks/tmux-agent-status.json`
for a repo-wide hook, or to `~/.copilot/hooks/tmux-agent-status.json` for a user-wide
hook. No enable step is required.

```sh
mkdir -p .github/hooks
cp /path/to/share/agents/copilot/tmux-agent-status.json .github/hooks/
```

## Prove it fired

Start a Copilot session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After submitting a prompt it should read `working`; after the agent stops it should
read `done` or be empty.

## Quirks

- **Notifications are fire-and-forget.** The shipped hook maps all `notification`
  events to `waiting`, which covers `permission_prompt` and `agent_idle`. If your
  agent emits other notification types you do not want mapped, narrow the matcher
  in the JSON file.
- **Subagent events are explicit.** `subagentStart`/`subagentStop` exist; a
  subagent stopping does not end the parent turn, so they are deliberately not
  mapped to `done`.
- **Stdout parsing.** Copilot CLI parses hook stdout as JSON per event, so
  every entry in the shipped file appends `printf '{}\n'`. The status commands
  write nothing to stdout themselves.
- **`TMUX_PANE` inheritance is undocumented.** Use `--pane #{pane_id}` or set
  `TMUX_AGENT_STATUS_PANE` if the hook runner is not a child of the pane.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
