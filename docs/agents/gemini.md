# Gemini CLI

Shape B agent with a manual settings.json step. As of the survey date, Gemini CLI
only supports hooks defined inside user or project `settings.json`; the
extension-hooks drop-in proposal is tracked in the upstream feature request.

## Supported states

| State | Gemini event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` (planned) | `notify --stdin` | payload schema unconfirmed |
| working | `PreToolUse` / `PostToolUse` (planned) | `notify --stdin` | payload schema unconfirmed |
| done | `SessionEnd` (planned) | `notify --stdin` | not confirmed as shipped |
| waiting | — | — | no event documented |
| error | — | — | no event documented |

The payload schema is not public, so the current binary mapping drops every
payload for `--agent gemini` and exits 0. This keeps the hook config valid while
the upstream API stabilises.

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
  `settings.json` because the extension-hooks proposal is not yet shipped.
- **Payload schema unconfirmed.** `tmux-agent-status notify --agent gemini` drops
  all payloads. Set `TMUX_AGENT_STATUS_DEBUG=1` to see which payloads arrive.
- **Event names are planned, not verified.** `SessionEnd` in particular is not
  confirmed as a shipped event.
