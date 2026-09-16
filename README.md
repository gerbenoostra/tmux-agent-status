# tmux-agent-status

Shows your agent's status as a glyph in your tmux window name.

Example result:
```
 0:notes  1:api ✅  2:refactor 🤖  3:migration 💬- 4:build*
```

To not interfere with your formatting, window naming scripts, or monitor-bell, this tool deliberately
does not change, colour, or format window names. It just enables the bell and provides a glyph. Completely
compatible with all your other tmux preferences.

## The four states

The following agent states are distinguished:

| State | Glyph | Means | Clears when |
| --- | --- | --- | --- |
| `working` | 🤖 | a turn is in flight | the next event on that pane |
| `done` | ✅ | the turn ended cleanly | you look at the window |
| `error` | ❗ | the turn aborted: API error, context overflow, unparseable tool call | you look at the window |
| `waiting` | 💬 | blocked on you: permission prompt, plan mode, a question, an idle nag while it is still blocked | you look at the window |

If one window contains multiple agents, the most demanding status is shown: `waiting` > `error` > `done` > `working`.

An agent runs several things at once, so its events arrive interleaved. Within one pane the glyph
keeps the most important state you have not seen yet, `error` > `done` > `waiting` > `working`, and
ignores a lower one until you look at the window or type the next prompt. A tool call finishing in
parallel cannot hide an open permission prompt. The flip side: if the agent asks for something after
a turn you have not looked at, the entry keeps its ✅, but the bell still rings.

For windows with no agent this tool is a no-op.

The glyphs are emoji, so they survive a font change. They need a tmux client in UTF-8 mode; a client
without it renders them as underscores.

## Compatible agents
Any agent that allows to hook on to lifecycle events work.
The [docs/agents](docs/agents/README.md) shows this for various agents (manual config, drop-in file or plugin), including [Claude Code](docs/agents/claude-code.md).

## Install
Installation consists of 4 steps:
1. Install the `tmux-agent-status` cli
2. include the `tmux-agent-status.conf` into your tmux config to hook onto tmux's events.
3. include the `agent_status` placeholder in your tmux's window status format.
4. register the `tmux-agent-status` commands as agent hooks.

**1. Install the `tmux-agent-status` cli**
See [docs/install.md](docs/install.md) on how to install the helper command line tool (nix flake input, `nix profile`, a prebuilt binary, `cargo`, and building from source).

