# 001 - agent-window-status: agent events as a tmux window icon and highlight

Status: partly implemented - 002's skeleton, 003's follow-ups and 007's session boundaries ship the
four states, the rank, the rollup, immediate and hook-driven clear-on-focus behavior, recovery from
stranded `working`, the bell and the format term. Still to come: the config file, `stale`, and agents
beyond Claude Code.

## Purpose

Make the tmux window list answer "which agent needs me" without switching windows and without a
dashboard being open. One glyph and one colour per window, driven by agent lifecycle events.

## Vision

The window status should be independent from window name or highlighting.
The tool should **only** add a status glyph/icon.

## Scope boundary

| In | Out |
| --- | --- |
| agent hooks -> a per-pane state | inventorying sessions across worktrees |
| the per-window rollup across agent panes | window **names** - it never reads or writes one |
| the `@agent_*` tmux option namespace | the rest of the `window-status-format` string |
| icon set, colour highlight, auto-clear | git, worktrees, branches - it never shells out to git |
| one appended term in the status format | a store, a daemon, or polling |
| | the `pane-died` / rename guard (belongs to whatever owns window names) |
| | resolving state for agents that publish nothing |

## What exists today, and what is wrong with it

A common setup uses agent hooks that `printf '\a'` to the tty, plus tmux's
`monitor-bell` turning that into a highlighted window entry. The bell is genuinely good and is
kept - it reaches you through the terminal even when you are not looking at the status bar, which a
tmux window option can never do, and its window highlight is the one this tool builds on rather than
competes with (see the bell section below).

Two things it cannot do:

1. **One state, not three.** A bell says "something happened". It cannot distinguish *blocked on
   you* from *finished* from *still grinding*, which is exactly the distinction that decides
   whether to switch windows.
2. **Transient.** It is terminal decoration, gone the moment it fires. Nothing can be queried
   later and nothing renders in the window list.

## Features and expected behaviour

### Four states, ranked, with sticky and auto-clearing semantics

| Rank | State | Default icon | Meaning | Clears on |
| --- | --- | --- | --- | --- |
| 4 | `waiting` | 💬 | blocked on the human: permission prompt, plan mode, AskUserQuestion, idle nag | focusing the window |
| 3 | `error` | ❗ | the turn aborted: API error, context overflow, unparseable tool call (`StopFailure`) | focusing the window |
| 2 | `done` | ✅ | the turn ended cleanly | focusing the window |
| 1 | `working` | 🤖 | a turn is in flight | the next state event on that pane |

The rank is only used to reduce several agent panes in one window to one glyph; see the rollup
below. It is **not** the order in which two events on the *same* pane win: a later event may not
demote a state nobody has seen yet, and that order is `error` > `done` > `waiting` > `working`. See
013, which also supersedes the `Notification` row of the hook table below. `error` is split out of `waiting` rather than folded into it, because "it stopped because it
broke" and "it stopped because it needs an answer" call for different reactions.

`working` is deliberately **sticky** - it must not clear on focus, or an agent you glance at goes
blank while still running. The other three auto-clear via a clear-on-focus hook, and clear
immediately if the pane is already focused, so a `done` on the window you are already watching
never renders at all. **Focused means a client is attached.** tmux calls a detached session's
current window active, but nobody is looking at it, so a turn that ends while you are away must
leave its glyph to be seen on the next attach. An attached client whose terminal window is hidden
is a further case, and is 008.

**A fifth state, `stale` 💤, comes later - not in the first version.** Sticky `working` has one
failure mode: an agent that dies without firing `Stop` or `StopFailure` leaves a permanent 🤖.
The fix is to decay `working` to `stale` after a timeout, which needs a timestamp stored beside the
status and a format conditional on its age - `#{?#{>:#{t:...}}}` arithmetic the status bar can only
just about do. It ranks **below `working`**: a window that has gone quiet wants you less than one
still producing output, and less than every state that ends a turn. Shipping it needs the timeout
and the arithmetic to both be worth it, so the first version ships four states and a known permanent
🤖 on a crashed agent.

### Multiple agents in one window: per-pane state, per-window rollup

