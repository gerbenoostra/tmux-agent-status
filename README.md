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
Basically any agent that can hook into lifecycle events works.
[docs/agents](docs/agents/README.md) gives an overview of the agents `register` can configure.

## Requirements

tmux 3.0 or newer.

## Setup

Setup has two steps:

1. [Install the CLI](docs/install.md) with the one-line installer, Nix, a prebuilt binary, Cargo, or
   from source.
   Simplest approach:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/gerbenoostra/tmux-agent-status/main/install.sh | sh
   ```

2. Register the hooks, which configures tmux and your detected agents:

   ```sh
   tmux-agent-status register
   ```
   Its behavior, the manual configuration route, optional bell and colour settings, and how to validate the result are covered in [docs/register.md](docs/register.md).

## How it works

The flow is:
1. Your coding agent's lifecycle hooks call the `tmux-agent-status`.
2. It writes a tmux option per pane indicating the agent status (`error`, `done`, `waiting`, or `working`)
3. All pane states are summarized into a single single glyph on the window (`error` > `done` > `waiting` > `working`)
4. It rings the terminal bell on any states that ends a turn (`done`, `error`, `waiting`).
5. When you switch to the tmux window, the non-sticky pane states are reset (only `working` stays).

We use two tmux options, separating status from final glyph:

- **`@agent_pane_status`**, per pane, holds the state name of the agent in that pane. Written from `$TMUX_PANE` by `set`.
- **`@agent_status`**, per window, holds the glyph. The maximum by rank over that window's panes,
  recomputed after every write. This is what's interpolated in the format string.

They have different names, as tmux option inheritance uses the window properties as fallback for pane properties.

To clear the status, we use two tmux hooks, both calling `tmux-agent-status clear-window <pane>`, as can be seen in [`share/tmux/tmux-agent-status.conf`](./share/tmux/tmux-agent-status.conf).

Therefore, switching to a window, or to another pane inside it, drops that window's `waiting`, `error` and `done`;
`working` survives, as the agent is still running.

A turn that ends on the window you are **already** watching is cleared on the spot: the bell rings and no glyph appears.

Watched means the window is the current window of a session with a client attached. So a turn that
ends while you are **detached** keeps its glyph: re-attaching does not clear it, and it is still on
the window entry when you get back, until you switch window or pane. That makes the glyph the one
signal that survives a reconnect, where the bell had nobody to reach.

A terminal window sitting behind another tab, desktop or monitor is the case tmux cannot see: the
client is attached, so tmux says you are watching, and the glyph is cleared on the spot. There the
bell is your only signal, and it reaches you only with `bell-action any`
(see [the bell settings](docs/register.md#optional-bell-settings) and [known limits](#known-limits)).

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