**2. Source the tmux snippet.**
It ships at [`share/tmux/tmux-agent-status.conf`](./share/tmux/tmux-agent-status.conf) in the package.
Its location on your disk depends on your installation method; see the [installation path guidance](docs/install.md#choose-installation-paths).
Import it from your tmux configuration using the path where you installed it.
For example, if you placed it in `~/.tmux`:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

**3. Include the glyph term in your tmux window format.**
Paste the following format term into **both** `window-status-format` and `window-status-current-format`, at a place you like.

```tmux
#{?@agent_status, #{@agent_status},}
```

We recommend after the name segment (outside any truncation you have) and before the window flags.
For example:

```tmux
set -g window-status-format '#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever you already had.

**Optional: terminal & tmux bell and the tmux highlight.**
When a turn ends (`waiting`, `error` and `done`, but not `working`) the tool writes a bell (`\a`) to
its tmux pane. Four tmux settings decide what that bell becomes. tmux's own defaults already do the
right thing, so this is only worth a look if you (or your tmux config framework) changed them:
check with `tmux show-options -g bell-action`.

| Setting | What it decides | Suggested |
| --- | --- | --- |
| `monitor-bell` | Whether tmux notices the bell at all. `off` means no highlight, and nothing reaches your terminal either. | `on` (tmux default) |
| `bell-action` | Which windows may pass a bell on to your terminal. `other` ignores the window you are currently on, which is exactly where an agent finishes while your terminal sits behind another tab, desktop or monitor. `any` passes those on too. | `any` (tmux default) |
| `visual-bell` | Whether the bell stays a bell. `on` replaces it with a tmux message, so your terminal never sees it. | `off` (tmux default) |
| `window-status-bell-style` | How a window that rang is painted until you visit it. It only ever applies to windows you are *not* on: tmux drops the flag of the current window immediately. | to taste |


For example:
```tmux
setw -g monitor-bell on
set -g bell-action any
setw -g window-status-bell-style 'fg=magenta,bold,nodim'
```

`bell-action` is the one worth understanding. Say your terminal has two tabs: you are working in one,
and the other holds a tmux session whose **current** window is running an agent. When that agent
finishes, `other` throws the bell away, because for tmux that window is the one you are "on".
Only `any` passes it out to the terminal, so only `any` lights up the other tab.

The price of `any` is that every other bell from the window you are on reaches the terminal too: a
shell completion beep, vim hitting the end of a search, and an agent finishing in the window you are
already watching.

What the terminal does with the bell is then up to the terminal. Ghostty, for example, prefixes the
tab title with 🔔 and asks for your attention while it is unfocused, and stays silent unless you
enable a sound in `bell-features`.

So the bell tells you *which tab*, and the glyph tells you *which window*.

**4. Register the agent hooks.**

Follow [docs/agents/README.md](docs/agents/README.md) for instructions to watch your agent of choice.

Depending on the agent, this can be done manually, by copying a file, or installing a plugin.

## Validation

Now you should be ready to go.
If you want to verify the parts, that can be done as follows.

To verify the tool is installed and on your path:
```sh
tmux-agent-status --version
```

The tmux hooks can be confirmed with:
```sh
tmux show-hooks -g | grep tmux-agent-status    # session-window-changed
tmux show-hooks -gw | grep tmux-agent-status   # window-pane-changed
```

The tmux window status glyph rendering can be verified by setting a glyph by hand in tmux: `tmux set-option -w @agent_status ✅`, then `tmux set-option -w -u @agent_status`.

## How it works

The `tmux-agent-status` executable is called from your coding agent's lifecycle hooks.
It writes a tmux option per pane indicating the agent status, summarizes the states of all panes to a single glyph on the window, and rings the terminal bell on the states that end a turn.

We use two tmux options, separating status from final glyph:

- **`@agent_pane_status`**, per pane, holds the state name of the agent in that pane. Written from `$TMUX_PANE` by `set`.
- **`@agent_status`**, per window, holds the glyph. The maximum by rank over that window's panes,
  recomputed after every write. This is what's interpolated in the format string.

They have different names, as tmux option inheritance uses the window properties as fallback for pane properties.

To clear the status, we use two tmux hooks, both calling `tmux-agent-status clear-window <pane>`, as can be seen in [`share/tmux/tmux-agent-status.conf`](./share/tmux/tmux-agent-status.conf).

Therefore, switching to a window, or to another pane inside it, drops that window's `waiting`, `error` and `done`;
`working` survives, because otherwise an agent you glance at would go blank while it is still running.

A turn that ends on the window you are **already** watching is cleared on the spot: the bell rings and no glyph appears,
because you are looking at the pane that would have explained it.

Watched means the window is the current window of a session with a client attached. So a turn that
ends while you are **detached** keeps its glyph: re-attaching does not clear it, and it is still on
the window entry when you get back, until you switch window or pane. That makes the glyph the one
signal that survives a reconnect, where the bell had nobody to reach.

A terminal window sitting behind another tab, desktop or monitor is the case tmux cannot see: the
client is attached, so tmux says you are watching, and the glyph is cleared on the spot. There the
bell is your only signal, and it reaches you only with `bell-action any`
(see [the bell settings](#install) and [known limits](#known-limits)).

## The bell, and colour

For the end states (`waiting`, `error` and `done`, thus not `working`) a terminal bell (`\a`) is printed.
With `monitor-bell on`, tmux gives you the window highlight, in whatever way you configure it, and
`bell-action` decides whether the bell also reaches your terminal: see the table under
[step 3 of the install](#install).
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

The tool deliberately stays as independent and small as possible. It doesn't require any daemon processes, nor
spawns subprocesses. It should also not interfere with your other agent or custom tmux configuration.

The only footprint within tmux are the two variables `@agent_pane_status` and `@agent_status`.

Then you can use the format string in a way you like.

## Disabling

Set `TMUX_AGENT_STATUS_DISABLED=1` to make the cli a no-op that always exits 0.
No tmux options are written, no bell rings, the binary returns success immediately.
Actually, any non-empty value counts; the documented spelling is `=1`.

This can be useful for CI, demo recordings, nested test sessions, or any environment where the agent hooks fire but you do not want the glyphs.

## Known limits

- An agent that dies without firing `Stop` or a session-end event keeps 🤖 until the next agent
  starts in that pane. We're planning a future `stale` 💤 state that decays from `working` after a
  timeout.
- A **zoomed** pane's siblings are hidden, but tmux still calls the whole window watched: their
  states clear when you look at the window, and a turn ending in a hidden sibling while you watch
  leaves no glyph. The bell is all you get, and only with `bell-action any`.
- The same goes for a terminal window behind another tab, desktop or monitor: the client is
  attached, so tmux says you are looking. We're planning to read the client's focus flag so those
  keep their glyph, which will need `focus-events on` in your tmux config.
- Creating, splitting or closing a pane counts as looking at that window, so it clears the window's
  non-sticky states.
- Only agents that can push lifecycle events get a glyph at all. An absent glyph means "no signal".

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development environment, testing against your real config, and release checks.

## Licence

MIT.