Two agent panes in one window is a normal case, not an edge case, and one window entry has room for
exactly one glyph. A single window option cannot express it, and tmux's option inheritance makes
the obvious fix silently wrong. Verified on a throwaway server with
`window-status-format` set to `#I:#W[#{@agent_status}]`:

| Setup | What renders / reads back | Why |
| --- | --- | --- |
| window option set, no pane options | the window value | ordinary lookup |
| + a same-named option on the **active** pane | the pane value | a pane option shadows the window one |
| + a same-named option on a **non-active** pane | still the active pane's | the format only ever sees the active pane |
| `list-panes -F '#{@agent_status}'` on a pane with no pane option | the **window** value | inheritance leaks: "unset" is unreadable |

The last row is the one that bites: a rollup cannot be stored in the same option name it reduces,
because the reader can no longer tell a pane with no status from a pane inheriting the rollup. So
two names:

- **`@agent_pane_status`** - per pane, written from `$TMUX_PANE`. The only thing an event writes
  directly, and never referenced by the format string.
- **`@agent_status`** - per window, the **maximum by rank** over that window's panes, recomputed by
  the setter after every write. The only thing the format reads.

`working` is lowest on purpose. The glyph answers "does this window want me": a pane that finished
wants a look, one still grinding does not. Combined with the focus-clear rule this composes
correctly - a window with one finished and one running agent shows ✅, and falls back to 🤖 the
moment you look at it, which is both facts in the right order.

A window with no agent pane at all has no `@agent_status`, and the format's `#{?@agent_status,...}`
renders nothing - identical to a stock format.

The reduction is small enough to stay inline. Verified against a three-pane window holding
`working`, `done` and no status:

```sh
tmux list-panes -t "$win" -F '#{@agent_pane_status}' | awk '
  BEGIN{r["working"]=1;r["done"]=2;r["error"]=3;r["waiting"]=4}
  $0!="" && r[$0]>best {best=r[$0];out=$0} END{print out}'   # -> done
```

### Focus clears the whole window, not one pane

A clear-on-focus hook clears `@agent_pane_status` on **every** pane of the window, then recomputes
the rollup. All panes of a window are on screen together, so seeing the window is seeing them;
clearing only the focused pane would leave a ✅ on a sibling pane you have already read.

**Which hook, verified while implementing 002:** `session-window-changed` (switching windows) and
`window-pane-changed` (switching panes inside one), not `pane-focus-in`. `pane-focus-in` fires only
when `focus-events` is on *and* a client is attached, so it is silent in exactly the setups that
never opted in; the other two fire in every setup and carry the newly current pane. tmux's
`run-shell` puts no `TMUX_PANE` in a hook's environment - it does expand `#{...}` in the command -
so the pane is passed to `clear-window` as an argument rather than read from the environment.

The exception is a **zoomed** pane, where the siblings are genuinely hidden. Known and accepted:
the alternative is a per-pane rule that misfires in the common case to be right in the rare one.

### The bell is the highlight; the recolour is opt-in

To not interfere with `monitor-bell` highlights, this tool does **not** color/highlight window names.

The glyph and the bell highlight compose without any work: the glyph is plain text inside the window
entry, so `window-status-bell-style` paints it along with the name.

We do not want this tool to use any inline `#[fg=…]`, it would override the bell style's foreground
for the whole entry. Verified with `capture-pane -e` against `monitor-bell on`, `bell-action other`,
`window-status-bell-style 'fg=magenta,bold,nodim'`:

| Format | Rendered | |
| --- | --- | --- |
| bell, no glyph | `^[[1m^[[35m 1:ringer!` | baseline |
| bell + glyph, no inline style | `^[[1m^[[35m 1:ringer waiting!` | **composes** |
| bell + glyph + inline `#[fg=colour214,bold]` | `^[[1m^[[38;5;214m 1:ringer waiting!` | magenta lost |
| the same, on a window that did **not** ring | `^[[1;2m^[[38;5;214m 2:quiet waiting` | bold **and** dim |


Therefore:

- **`monitor-bell` is the highlight channel.** tmux already maintains it - sticky, non-current-window
  only, cleared on visit - and it costs no format complexity at all.
