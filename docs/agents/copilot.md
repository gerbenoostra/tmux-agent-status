# GitHub Copilot CLI

Shape A agent with a drop-in JSON hook directory. Copilot CLI loads policy
files, then `.github/hooks/*.json` (repo scope), then `~/.copilot/hooks/*.json`
(user scope), then inline `hooks` blocks in `settings.json`.

## Supported states

| State | Copilot event | Command | Notes |
| --- | --- | --- | --- |
| reset | `sessionStart` | `tmux-agent-status reset` | |
| start | `userPromptSubmitted` | `tmux-agent-status start` | a turn begins; replaces whatever the last turn left |
| working | `subagentStart`, `subagentStop` | `tmux-agent-status set working` | a subagent starting or stopping does not end the parent turn |
| done | `agentStop` | `tmux-agent-status set done` | |
| waiting | `notification` | `tmux-agent-status set waiting` | captures `permission_prompt` and `agent_idle` |
| error | `errorOccurred` | `tmux-agent-status set error` | |
| finish | `sessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

## Drop-in file

Copy [`share/agents/copilot/tmux-agent-status.json`](../../share/agents/copilot/tmux-agent-status.json) to `.github/hooks/tmux-agent-status.json`
for a repo-wide hook, or to `~/.copilot/hooks/tmux-agent-status.json` for a user-wide
hook. No enable step is required.

```sh
mkdir -p .github/hooks
cp /path/to/share/agents/copilot/tmux-agent-status.json .github/hooks/
```

## Quirks

- **Repo-scope hooks need the folder trusted.** `.github/hooks/*.json` is gated behind the same
  "this folder is not trusted" confirmation every repo session needs before it sends a prompt. It is
  not a hooks-specific step, but a first-time user in an untrusted repo will see that prompt before
  the hook ever fires.
- **Notifications are fire-and-forget.** The shipped hook maps all `notification`
  events to `waiting`, which covers `permission_prompt` and `agent_idle`.
  Narrowing matters more than it used to: a `waiting` is no longer replaced by the
  next `working`, so a type that does not mean blocked on you leaves a 💬 up until
  you focus that pane. The two documented types are both fine; if Copilot
  emits others, narrow the matcher in the JSON file the way
  [claude-code.md](claude-code.md) does.
- **Subagent events are explicit.** `subagentStart`/`subagentStop` exist; a
  subagent stopping does not end the parent turn, so they are deliberately not
  mapped to `done`.
- **Stdout parsing.** Copilot CLI parses hook stdout as JSON per event, so
  every entry in the shipped file uses `--json`. The status commands write nothing
  to stdout themselves; `--json` prints `{}` so the parser never sees empty stdout.
