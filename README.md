# tmux-agent-status

A lightweight tool that adds a glyph to the tmux window status, so you know in which window an agent needs you, and why.

Example result:
```
 0:notes  1:api ✅  2:refactor 🤖  3:migration 💬- 4:build*
```

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

A window with multiple agents will show the most demanding status. Thus `waiting` > `error` > `done` > `working`.

For windows with no agent this tool is a no-op.

The glyphs are emoji, so they survive a font change. They need a tmux client in UTF-8 mode; a client
without it renders them as underscores.

## Compatible agents
Any agent that allows to hook on to lifecycle events work.
You can register the following commands as hook commands:
```
tmux-agent-status set working
tmux-agent-status set done
tmux-agent-status set error
tmux-agent-status set waiting
```

The installation notes show how to do this per agent. Claude Code has a plugin that carries the
hook set for you; see [step 4](#claude-code).

## Install

See [docs/install.md](docs/install.md) for the nix flake input, `nix profile`, a prebuilt binary,
`cargo`, and building from source.

After installing the command line tool, there are three things left:
 - include the `tmux-agent-status.conf` into your tmux config to hook onto tmux's events.
 - include the `agent_status` placeholder in your tmux's window status format.
 - register the `tmux-agent-status set [state]` as agent hooks.

## Set up

**1. Check the binary.**
After installing, verify the binary is available: `tmux-agent-status --version` should print the version and the executable that is
actually running.

**2. Source the tmux snippet.**
It ships at `share/tmux/tmux-agent-status.conf` in the package [here](./share/tmux/tmux-agent-status.conf).
Import it from your tmux configuration using the path where you installed it. For example, if you
placed it in `~/.tmux`:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The snippet may live elsewhere; see the [installation path guidance](docs/install.md#choose-installation-paths).

It only adds two tmux hooks. Both call `tmux-agent-status clear-window <pane>`; the optional pane argument defaults to `$TMUX_PANE` for manual calls. Confirm with `tmux show-hooks -g | grep tmux-agent-status` and
`tmux show-hooks -gw | grep tmux-agent-status`: they sit in different scopes, so one command shows
only one of them.

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

**4. Register the agent hooks.**

Claude Code has a plugin, for other agents manually edit its configuration.

### Configure agents

#### Claude Code

There are two options, either the plugin or manually editing the hooks.

**The plugin.**

```
/plugin marketplace add gerbenoostra/tmux-agent-status
/plugin install tmux-agent-status
```

Restart the session and the six hooks below are live.

You can uninstall/revert using:
```
/plugin uninstall tmux-agent-status
/plugin marketplace remove tmux-agent-status
```

The plugin only carries the hook configuration. Your `~/.claude/settings.json` will be untouched, except
for the `enabledPlugins` and `extraKnownMarketplaces` by Claude Code.

The plugin also ships `/tmux-agent-status:doctor`, a read-only check of all four setup steps.

**Or the manual paste.**
Copy the contents of [`plugins/tmux-agent-status/hooks/hooks.json`](./plugins/tmux-agent-status/hooks/hooks.json) into
`~/.claude/settings.json`.

**The watched events**
These are the hooks being watched:

| Event | Matcher | State |
| --- | --- | --- |
| `UserPromptSubmit` | all | `working` |
| `PostToolUse` | all | `working` |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `waiting` |
| `Notification` | all, **not** narrowed | `waiting` |
| `Stop` | all | `done` |
| `StopFailure` | all | `error` |

Two things to know here. `Notification` must **not** be narrowed to permission prompts, should also catch idle
events that indicate "still blocked, and has been for a while".

As the tool rings the bell itself, no standalone `printf '\a'` hooks for the same events are needed.

Instead of relying on the hooks for the bell (and thus highlight), you can also use Claude's own `\a` bell events by configuring them as follows:
```json
{
  "preferredNotifChannel": "terminal_bell",
  "inputNeededNotifEnabled": true,
  "agentPushNotifEnabled": true
}
```

The agent hook will not raise errors if the `tmux-agent-status` command cannot be found. You'll only notice it as
no glyph appearing on the window.


## How it works

The `tmux-agent-status` executable is called from your coding agent's lifecycle hooks. It writes
a tmux option per pane indicating the agent status, summarizes the states of all panes to a single glyph on the window,
and rings the terminal bell.

We use two tmux options, separating status from final glyph:

- **`@agent_pane_status`**, per pane, holds the state name. Written from `$TMUX_PANE` by `set`.
- **`@agent_status`**, per window, holds the glyph. The maximum by rank over that window's panes,
  recomputed after every write. The only thing the format string reads.

They have different names because tmux option inheritance returns the window's value when reading a
pane without a value.

Looking at a window clears it. Switching to a window, or to another pane inside it, drops that
window's `waiting`, `error` and `done`; `working` survives, or an agent you glance at would go blank
while it is still running. A turn that ends on the window you are **already** watching is cleared on
the spot: the bell rings and no glyph appears, because you are looking at the pane that would have
explained it. Watched means the window is the current window of a session with a client attached, so
a turn ending while you are detached keeps its glyph until you come back.

## The bell, and colour

For the end states (`waiting`, `error` and `done`, thus not `working`) a terminal bell (`\a`) is printed.
With `monitor-bell on`, tmux gives you the window highlight, in whatever way you configure it.
To not interfere with your own highlight format, this tool deliberately does not colour or name windows.

If you want `error` to stand out further, the shipped snippet shows an opt-in one-liner for it.

## Interoperability

`@agent_pane_status` and `@agent_status` are this tool's entire tmux footprint, so anything owning a
different option prefix can stay installed alongside it.
The one shared resource is the format string specifying the window name, where this tool appends one
term to.

## Known limits

- An agent that dies without firing `Stop` leaves a permanent 🤖. We're planning a future `stale` 💤 state
  that decays from `working` after a timeout. An agent with a session-lifecycle event, like devin's
  `SessionStart`, can release it on the next run in that pane.
- A **zoomed** pane's siblings are hidden, but tmux still calls the whole window watched: their
  states clear when you look at the window, and a turn ending in a hidden sibling while you watch
  rings the bell and leaves no glyph.
- The same goes for a terminal window behind another tab, desktop or monitor: the client is
  attached, so tmux says you are looking. We're planning to read the client's focus flag so those
  keep their glyph.
- Creating or splitting a pane counts as looking at that window, so it clears the window's
  non-sticky states.
- Only agents that can push lifecycle events get a glyph at all. An absent glyph means "no signal".

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development environment, testing against your real config, and release checks.

## Licence

MIT.