- **The setter rings the BEL itself**, to `/dev/tty` (Claude Code captures a hook's stdout and would
  swallow the escape). This is what extends the existing highlight to agents that do not ring on
  their own - OpenCode, Gemini, Droid, Amp - and it lets any standalone `printf '\a'` bell hooks
  collapse into the state calls rather than sitting alongside them.
- **Every turn-ending state rings by default: `waiting`, `error` and `done`.** `done` is the
  arguable one and it rings: a finished turn is exactly the thing worth being told about from
  another window, and suppressing it would make the tool quieter than the four `printf` hooks it
  replaces. `working` does not ring, because it fires on every `PostToolUse` - a bell per tool call
  is not a signal. Which states ring is config, so a user who disagrees changes one list.
- **This tool emits no inline style by default.** Shipping the glyph term alone leaves any existing
  bell highlight exactly as it is.
- Agents like claude can also trigger bells themselves through their own config. That does not interfere
  with our bell.

The recolour survives as an **optional second snippet**, documented as opt-in and not part of the
contract. When adopted, it colours `error` to distinguish from other bell-highlight, by setting it
for example to red.
```tmux
# optional: `error` is the one state the bell cannot express.
# Inside #{...}, `#,` escapes a comma so it is not parsed as a format separator.
#{?#{==:#{@agent_status},error},#[fg=colour160#,bold#,nodim],}
```

With the snippet omitted, `error` is distinguished by its ❗ alone.

Rejected: guarding the recolour with `#{?window_bell_flag,…}` so the bell always wins. It works
(verified), but it makes the colour mean "whichever channel fired first" rather than anything about
the agent.

### The icon set, and where it renders

Emoji (🤖 💬 ❗ ✅) are the **default set**: they survive a font change, which nerdfont glyphs do
not. `nerdfont: true` is a supported setting for the users who have the font and want the tighter
monospace alignment, and a `status_icons` map overrides any individual glyph in either set - the
`error` glyph in particular is a first guess, and 💥 reads as well as ❗ at status-bar size. The
default set, the nerdfont switch and the override map are three settings, not a debate.

The glyph renders outside any `#{=/N/…:}` truncation the user already has, so a long window name can
never push it off. It sits between the (possibly truncated) label and the window flags.

### Never touches the window name, and never reads one

A window entry is `[name][ icon]`, and this tool owns only the second half. The name may come from
`automatic-rename`, from a window-labelling script, from a manual `<prefix> ,` rename, or from a
worktree tool - the same glyph is appended in all those cases and none of them is read.
`@agent_status` is a separate option in a separate segment of the format.

**The option considered and rejected**: include a richer (opt-in) entry by owning the name too.
This tool should be independent, and not interfere. Window naming is a distinct functionality.

## Design decisions

### The status format is edited once, in the user's `~/.tmux.conf`, globally

**The option**: the tool reads the global `window-status-format` itself, splices its term in and
writes the result back, so installing it costs the user no config edit at all.

**The verdict: no.** Verified on a throwaway server. Writing the spliced format back globally would
overwrite a value the user maintains, so it has to go to a **window-local** option instead - a copy
of the format, frozen at the moment that window was touched. Reloading `~/.tmux.conf` then updates
only the untouched windows; every other one keeps its frozen copy, and keeps it after the tool is
uninstalled. One line of setup is not worth a silent divergence that surfaces months later.

So: **this tool does string surgery on nobody's format.** It ships **one term**, documented for the
user to add once, appended after the name segment and outside the truncation. It emits no style of
its own (see the bell section) and makes no claim about how the name is produced:

```tmux
#{?@agent_status, #{@agent_status},}
```

Applied to **both** `window-status-format` and `window-status-current-format`. Against a format that
truncates the name, that is a single insertion between the closing `}}` of the `#{=/25/…:}`
truncation and `#{?window_flags,...}`:

```tmux
set -g window-status-format '#I:#{=/25/…:<name segment, unchanged>}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever the user already has. This tool never edits it.

Verified rendering:

```
 t 1:repo>* 2:gemini-support 🤖- 3:batch-processing 💬  4:auto-layout  5:sleep
