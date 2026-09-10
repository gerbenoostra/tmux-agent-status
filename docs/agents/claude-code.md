# Claude Code

Shape A agent, and the reference one: it is the agent this tool was built against, so it is the only
supported agent where every state has a published event. Claude Code reads hooks from a plugin's own
directory or from `~/.claude/settings.json`.

## Supported states

| State | Claude Code event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` (`startup\|resume\|clear\|fork`) | `tmux-agent-status reset` | |
| working | `UserPromptSubmit`, `PostToolUse` | `tmux-agent-status set working` | |
| done | `Stop` | `tmux-agent-status set done` | |
| waiting | `Notification`, `PreToolUse` (`AskUserQuestion\|ExitPlanMode`) | `tmux-agent-status set waiting` | |
| error | `StopFailure` | `tmux-agent-status set error` | a real turn-abort event, which most agents lack |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

`Notification` is deliberately **not** narrowed to permission prompts: the idle nag arrives on the
same event, and the repeat is what makes `waiting` useful at all.

## The plugin

The recommended route. It carries the hook set in its own directory, so nothing of yours is edited:

```
/plugin marketplace add gerbenoostra/tmux-agent-status
/plugin install tmux-agent-status
```

Restart the session and the six hook entries above are live. Your `~/.claude/settings.json` stays
untouched apart from the `enabledPlugins` and `extraKnownMarketplaces` entries Claude Code records
itself. To revert:

```
/plugin uninstall tmux-agent-status
/plugin marketplace remove tmux-agent-status
```

## Drop-in file

`share/agents/claude-code/hooks.json` is the same hook set as a file, for a user who did not install
the plugin. It is byte for byte the plugin's own `hooks/hooks.json`, and a test keeps it that way.

Claude Code has no hooks drop-in directory, so this is a **merge**, not a copy. Take the file whole
if `~/.claude/settings.json` has no `hooks` key; if you already have one, add these eight events
inside it. Do not append the file as a second top-level object and do not end up with two `hooks`
keys - JSON's last one silently wins and the hooks you had are gone.

See [docs/install.md](../install.md) for where `share/agents/` lands for Nix, prebuilt tarballs and
`cargo install`.

## Prove it fired

Start a Claude Code session in a tmux pane and check `@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After submitting a prompt it should read `working`; after the turn stops it should read `done` or be
empty. With the plugin installed, `/tmux-agent-status:doctor` checks all four setup steps for you.

## Quirks

- **`StopFailure` is a genuine error event.** Nearly every other surveyed agent leaves the `error`
  column empty and has to infer an abort, or cannot see one at all.
- **No `printf '{}'` wrapper is needed.** Claude Code does not require JSON on stdout for these
  events, and the hook commands write nothing to stdout anyway; the bell goes to `/dev/tty`.
- **The bell is ours.** No standalone `printf '\a'` hook for the same events is needed. To use
  Claude Code's own notification channel instead, see the
  [README](../../README.md#claude-code).
- **A missing binary is silent.** If `tmux-agent-status` is not on the `PATH` the hooks inherit, no
  error is raised anywhere; you simply never see a glyph.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that exits 0. Set
`TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr (shape B agents only).
