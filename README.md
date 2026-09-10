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
tmux-agent-status reset
tmux-agent-status finish
```

The `set` commands go on the agent's turn events. `reset` goes on session start and drops whatever
the previous agent left in the pane; `finish` goes on session end and resolves the session to done,
leaving an `error` alone. Neither of those two rings the bell.

The [docs/agents](docs/agents/README.md) pages give the manaul config, drop-in file or plugin for each supported agent, [Claude Code](docs/agents/claude-code.md) included.

## Install

See [docs/install.md](docs/install.md) on how to install the helper command line tool (nix flake input, `nix profile`, a prebuilt binary,
`cargo`, and building from source).

After installing the command line tool, there are three things left:
 - include the `tmux-agent-status.conf` into your tmux config to hook onto tmux's events.
 - include the `agent_status` placeholder in your tmux's window status format.
 - register the `tmux-agent-status` commands as agent hooks.

## Set up

**1. Check the binary.**
After installing, verify the binary is available: `tmux-agent-status --version` should print the version and the executable that is actually running.

**2. Source the tmux snippet.**
It ships at [`share/tmux/tmux-agent-status.conf`](./share/tmux/tmux-agent-status.conf) in the package.
Import it from your tmux configuration using the path where you installed it. For example, if you placed it in `~/.tmux`:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The snippet may live elsewhere; see the [installation path guidance](docs/install.md#choose-installation-paths).

It only adds two tmux hooks. Both call `tmux-agent-status clear-window <pane>`; the optional pane argument defaults to `$TMUX_PANE` for manual calls.
Can be confirmed with `tmux show-hooks -g | grep tmux-agent-status` and `tmux show-hooks -gw | grep tmux-agent-status`.

**3. Include the glyph term in your tmux window format.**
Paste the following format term into **both** `window-status-format` and `window-status-current-format`, after the name segment (outside any truncation you have) and before the window flags:

```tmux
#{?@agent_status, #{@agent_status},}
```

For example:

```tmux
set -g window-status-format '#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever you already had.
You can confirm it renders by setting a glyph by hand: `tmux set-option -w @agent_status ✅`, then `tmux set-option -w -u @agent_status`.

**Optional: terminal & tmux bell and the tmux highlight.**
The tool writes a `\a` to the tmux pane. You can configure tmux with what you want to be done with it:

| Setting | What it decides | Suggested |
| --- | --- | --- |
| `monitor-bell` | Whether tmux notices the bell at all. `off` means no highlight, and nothing reaches your terminal either. | `on` (tmux default) |
| `bell-action` | Which windows may pass a bell on to your terminal. `other` ignores the window you are currently on, which is exactly where an agent finishes while your terminal sits behind another tab, desktop or monitor. `any` passes those on too. | `any` (tmux default) |
| `visual-bell` | Whether the bell stays a bell. `on` replaces it with a tmux message, so your terminal never sees it. | `off` (tmux default) |
| `window-status-bell-style` | How a window that rang is painted until you visit it. It only ever applies to windows you are *not* on: tmux drops the flag of the current window immediately. | to taste |

```tmux
setw -g monitor-bell on
set -g bell-action any
set -g visual-bell off
setw -g window-status-bell-style 'fg=magenta,bold,nodim'
```

The price of `any` is that every other bell from the window you are on reaches the terminal as well:
a shell completion beep, vim hitting the end of a search, and an agent finishing in the window you
are already watching. The benefit is that you also get a bell on tabs in your terminal if the
active tmux window rang. If you are in one terminal tab, and in another you have a tmux session with an agent,
and that agent is in the active window, it will get a terminal bell with `any`, and no terminal bell otherwise.

What your terminal then does with that bell is its own business, and it is often not a sound.
Ghostty, for example, prefixes the tab title with 🔔 and asks for attention while it is unfocused,
and stays silent unless you enable a sound in `bell-features`. So the bell tells you *which tab*,
and the glyph tells you *which window*.

**4. Register the agent hooks.**

Claude Code has a plugin, for other agents manually edit its configuration.

### Configure agents

Follow [docs/agents/README.md](docs/agents/README.md) for instructions to watch your agent of choice, it refers to a page per agent.

For example, see [docs/agents/claude-code.md](docs/agents/claude-code.md) for the plugin, the manual hook merge,
the event mapping of Claude.

## How it works

The `tmux-agent-status` executable is called from your coding agent's lifecycle hooks. It writes
a tmux option per pane indicating the agent status, summarizes the states of all panes to a single glyph on the window,
and rings the terminal bell.

We use two tmux options, separating status from final glyph:

- **`@agent_pane_status`**, per pane, holds the state name of the agent in that pane. Written from `$TMUX_PANE` by `set`.
- **`@agent_status`**, per window, holds the glyph. The maximum by rank over that window's panes,
  recomputed after every write. This is what's interpolated in the format string.

They have different names because tmux option inheritance returns the window's value when reading a
pane without a value.

Looking at a window clears it. Switching to a window, or to another pane inside it, drops that
window's `waiting`, `error` and `done`; `working` survives, because otherwise an agent you glance at
would go blank while it is still running. A turn that ends on the window you are **already** watching
is cleared on the spot: the bell rings and no glyph appears, because you are looking at the pane that
would have explained it. Watched means the window is the current window of a session with a client attached, so
a turn ending while you are detached keeps its glyph until you come back.

## The bell, and colour

For the end states (`waiting`, `error` and `done`, thus not `working`) a terminal bell (`\a`) is printed.
With `monitor-bell on`, tmux gives you the window highlight, in whatever way you configure it, and
`bell-action` decides whether the bell also reaches your terminal: see the table in
[setup step 3](#set-up).
To not interfere with your own highlight format, this tool deliberately does not colour or name windows.

If you want `error` to stand out further, paste this in front of the name segment, in both formats:

```tmux
#{?#{==:#{@agent_status},❗},#[fg=colour160#,bold#,nodim],}
```

For example:

```tmux
set -g window-status-format '#I:#{?#{==:#{@agent_status},❗},#[fg=colour160#,bold#,nodim],}#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The style runs to the end of the entry, so the name and flags turn red, and on that window it replaces
the `window-status-bell-style` colour. `#,` escapes a comma inside `#{...}`.
It is also documented in the [shipped snippet](./share/tmux/tmux-agent-status.conf).

## Interoperability

`@agent_pane_status` and `@agent_status` are this tool's entire tmux footprint, so anything owning a
different option prefix can stay installed alongside it.
The one shared resource is the format string specifying the window name, where this tool appends one
term to.

## Disabling

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every subcommand into a no-op that exits 0. No tmux
options are written, no bell rings - the binary returns success immediately. Any non-empty value
counts; the documented spelling is `=1`.

Useful for CI, demo recordings, nested test sessions, or any environment where the hooks fire but
you do not want the glyphs.

## Known limits

- An agent that dies without firing `Stop` or a session-end event keeps 🤖 until the next agent
  starts in that pane. We're planning a future `stale` 💤 state that decays from `working` after a
  timeout.
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
