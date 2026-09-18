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
Basically any agent that allows to hook on to lifecycle events work.
The [docs/agents](docs/agents/README.md) gives an overview of all agents that have been included in auto configuration.

## Setup

Setup has two steps:

1. [Install the CLI](docs/install.md) with the one-line installer, Nix, a prebuilt binary, Cargo, or
   from source.
2. Configure tmux and your installed agents:

```sh
tmux-agent-status register
```

`register` detects your agents, previews and confirms every change, backs up every file it edits,
and checks your tmux configuration against a throwaway server - which loads your config, so whatever
it runs (`run-shell`, `if-shell`, a plugin manager) runs there too. If it cannot edit a generated or
read-only file safely, it prints the change for you to apply instead.

Start with `tmux-agent-status register --dry-run` to inspect the plan without changing anything.
Use `tmux-agent-status register --help` for step selection, non-interactive operation, and target
overrides.

## Manual configuration

`register` is the recommended route. If you prefer to manage every file yourself, configure the
same three parts manually.

### Tmux hooks

Copy [`share/tmux/tmux-agent-status.conf`](./share/tmux/tmux-agent-status.conf) somewhere readable
by tmux, then source that path from your tmux configuration. For example:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The package-specific locations of the shipped file are listed in the [manual file reference](docs/install.md#manual-file-reference).

### Tmux window format

Add this term to **both** `window-status-format` and `window-status-current-format`:

```tmux
#{?@agent_status, #{@agent_status},}
```

Place it after the name segment, outside any truncation, and before the window flags. For example:

```tmux
set -g window-status-format '#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever you already had.

### Agent hooks

Follow the [agent setup guide](docs/agents/README.md) and the linked page for your agent.
Depending on the agent, manual delivery means installing a plugin, copying a drop-in file, or merging settings.

### Optional bell settings

When a turn ends (`waiting`, `error`, or `done`, but not `working`), the tool writes a bell (`\a`)
to its tmux pane. tmux's defaults already do the right thing, so these settings matter only if you
or your tmux framework changed them. Check with `tmux show-options -g bell-action`.

| Setting | What it decides | Suggested |
| --- | --- | --- |
| `monitor-bell` | Whether tmux notices the bell at all. `off` means no highlight, and nothing reaches your terminal either. | `on` (tmux default) |
| `bell-action` | Which windows may pass a bell to your terminal. `other` ignores the window currently selected in tmux; `any` also passes bells from that window. | `any` (tmux default) |
| `visual-bell` | Whether the bell stays a bell. `on` replaces it with a tmux message, so your terminal never sees it. | `off` (tmux default) |
| `window-status-bell-style` | How a window that rang is painted until you visit it. | to taste |

For example:

```tmux
setw -g monitor-bell on
set -g bell-action any
setw -g window-status-bell-style 'fg=magenta,bold,nodim'
```

`bell-action any` is useful when the tmux window running the agent is current inside a terminal tab
that is itself hidden. The trade-off is that other bells from that window also reach your terminal.
What the terminal does with the bell is up to the terminal. The bell tells you *which terminal tab*;
the glyph tells you *which tmux window*.

### The bell, and colour

For the end states (`waiting`, `error` and `done`, thus not `working`) a terminal bell (`\a`) is printed.
With `monitor-bell on`, tmux gives you the window highlight, in whatever way you configure it, and
`bell-action` decides whether the bell also reaches your terminal: see the
[optional bell settings](#optional-bell-settings).
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

## Validation

Now you should be ready to go.
If you want to verify the parts, that can be done as follows.

`tmux-agent-status register --dry-run` reports the configuration state without changing anything, which is the quickest check.

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
(see [the bell settings](#optional-bell-settings) and [known limits](#known-limits)).

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
