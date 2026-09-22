# Claude Code

Shape A agent: Claude Code reads hooks from a plugin's own directory or from `~/.claude/settings.json`.

## Supported states

| State | Claude Code event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` (`startup\|resume\|clear\|fork`) | `tmux-agent-status reset` | |
| start | `UserPromptSubmit` | `tmux-agent-status start` | a turn begins; replaces whatever the last turn left |
| working | `PostToolUse` | `tmux-agent-status set working` |  |
| done | `Stop` | `tmux-agent-status set done` | |
| waiting | `Notification` (`permission_prompt\|elicitation_dialog\|elicitation_url_dialog\|agent_needs_input`), `PreToolUse` (`AskUserQuestion\|ExitPlanMode`) | `tmux-agent-status set waiting` | the types that mean blocked on you; see [Quirks](#quirks) |
| error | `StopFailure` | `tmux-agent-status set error` | a real turn-abort event, which most agents lack |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

## The plugin

The recommended route. It carries the hook set in its own directory, so nothing of yours is edited:

```
/plugin marketplace add gerbenoostra/tmux-agent-status
/plugin install tmux-agent-status
```

Restart the session and the eight hook entries above are live. Your `~/.claude/settings.json` stays
untouched apart from the `enabledPlugins` and `extraKnownMarketplaces` entries Claude Code records
itself. To revert:

```
/plugin uninstall tmux-agent-status
/plugin marketplace remove tmux-agent-status
```

## Manual configuration

Claude Code has no hooks drop-in directory, thus you need to **merge** [`share/agents/claude-code/hooks.json`](../../share/agents/claude-code/hooks.json)
into your `~/.claude/settings.json`. Take the whole file if your Claude settings has no `hooks` key;
if you already have one, add these eight events inside it. Do not append the file as a second top-level
object and do not end up with two `hooks` keys - JSON's last one silently wins and the hooks you had are gone.

See [docs/install.md](../install.md) for where `share/agents/` lands for Nix, prebuilt tarballs and
`cargo install`.

## Quirks

- **The `Notification` matcher is narrowed on purpose.** Claude Code sends twelve notification
  types to this event, and several of them - `agent_completed`, `auth_success`,
  `elicitation_complete`, `quota_auto_resume_*` - do not mean it is blocked on you. A `waiting` is
  not replaced by a later `working`, so an unnarrowed matcher would leave 💬 up for the rest of the
  turn. `idle_prompt` is left out too: probed on 2.1.273, it does not fire while a permission
  prompt is open, and fires around eighteen minutes after a turn has already ended.
- **`StopFailure` is a genuine error event.** Nearly every other surveyed agent leaves the `error`
  column empty and has to infer an abort, or cannot see one at all.
- **The plugin hook runner accepts empty stdout.** The status commands write nothing to stdout, so
  no `--json` wrapper is needed in the plugin's hook set.
- **Claude's own notification channel.** If you prefer Claude Code's built-in `\a` bell events to
  the hook's bell, see [below](#using-claude-codes-own-notification-channel).

## Using Claude Code's own notification channel

Instead of relying on the hooks for the bell (and thus the highlight), you can also use Claude's own
`\a` bell events by configuring them as follows:

```json
{
  "preferredNotifChannel": "terminal_bell",
  "inputNeededNotifEnabled": true,
  "agentPushNotifEnabled": true
}
```
