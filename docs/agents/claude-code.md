# Claude Code

Shape A agent with a tracked-aggregate lifecycle: Claude Code reads hooks from a plugin's own
directory or from `~/.claude/settings.json`, and its session and subagent events carry stable IDs
the pane can track, so a background agent that outlives its parent turn keeps the pane at 🤖 until
the aggregate actually finishes.

The minimum probed version is Claude Code 2.1.287; everything below is what a real run emitted.

## Supported states

| State | Claude Code event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` (`startup\|resume\|clear\|fork`) | `tmux-agent-status reset --agent claude-code --stdin` | accepts the session; an unreadable payload still clears the pane |
| start | `UserPromptSubmit` | `tmux-agent-status start` | a turn begins; replaces whatever the last turn left |
| working | `PostToolUse` | `tmux-agent-status set working` |  |
| work started | `SubagentStart` | `tmux-agent-status notify --agent claude-code --stdin` | opens a tracked background item under its `agent_id` |
| work stopped | `SubagentStop`, `PostToolUse` (`TaskStop`) | `tmux-agent-status notify --agent claude-code --stdin` | `SubagentStop` for a finished agent; a successful `TaskStop` for a cancelled one |
| done | `Stop` | `tmux-agent-status set done` | silent while tracked work remains; the final root `Stop` rings once |
| waiting | `Notification` (`permission_prompt\|elicitation_dialog\|elicitation_url_dialog\|agent_needs_input`), `PreToolUse` (`AskUserQuestion\|ExitPlanMode`) | `tmux-agent-status set waiting` | the types that mean blocked on you; see [Quirks](#quirks) |
| error | `StopFailure` | `tmux-agent-status set error` | a real turn-abort event, which most agents lack |
| finish | `SessionEnd` | `tmux-agent-status finish --agent claude-code --stdin` | ends the accepted session and its tracked work, no bell |

## The plugin

The recommended route. It carries the hook set in its own directory, so nothing of yours is edited:

```
/plugin marketplace add gerbenoostra/tmux-agent-status
/plugin install tmux-agent-status
```

Restart the session and the hook set above is live. Your `~/.claude/settings.json` stays
untouched apart from the `enabledPlugins` and `extraKnownMarketplaces` entries Claude Code records
itself. To revert:

```
/plugin uninstall tmux-agent-status
/plugin marketplace remove tmux-agent-status
```

## Manual configuration

Claude Code has no hooks drop-in directory, thus you need to **merge** [`share/agents/claude-code/hooks.json`](../../share/agents/claude-code/hooks.json)
into your `~/.claude/settings.json`. Take the whole file if your Claude settings has no `hooks` key;
if you already have one, add these ten events inside it. Do not append the file as a second top-level
object and do not end up with two `hooks` keys - JSON's last one silently wins and the hooks you had are gone.

See [docs/install.md](../install.md) for where `share/agents/` lands for Nix, prebuilt tarballs and
`cargo install`.

## Background work

A `run_in_background` Agent fires `SubagentStart` with a stable `agent_id`, keeps firing its tool
events under that ID, and ends with `SubagentStop`. When the parent turn ends first the pane stays
🤖, because Claude then runs an automatic wake turn on its own that ends in a root `Stop` - that
last `Stop`, not the child's, is what exposes ✅ and rings, once.

- **Cancellation stops work by ID.** A `TaskStop` the model runs emits `PostToolUse` carrying the
  cancelled agent's ID in both `tool_input.task_id` and `tool_response.task_id` - the same ID as its
  `SubagentStart`'s `agent_id` - and no `SubagentStop`. Only a `PostToolUse` whose two IDs agree
  stops the item; a failed `TaskStop` arrives as `PostToolUseFailure` and removes nothing.
- **The start hook is synchronous on purpose.** Claude awaits `SubagentStart` hooks before the
  child's first model call, so a stop can never reach us before its start; a `SubagentStop` for a
  never-started ID - internal helpers fire those routinely - is a no-op.
- **A missing stop keeps the pane at 🤖** until `SessionEnd` clears the session's ledger or the
  next `SessionStart` resets the pane.
- **Not everything is trackable.** `background_tasks` entries of type `shell` - background `Bash` -
  and `Monitor` timers fire no lifecycle events, so a root `Stop` can show ✅ while they run.
  Killing a task from the tasks UI emits no stop event of its own; if the killed item's
  `SubagentStop` never arrives, the pane holds 🤖 until the session boundary clears it.
- **Upgrading mid-session stays scalar.** A session already running when these hooks are installed
  has no accepted session until its next `SessionStart`, so its work events are ignored and the
  pane behaves as before for the rest of that session.
- **One session per pane is tracked.** The agents sidebar emits `SessionStart`/`SessionEnd` pairs
  for other session ids in the same pane; the latest `SessionStart` wins - it resets the pane and
  accepts that session - and lifecycle events naming any other session are ignored.

## Quirks

- **The `Notification` matcher is narrowed on purpose.** Claude Code sends twelve notification
  types to this event, and several of them - `agent_completed`, `auth_success`,
  `elicitation_complete`, `quota_auto_resume_*` - do not mean it is blocked on you. A `waiting` is
  not replaced by a later `working`, so an unnarrowed matcher would leave 💬 up for the rest of the
  turn. `idle_prompt` is left out too: probed on 2.1.287, it fires a few minutes after the last
  event - even mid-turn during API retries - so it cannot replace the ✅ it would arrive on, and it
  puts a 💬 up if you have already looked.
- **`SessionStart` has a `compact` source too.** `/compact` emits `SessionStart` with
  `source: "compact"` on the same session id; the matcher deliberately excludes it so a compact
  does not `reset` the pane and its tracked work. `/clear` and `--resume` do match (`clear`,
  `resume`), and `/clear` gets a fresh session id.
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
