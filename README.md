# tmux-agent-status

A lightweight tool that adds a glyph to the tmux window status, so you know in which window an agent needs you, and why.

Example result:
```
 0:notes  1:api ✅  2:refactor 🤖  3:migration 💬- 4:build*
```

The internal `agent-status` executable is called from your coding agent's lifecycle hooks. It writes
a tmux option per pane indicating the agent status, summarizes the states of all panes to a single glyph on the window,
and rings the terminal bell.

To not interfere with your formatting, window naming scripts, or monitor-bell, this tool deliberately
does not change, colour, or format window names. It just enables the bell and adds a glyph.

## The four states

The following states are distinguished:

| State | Glyph | Means | Clears when |
| --- | --- | --- | --- |
| `working` | 🤖 | a turn is in flight | the next event on that pane |
| `done` | ✅ | the turn ended cleanly | you look at the window |
| `error` | ❗ | the turn aborted: API error, context overflow, unparseable tool call | you look at the window |
| `waiting` | 💬 | blocked on you: permission prompt, plan mode, a question, the idle nag | you look at the window |

A window with multiple agents will show the status that wants you most. Thus `waiting` > `error` > `done` > `working`.

For windows with no agent this tool is a no-op.

The glyphs are emoji, so they survive a font change. They need a tmux client in UTF-8 mode; a client
without it renders them as underscores.

## Install

See [docs/install.md](docs/install.md) for the nix flake input, `nix profile`, a prebuilt binary,
`cargo`, and building from source.

After installing the command line tool, you'll need to include it in your tmux's window status format, and include it in your agent hooks.

## Set up, in this order

**1. Check the binary.**
After installing, verify the binary is available: `agent-status --version` should print the version and the executable that is
actually running.

**2. Source the tmux snippet.**
It ships at `share/tmux/agent-status.conf` in the package [here](./share/tmux/agent-status.conf).
Import it from your tmux configuration using the path where you installed it. For example, if you
placed it in `~/.tmux`:

```tmux
source-file ~/.tmux/agent-status.conf
```

The snippet may live elsewhere; see the [installation path guidance](docs/install.md#choose-installation-paths).

It only adds two tmux hooks. Confirm with `tmux show-hooks -g | grep agent-status`.

**3. Paste the format term.**
Into **both** `window-status-format` and `window-status-current-format`, after the name segment (outside any truncation you have) and before
the window flags:

```tmux
#{?@agent_status, #{@agent_status},}
```

For example:

```tmux
set -g window-status-format '#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever you already had. Confirm it renders by setting a glyph by hand:
`tmux set-option -w @agent_status ✅`, then `tmux set-option -w -u @agent_status`.

If you want to highlight or colour the window title when the bell has rung, add this to `~/.tmux.conf`:
```tmux
setw -g monitor-bell on
set -g bell-action other
setw -g window-status-bell-style 'fg=magenta,bold,nodim'
```

**4. Paste the agent hooks.**

To prevent unexpected scrambling of your config files, we ask you to manually edit your agent config.

For Claude Code, we recommend watching the following hooks in `~/.claude/settings.json`:

| Event | Matcher | State |
| --- | --- | --- |
| `UserPromptSubmit` | all | `working` |
| `PostToolUse` | all | `working` |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `waiting` |
| `Notification` | all, **not** narrowed | `waiting` |
| `Stop` | all | `done` |
| `StopFailure` | all | `error` |

Which can be done as follows:
```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "agent-status set working" }] }
    ],
    "PostToolUse": [
      { "matcher": "*", "hooks": [{ "type": "command", "command": "agent-status set working" }] }
    ],
    "PreToolUse": [
      {
        "matcher": "AskUserQuestion|ExitPlanMode",
        "hooks": [{ "type": "command", "command": "agent-status set waiting" }]
      }
    ],
    "Notification": [
      { "hooks": [{ "type": "command", "command": "agent-status set waiting" }] }
    ],
    "Stop": [{ "hooks": [{ "type": "command", "command": "agent-status set done" }] }],
    "StopFailure": [{ "hooks": [{ "type": "command", "command": "agent-status set error" }] }]
  }
}
```

Two things to know here. `Notification` must **not** be narrowed to permission prompts: the idle nag
is precisely the event meaning "still blocked, and has been for a while".

As the tool rings the bell itself, no standalone `printf '\a'` hooks for the same events are needed.

Instead of relying on the hooks for the bell (and thus highlight), you can also use Claude's own `\a` bell events by configuring them as follows:
```json
{
  "preferredNotifChannel": "terminal_bell",
  "inputNeededNotifEnabled": true,
  "agentPushNotifEnabled": true
}
```

The agent hook will not raise errors if the `agent-status` command cannot be found. You'll only notice it as
no `@agent_status` value being available in the window. If you don't get glyphs and want to diagnose whether the
hook is failing or the glyph printing fails, you can manually inspect the status by running:
```sh
tmux display-message -p '#{@agent_status}'
```


## How it works

We use two tmux options, separating status from final glyph:

- **`@agent_pane_status`**, per pane, holds the state name. Written from `$TMUX_PANE` by `set`.
- **`@agent_status`**, per window, holds the glyph. The maximum by rank over that window's panes,
  recomputed after every write. The only thing the format string reads.

They have different names because tmux option inheritance returns the window's value when reading a
pane without a value.

## The bell, and colour

For the end states (`waiting`, `error` and `done`, thus not `working`) a terminal bell (`\a`) is printed.
With `monitor-bell on`, tmux gives you the window highlight, in whatever way you configure it.
To not interfere with your own highlight format, this tool deliberately does not colour or name windows.

If you want `error` to stand out further, the shipped snippet carries an opt-in one-liner for it.

## Interoperability

`@agent_pane_status` and `@agent_status` are this tool's entire tmux footprint, so anything owning a
different option prefix can stay installed alongside it.
The one shared resource is the format string specifying the window name, where this tool appends one
term to.

## Known limits

- An agent that dies without firing `Stop` leaves a permanent 🤖. We're planning a future `stale` 💤 state
  that decays from `working` after a timeout.
- A **zoomed** pane's siblings are hidden, but are still cleared when looking at the tmux window.
- Creating or splitting a pane counts as looking at that window, so it clears the window's
  non-sticky states.
- Only agents that can push lifecycle events get a glyph at all. An absent glyph means "no signal".

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development environment, testing against your real config, and release checks.

## Licence

MIT.
