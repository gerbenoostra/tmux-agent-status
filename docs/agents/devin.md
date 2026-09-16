# Devin CLI

Shape A agent with a standalone drop-in file. This page covers **Devin CLI**
("Devin for Terminal"), Cognition's local terminal agent - the only Devin that
runs as a process inside a tmux pane. Devin Desktop's Cascade hooks and the
cloud API's webhook automations fire somewhere else entirely and cannot drive a
local glyph.

## Supported states

| State | Devin event | Command | Notes |
| --- | --- | --- | --- |
| reset | `SessionStart` | `tmux-agent-status reset` | |
| start | `UserPromptSubmit` | `tmux-agent-status start` | a turn begins; replaces whatever the last turn left |
| working | `PostToolUse` | `tmux-agent-status set working` |  |
| waiting | `PermissionRequest`, `PreToolUse` matching `^(ask_user_question\|exit_plan_mode)$` | `tmux-agent-status set waiting` | permission prompt, question, plan approval |
| done | `Stop` | `tmux-agent-status set done` | rings the bell; `finish` never does |
| error | — | — | no published turn-abort event; a failed tool call is not one |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |


The `PreToolUse` matcher is a regex over Devin's **own** tool names, which are
lower case and shorter than Claude's: `read`, `exec`, `grep`, `glob`,
`exit_plan_mode`, `ask_user_question`.

`PreToolUse` is matched, not blanket-mapped: the two tools it names are the ones
that block on you, and a blanket `working` on every tool call would say nothing
the `PostToolUse` entry does not already say. It no longer guards against an
overwrite - `working` is the lowest state within a pane and never replaces a
`waiting`, whichever order the two hooks land in, which is what
[013](../../tasks/plans/013-pane-state-precedence.md) fixed. The pane goes back
to `working` on the first tool call after the window has been seen and the 💬
cleared with it.

## Drop-in file

Copy [`share/agents/devin/hooks.v1.json`](../../share/agents/devin/hooks.v1.json) to `<repo>/.devin/hooks.v1.json`. In
that file the hook map **is** the whole file: there is no top-level `hooks` key,
unlike every other location Devin reads.

```sh
mkdir -p .devin
cp /path/to/share/agents/devin/hooks.v1.json .devin/hooks.v1.json
```

Devin walks the working directory and its ancestors up to the repository root,
so a file at the repo root covers every subdirectory you start Devin from.

### User-wide instead of per repository

There is no user-level `hooks.v1.json`; a user-wide hook set has to be merged
under a `"hooks"` key into `~/.config/devin/config.json`
(`%APPDATA%\devin\config.json` on Windows), which is a file you maintain. The
drop-in file ([`share/agents/devin/hooks.v1.json`](../../share/agents/devin/hooks.v1.json)) is what should be merged: its
top-level object is the `hooks` object, so nest the whole drop-in under `"hooks"`
in your config. Merge it into the `hooks` object you already have rather than
adding a second one; JSON's last key silently wins.


## Quirks

- **One unknown event key discards every hook.** Devin drops the *whole* hook
  map and warns `Ignoring invalid value for "hooks" ... Using the default ({})`.
  A single typo, or one event name Devin does not know, silently disables the
  lot. Only the eight documented events exist: `SessionStart`, `SessionEnd`,
  `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Stop`,
  `PostCompaction`.
- **A Claude Code hook set is not a Devin hook set.** Devin reads
  `~/.claude/settings.json` and `.claude/settings.json` by default
  (`read_config_from.claude`), so this project's Claude hook set is a candidate
  for the rule above: it carries `StopFailure` and `Notification`, which Devin
  does not know. Install the drop-in on this page rather than relying on the
  Claude route.
- **No `error` event.** Devin publishes no turn-abort event. `PostToolUse`
  carries `tool_response.success` and `tool_response.error`, but a failed tool
  call is an ordinary part of a turn that is still running, so it maps to
  `working` like any other tool event. An aborted turn keeps `working` until
  `SessionEnd`, or until the next session's `reset`.
- **Subagents.** There is no subagent stop event; `run_subagent` and
  `read_subagent` are ordinary tool calls, so they map to `working` through
  `PostToolUse`, which is correct - the parent turn has not ended. Only the
  parent's `Stop` means `done`.
- **Stdout is parsed as JSON** on `PreToolUse`, `PermissionRequest`,
  `UserPromptSubmit`, `SessionStart` and `Stop`, so every entry in the drop-in
  uses `--json`. The status commands write nothing to stdout themselves;
  `--json` prints `{}` so an empty stdout never reaches the parser.
- **`SessionEnd` needs a clean exit.** A killed terminal or `kill -9` skips the
  hook and strands the glyph until the next session's `reset`.
