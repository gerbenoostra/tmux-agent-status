# Gemini CLI

Shape B agent with a manual settings.json step. Gemini's hook system has shipped:
the documented events are `SessionStart`, `SessionEnd`, `BeforeAgent`,
`AfterAgent`, `BeforeModel`, `AfterModel`, `BeforeToolSelection`, `BeforeTool`,
`AfterTool`, `PreCompress` and `Notification`. Lifecycle matchers are exact
strings (`startup`, `resume`, `clear`), tool matchers are regular expressions.

## Supported states

| State | Gemini event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` | `notify --stdin` | fires on startup, resume and `/clear` |
| start | `BeforeAgent` | `notify --stdin` | after prompt submit, before planning |
| working | `BeforeTool` / `AfterTool` | `notify --stdin` | matcher is a regex over the tool name |
| done | `AfterAgent` | `notify --stdin` | the agent loop ends |
| waiting | `Notification` | `notify --stdin` | notification kinds unconfirmed |
| error | — | — | no event documented |
| finish | `SessionEnd` | `notify --stdin` | fires on exit and `/clear` |

The payload schema is not verified, so the current binary mapping drops every
payload for `--agent gemini` and exits 0. This keeps the hook config valid while
the upstream API stabilises. There is no subagent start/end pair, so Gemini
stays scalar - see [Background work](README.md#background-work).

## Manual settings.json step

Merge this block into your user or project `settings.json`. Do not add a second
`hooks` key; if you already have one, add these entries inside it.

```json
{
  "hooks": {
    "SessionStart": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ],
    "PreToolUse": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ],
    "PostToolUse": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ],
    "SessionEnd": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ]
  }
}
```

## Work in progress

After Gemini ships and fires one of the above events, check that the command exits
0 and does not break the agent. Once the payload schema is confirmed, the mapping
in `src/notify.rs` will be updated and the matrix above will change.

## Quirks

- **No drop-in file today.** The snippet above is a manual merge into
  `settings.json`; extension hook loading is still an upstream proposal.
- **The snippet predates the current event names.** It names `PreToolUse` and
  `PostToolUse`, while Gemini's shipped events are `BeforeTool`/`AfterTool`
  with `BeforeAgent`/`AfterAgent` around the loop. The mapping drops every
  payload anyway, so nothing is lost either way; a real probe still has to
  confirm which names fire and what the payloads carry.
- **Payload schema unconfirmed.** `tmux-agent-status notify --agent gemini` drops
  all payloads. Set `TMUX_AGENT_STATUS_DEBUG=1` to see which payloads arrive.