```

The format string is the **one shared resource** between this tool and anything that produces window
names, and the segments are disjoint: the other supplies a name, this appends a glyph after it.
Neither reads the other's segment.

### The `pane-died` rename guard is out of scope

An earlier draft carried it. It does not belong here: `remain-on-exit on` plus a `pane-died` hook
that resets `automatic-rename` damages a **window name** - which this tool neither reads nor writes.
A rename does not touch `@agent_status`, so this tool is unaffected either way.

### Hook set: keep the bell, add the state, cover the abort and the dialog

The bell and the icon compose - they write to different channels (BEL vs a tmux option) and the
Claude settings merge is additive. The bell column below is emitted **by the state setter** rather
than by separate `printf` entries, so each event is one hook entry; four standalone BEL hooks in
`~/.claude/settings.json` are replaced, not kept alongside.

| Event | Matcher | Bell | State |
| --- | --- | --- | --- |
| `SessionStart` | `startup\|resume\|clear\|fork` | - | `reset` |
| `SessionEnd` | all | - | `finish` |
| `UserPromptSubmit` | all | - | `working` |
| `PostToolUse` | all | - | `working` |
| `Notification` | `permission_prompt\|elicitation_dialog\|elicitation_url_dialog\|agent_needs_input` | yes | `waiting` |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | yes | `waiting` |
| `Stop` | all | yes | `done` |
| `StopFailure` | all | yes | `error` |

The last two rows are the ones easiest to leave out, and each closes a way for the icon to lie.
Without `StopFailure`, a turn that dies on an API error, context overflow or unparseable tool call
stays 🤖 **working** forever. Without `PreToolUse` on `AskUserQuestion|ExitPlanMode`, a plan-mode
dialog sitting there waiting for you also shows **working**. `Notification` was originally left un-narrowed here, on the reasoning that its idle nag is the
event meaning "still blocked, and has been for a while". 013 reverses that, on two measurements:
the nag does not fire while a permission prompt is open, and it fires long after a turn has ended,
where the state it would replace is a `done` that now outranks it. The repeat it was kept for is
also no longer needed, since a `working` can no longer hide a `waiting`.

**Rejected as a substitute for any of the three**: a display-side filter that dims an agent idle for
over an hour. It does not correct a wrong state, it only greys it out, and the window list has
nowhere to put a second dimension anyway.

`error` rather than `done` for `StopFailure`: an aborted turn needs the human. It ranks just under
`waiting` - a question you can answer beats a turn that has already fallen over - and auto-clears on
focus the same way.

### Three tmux calls per event, and no state file

`set` runs on **every** tool use, so its per-invocation cost is a constraint on the design, not an
optimisation to reach for later. The bell hook it replaces is a single `printf`, and that is the
bar: several `tmux` invocations plus a state-file write per event is heavy enough to be felt on a
busy turn, and a status indicator that is felt has already failed.

Target: `set-option -p` on `$TMUX_PANE`, `list-panes -F` to read the window's panes, `set-option -w`
for the rollup. Three calls, no fork beyond tmux itself, no file touched. 013 lowers this to two
tmux processes - one read, then every write of the event as a single command list - and moves each
decision that depends on the current state into the formats those writes carry, because hooks of
one turn can run concurrently and a decision taken on the read can be stale by the time it lands. Pane resolution comes from
`$TMUX_PANE` (present in the hook's environment); the process-ancestry walk is the fallback, not the
path. The rollup can collapse to two calls when the new state already outranks the current
`@agent_status` and nothing needs re-reading, but that is an optimisation, not the contract.

### Never write to `~/.claude/settings.json` from a tool

**The option**: a `setup` command that merges the hook entries into the user's settings file, so
installing is one command instead of a paste.

**The verdict: no**, and this one was paid for. That file is hand-maintained, is frequently a
symlink into a dotfiles repo, and is written by the agent itself at unpredictable moments. Merging
into it means reserialising the whole document, which reorders keys unless the serialiser preserves
order, and a truncate-in-place write loses the file outright if it loses the race. A config file
frequently lives in a dotfiles repo or is edited by the agent itself, so a race or serialisation bug
can silently corrupt or empty it. A tool that can do that to a config it did not write has no
business writing it.

Consequence: **this tool never edits `~/.claude/settings.json`.** It documents the hook entries and
the user pastes them in, reviewed in a diff like any other change. If a plugin route is ever used it
must be the `enabledPlugins` mechanism, which never touches the file.

### Other agents

Of the agents surveyed, only four can push an event at all: OpenCode (eight events, richest),
Gemini (seven), Droid and Amp (`SessionStart`, and Amp documents that **no** `session.end` exists).
Each agent's vocabulary maps onto the same `{working, waiting, done}` triple with a small per-agent
table.

Everything else gets no icon. That is honest and costs nothing: an absent glyph means "we have no
signal", which is different from a wrong glyph.

## Interoperability

- **With any other status tool**: `@agent_pane_status` and `@agent_status` are the tool's entire
  tmux footprint, so anything owning a different option prefix can stay installed alongside it
  through a migration without the two fighting. The one genuinely shared resource is the format
  string, and this tool only ever appends one term to it.

## Independence

- Ships standalone. It needs only agent hooks and `~/.tmux.conf`, and it improves the status bar on
  hand-made windows with no worktree tooling at all.
- Writes only: `@agent_pane_status` pane options, the `@agent_status` window rollup, a BEL to
  `/dev/tty`, and the two clear-on-focus hooks. The user's agent hook entries are documented, never
  written.
- Ships **no name segment at all**. None is needed: the glyph term appends to whatever the name half
  renders, and `#W` is never empty - it falls back to `automatic-rename`'s process name or the
  window index.
