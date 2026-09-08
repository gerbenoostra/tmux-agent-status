# tmux-agent-status

One glyph on the tmux window entry, telling you which agent wants you.

```
 0:notes  1:api ✅  2:refactor 🤖  3:migration 💬- 4:build*
```

`agent-status` is called from your coding agent's lifecycle hooks. It writes one tmux option per
pane, reduces the panes of a window to a single glyph, and rings the terminal bell. That is all it
does: it never touches a window name, never shells out to git, never writes a state file, never
edits your config files and never spawns a daemon.

## The four states

| State | Glyph | Means | Clears when |
| --- | --- | --- | --- |
| `waiting` | 💬 | blocked on you: permission prompt, plan mode, a question, the idle nag | you look at the window |
| `error` | ❗ | the turn aborted: API error, context overflow, unparseable tool call | you look at the window |
| `done` | ✅ | the turn ended cleanly | you look at the window |
| `working` | 🤖 | a turn is in flight | the next event on that pane |

A window showing two agents shows the one that wants you most, in that order - `working` ranks
lowest, because a pane that finished wants a look and one still grinding does not. A window with no
agent in it renders exactly as it did before you installed this.

The glyphs are emoji, so they survive a font change. They need a tmux client in UTF-8 mode; a client
without it renders them as underscores.

## Install

See [docs/install.md](docs/install.md) for the nix flake input, `nix profile`, a prebuilt binary,
`cargo`, and building from source.

## Set up, in this order

Each step is verifiable on its own, so no step is ever debugged through another.

**1. Check the binary.** `agent-status --version` prints the version and the executable that is
actually running.

**2. Source the tmux snippet.** It ships at `share/tmux/agent-status.conf` in the package. Add to
`~/.tmux.conf`:

```tmux
source-file ~/.tmux/agent-status.conf
```

It sets two hooks and nothing else. Confirm with `tmux show-hooks -g | grep agent-status`.

**3. Paste the format term.** Into **both** `window-status-format` and
`window-status-current-format`, after the name segment (outside any truncation you have) and before
the window flags:

```tmux
#{?@agent_status, #{@agent_status},}
```

For example:

```tmux
set -g window-status-format '#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever you already had - this tool does string surgery on nobody's format.
Confirm it renders by setting a glyph by hand:
`tmux set-option -w @agent_status ✅`, then `tmux set-option -w -u @agent_status`.

**4. Paste the agent hooks.** For Claude Code, in `~/.claude/settings.json`:

| Event | Matcher | State |
| --- | --- | --- |
| `UserPromptSubmit` | all | `working` |
| `PostToolUse` | all | `working` |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `waiting` |
| `Notification` | all, **not** narrowed | `waiting` |
| `Stop` | all | `done` |
| `StopFailure` | all | `error` |

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

You paste this yourself: a tool that rewrites a hand-maintained settings file it did not write can
corrupt it, and this one will not go near it.

Two things to know here. `Notification` must **not** be narrowed to permission prompts: the idle nag
is precisely the event meaning "still blocked, and has been for a while". And the setter rings the
bell itself, so any standalone `printf '\a'` hooks for the same events should be removed rather than
kept alongside.

Confirm the first turn by reading the option back, not by looking at the status bar:

```sh
tmux display-message -p '#{@agent_status}'
```

An agent hook that cannot find `agent-status` exits 0 and says nothing - that is the failure policy,
and it means a wrong `PATH` fails invisibly.

## How it works

Two tmux options, and they cannot be merged into one:

- **`@agent_pane_status`**, per pane, holds the state name. Written from `$TMUX_PANE` by `set`.
- **`@agent_status`**, per window, holds the glyph. The maximum by rank over that window's panes,
  recomputed after every write. The only thing the format string reads.

They have different names because tmux option inheritance makes an unset pane option read back as
the window's value, so a rollup stored under the name it reduces could no longer tell a pane with no
state from a pane inheriting the rollup.

Each event costs three tmux invocations and no file writes, because `set` runs on every tool call.

## The bell, and colour

The bell is the highlight channel. `waiting`, `error` and `done` ring; `working` does not, because
it fires on every tool call and a bell per tool call is not a signal. With `monitor-bell on`, tmux
gives you the window highlight for free and paints the glyph along with it, which is why this tool
emits no colour of its own.

If you want `error` to stand out further, the shipped snippet carries an opt-in one-liner for it, as
a comment.

## Interoperability

`@agent_pane_status` and `@agent_status` are this tool's entire tmux footprint, so anything owning a
different option prefix can stay installed alongside it. The one shared resource is the format
string, and this tool only ever appends one term to it - which you paste, so nothing surprises you.

## Known limits

- An agent that dies without firing `Stop` leaves a permanent 🤖. A `stale` 💤 state that decays
  from `working` after a timeout is designed but deliberately not in the first version.
- A **zoomed** pane's siblings are genuinely hidden, and looking at the window still clears them.
- Creating or splitting a pane counts as looking at that window, so it clears the window's
  non-sticky states. You are looking at the window when you split it, so this is right more often
  than not - but a state set in the same breath as a new pane can lose the race and be cleared.
- Only agents that can push lifecycle events get a glyph at all. An absent glyph means "no signal",
  which is different from a wrong one.

## Development

```sh
nix develop          # cargo, clippy, rustfmt, rust-analyzer, tmux, just
just check           # fmt-check + lint + test, exactly what CI runs
just harness         # a throwaway tmux server showing all four states, to look at
just link            # shadow the installed binary with this checkout's debug build
```

`just link` is the edit loop: a symlink in `~/.local/bin` shadows the installed binary, so every
`cargo build` is live on the next hook fire, with no rebuild and no sudo. The shadow is invisible,
which is what `--version` printing the resolved path is for. `just unlink` reverts it.

The design lives in `tasks/plans/`. Read 001 before changing behaviour.

## Licence

MIT.
