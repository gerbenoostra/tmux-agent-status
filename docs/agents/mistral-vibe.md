# Mistral Vibe

Shape B agent: a single callback receives the JSON payload and the binary maps
it to a state. The drop-in file is TOML and pipes each event to `tmux-agent-status
notify --agent mistral-vibe --stdin`.

## Supported states

| State | Vibe event | Command | Notes |
| --- | --- | --- | --- |
| reset | — | — | no `session_start` hook type documented |
| working | `pre_tool` | `notify --stdin` | any tool call starts/continues a turn |
| done | `post_agent` | `notify --stdin` | assistant turn ended with no pending tools |
| waiting | — | — | no blocked-on-you event documented |
| error | `post_tool` with `tool_status = failure` | `notify --stdin` | inferred from tool result |

## Drop-in file

Copy `share/agents/mistral-vibe/hooks.toml` to `./.vibe/hooks.toml` (project) or
`~/.vibe/hooks.toml` (user). Project-level `hooks.toml` only loads when the working
directory is in the trusted-folders list (`~/.vibe/trusted_folders.toml`); there is
no separate feature flag.

```sh
cp /path/to/share/agents/mistral-vibe/hooks.toml ./.vibe/hooks.toml
```

## Prove it fired

Start a Vibe session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After the first tool call it should read `working`; after an assistant turn that
ends cleanly it should read `done` or be empty.

## Quirks

- **Strict stdout parsing.** Vibe's `strict = true` treats malformed stdout as a
  denial. The `notify` command writes nothing to stdout and exits 0, so it stays
  in the safe lane.
- **No `session_start`/`session_end` hooks.** A new session cannot `reset` the pane,
  and a session ending cannot `finish` it. Starting a new agent in the same pane
  clears any stranded glyph.
- **No confirmed `waiting` event.** `ask_user_question` is a tool call, so it only
  drives `working`.
- **Subagents inherit hooks transitively.** There is no distinct subagent stop
  event; the parent's own `post_agent` is the correct `done` signal.
- **`TMUX_PANE` inheritance is undocumented.** If Vibe's hook runner is not a
  child of the pane, add `--pane #{pane_id}` or set `TMUX_AGENT_STATUS_PANE`.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped payloads to stderr.
