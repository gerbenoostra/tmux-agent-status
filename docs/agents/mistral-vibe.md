# Mistral Vibe

Shape B agent: a single callback receives the JSON payload and the binary maps
it to a state. The drop-in file is TOML and pipes each event to `tmux-agent-status
notify --agent mistral-vibe --stdin`.

## Supported states

| State | Vibe event | Command | Notes |
| --- | --- | --- | --- |
| reset | — | — | no `session_start` hook type documented |
| working | `pre_tool`, `post_tool` | `notify --stdin` | any tool call starts/continues a turn |
| done | `post_agent` | `notify --stdin` | assistant turn ended with no pending tools |
| waiting | — | — | no blocked-on-you event documented |
| error | — | — | `post_tool` `tool_status = failure` is a tool result, not a turn abort |

## Drop-in file

Copy [`share/agents/mistral-vibe/hooks.toml`](../../share/agents/mistral-vibe/hooks.toml) to `./.vibe/hooks.toml` (project) or
`~/.vibe/hooks.toml` (user). Project-level `hooks.toml` only loads when the working
directory is in the trusted-folders list (`~/.vibe/trusted_folders.toml`); there is
no separate feature flag.

```sh
cp /path/to/share/agents/mistral-vibe/hooks.toml ./.vibe/hooks.toml
```

## Quirks

- **Strict stdout parsing.** Vibe's `strict = true` treats malformed stdout as a
  denial. The `notify` command writes nothing to stdout and exits 0, so it stays
  in the safe lane.
- **No `session_start`/`session_end` hooks.** A new session cannot `reset` the pane,
  and a session ending cannot `finish` it. Starting a new agent in the same pane
  clears any stranded glyph.
- **No confirmed `waiting` event.** `ask_user_question` is a tool call, so it only
  drives `working`.
- **A failed tool call is not an `error`.** `post_tool` with
  `tool_status = "failure"` means a grep matched nothing or a test run failed;
  the turn is still running, so it maps to `working`. Mapping it to `error` would
  paint ❗ and ring the bell several times during a healthy turn. Vibe publishes
  no turn-abort event, so its `error` column stays empty.
- **Subagents inherit hooks transitively.** There is no distinct subagent stop
  event; the parent's own `post_agent` is the correct `done` signal.
