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

The [docs/agents](docs/agents/README.md) pages give the drop-in file or manual
config for each supported agent, [Claude Code](docs/agents/claude-code.md)
included. Claude Code has a plugin that carries the hook set for you; see
[step 4](#claude-code).

## Install

See [docs/install.md](docs/install.md) for the nix flake input, `nix profile`, a prebuilt binary,
`cargo`, and building from source.

After installing the command line tool, there are three things left:
 - include the `tmux-agent-status.conf` into your tmux config to hook onto tmux's events.
 - include the `agent_status` placeholder in your tmux's window status format.
 - register the `tmux-agent-status` commands as agent hooks.

## Set up

**1. Check the binary.**
After installing, verify the binary is available: `tmux-agent-status --version` should print the version and the executable that is actually running.

**2. Source the tmux snippet.**
It ships at `share/tmux/tmux-agent-status.conf` in the package [here](./share/tmux/tmux-agent-status.conf).
Import it from your tmux configuration using the path where you installed it. For example, if you placed it in `~/.tmux`:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The snippet may live elsewhere; see the [installation path guidance](docs/install.md#choose-installation-paths).

It only adds two tmux hooks. Both call `tmux-agent-status clear-window <pane>`; the optional pane argument defaults to `$TMUX_PANE` for manual calls.
Confirm with `tmux show-hooks -g | grep tmux-agent-status` and `tmux show-hooks -gw | grep tmux-agent-status`: they sit in different scopes, so one command shows only one of them.

**3. Paste the format term.**
Into **both** `window-status-format` and `window-status-current-format`, after the name segment (outside any truncation you have) and before the window flags:

```tmux
#{?@agent_status, #{@agent_status},}
```

For example:

```tmux
set -g window-status-format '#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever you already had. Confirm it renders by setting a glyph by hand:
`tmux set-option -w @agent_status ✅`, then `tmux set-option -w -u @agent_status`.

If you want to highlight or colour the window title when the bell has rung, add something like this to `~/.tmux.conf`:
```tmux
setw -g monitor-bell on
set -g bell-action other
setw -g window-status-bell-style 'fg=magenta,bold,nodim'
```
The above example:
1) monitors the bell to highlight windows
2) only highlights the non-active windows (=other)
3) specifies which formatting should be applied.

**4. Register the agent hooks.**

Claude Code has a plugin, for other agents manually edit its configuration.

### Configure agents

#### Claude Code

There are two options, either the plugin or manually editing the hooks.

**Using the plugin.**

```
/plugin marketplace add gerbenoostra/tmux-agent-status
/plugin install tmux-agent-status
```

Restart the session and the eight hooks below are live.

You can uninstall/revert using:
```
/plugin uninstall tmux-agent-status
/plugin marketplace remove tmux-agent-status
```

The plugin only contains the hook configuration. Your `~/.claude/settings.json` will be untouched, except
for the `enabledPlugins` and `extraKnownMarketplaces` by Claude Code.

The plugin also ships `/tmux-agent-status:doctor`, a read-only check of all four setup steps.

**Manual config edit.**
[`plugins/tmux-agent-status/hooks/hooks.json`](./plugins/tmux-agent-status/hooks/hooks.json) is the
required hook definition in the shape of claude's `settings.json`. It ships as
[`share/agents/claude-code/hooks.json`](./share/agents/claude-code/hooks.json) too, which is what an
installed user has without a checkout - a symlink onto the same file here, a real file once packaged.
**Merge its `hooks` object into** `~/.claude/settings.json`: if you have no `hooks` key, take the file whole; if you already
have one, add these eight events inside it. Do not append the file as a second top-level object, and
do not end up with two `hooks` keys - JSON's last one silently wins and the hooks you had are gone.

**The watched events**
These are the hooks being watched:

| Event | Matcher | Command |
| --- | --- | --- |
| `SessionStart` | `startup\|resume\|clear\|fork` | `reset` |
| `SessionEnd` | all | `finish` |
| `UserPromptSubmit` | all | `set working` |
| `PostToolUse` | all | `set working` |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `set waiting` |
| `Notification` | all, **not** narrowed | `set waiting` |
| `Stop` | all | `set done` |
| `StopFailure` | all | `set error` |

`SessionStart` and `SessionEnd` do not ring or report a turn. The former clears this pane's previous
status, while the latter resolves the ending session. `Notification` must **not** be narrowed to
permission prompts and to also catch idle events that indicate "still blocked, and has been for
a while".

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
With `monitor-bell on`, tmux gives you the window highlight, in whatever way you configure it.
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