- Never writes a window name, never writes git config, never writes a state file, never spawns a
  daemon.

## Verification plan

A throwaway-server harness, tool-agnostic:

```bash
tmux -L wmtest -f tests/fixtures/tmux.conf new-session -d -s t -c /tmp/repo
tmux -L wmhost new-session -d -s host -x 210 -y 55 "tmux -L wmtest attach -t t"
tmux -L wmhost capture-pane -p -t host   # read the rendered status bar
```

An attached client is required or `#()` jobs in the status format never run. Fake an agent pane
without running a real agent: `bash -c "exec -a claude /bin/sleep 900"` plus
`tmux select-pane -T '<title>'`, then drive the state setter with `TMUX_PANE=%N`.

The harness must verify that: the recolour prefix emits a style for `waiting` and `error` only;
an unset `@agent_status` renders byte-identical to a stock line; the reducer picks the right state
across three panes; per-pane readback is empty for a pane with no state.

Cases that must pass:

- each of the four states renders its glyph
- with no optional snippet, no state emits an inline style, and a belled window keeps its
  `window-status-bell-style` colour with the glyph rendered inside it
- with the optional snippet, `error` overrides the bell colour and every other state does not
- a quiet `error` window renders `nodim`, not bold-and-dim
- the setter's BEL raises the window's bell flag, and does not on the current window
  (`bell-action other`)
- `waiting`/`error`/`done` clear on focus, `working` does not
- a `done` on the focused window never renders at all
- a long window name truncates without displacing the icon
- a window with no agent pane renders exactly as it did before installing
- **rollup**: two agent panes in one window, every ordered pair of states, renders the higher rank
- **rollup**: the lower-ranked pane's state survives - clear the higher one and the other appears
- **rollup**: focusing the window clears every pane, not just the focused one
- **inheritance**: `list-panes -F '#{@agent_pane_status}'` prints empty for a pane with no state
  even while `@agent_status` is set on the window (the trap that forced two option names)
- reloading `~/.tmux.conf` updates **every** window (the frozen-snapshot regression test)

## Closed decisions

Each of these is specified in the section named; this is only the index.

| Decision | Outcome | Where |
| --- | --- | --- |
| Icon set | emoji by default, `nerdfont: true` supported, `status_icons` overrides either | the icon set |
| Whether the bell is dropped for events that also set `waiting` | kept, and promoted to *the* highlight channel | the bell |
| Which states ring | every turn-ending state, `done` included; `working` never | the bell |
| Whether `error` recolours | only via the optional snippet, and then it is the only state that does | the bell |
| A crashed agent's permanent 🤖 | decay `working` to `stale` 💤, ranked below `working` - **later, not the first version** | the four states |
| Implementation language | Rust | 002 |
| Repository and executable name | repo `tmux-agent-status`, executable `tmux-agent-status` | 002 |
