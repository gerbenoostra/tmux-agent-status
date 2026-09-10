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
| working | `UserPromptSubmit`, `PostToolUse` | `tmux-agent-status set working` | |
| waiting | `PermissionRequest`, `PreToolUse` matching `^(ask_user_question\|exit_plan_mode)$` | `tmux-agent-status set waiting` | permission prompt, question, plan approval |
| done | `Stop` | `tmux-agent-status set done` | rings the bell; `finish` never does |
| error | — | — | no published turn-abort event; a failed tool call is not one |
| finish | `SessionEnd` | `tmux-agent-status finish` | resolves a lingering `working`, no bell |

Verified end to end on Devin CLI v3000.10.21: `reset` cleared a stale glyph a
new session inherited, a turn ran `working`, `ask_user_question` raised
`waiting`, the answer put it back to `working`, `Stop` rang the bell on `done`,
and `/exit` left `done` standing. `PermissionRequest` is the one row not
observed - the run auto-approved every tool it used - so it rests on Devin's
docs rather than on a run.

The `PreToolUse` matcher is a regex over Devin's **own** tool names, which are
lower case and shorter than Claude's: `read`, `exec`, `grep`, `glob`,
`exit_plan_mode`, `ask_user_question`. Claude's `AskUserQuestion|ExitPlanMode`
matches nothing here.

`PreToolUse` is matched, not blanket-mapped: an unmatched `PreToolUse` firing
after `PermissionRequest` would overwrite `waiting` with `working` while the
prompt is still on screen. `PostToolUse` is what returns the pane to `working`
once the tool has run.

## Drop-in file

Copy `share/agents/devin/hooks.v1.json` to `<repo>/.devin/hooks.v1.json`. In
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
drop-in file (`share/agents/devin/hooks.v1.json`) is what should be merged: its
top-level object is the `hooks` object, so nest the whole drop-in under `"hooks"`
in your config. Merge it into the `hooks` object you already have rather than
adding a second one; JSON's last key silently wins.

## Prove it fired

Run `/hooks` inside Devin to see what it loaded. Then, in a tmux pane, check
`@agent_pane_status`:

```sh
tmux display-message -p '#{@agent_pane_status}'
```

After submitting a prompt it should read `working`; after the turn stops it
should read `done` or be empty.

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
- **`TMUX_PANE` inheritance is undocumented.** Hooks are plain shell commands in
  a child process, so it should be inherited, but nothing says so. Use
  `--pane #{pane_id}` or set `TMUX_AGENT_STATUS_PANE` if the glyph lands
  nowhere.

## Opt-out and debug

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every hook command into a no-op that
exits 0. Set `TMUX_AGENT_STATUS_DEBUG=1` to log dropped `notify` events to stderr
(shape B agents only).
