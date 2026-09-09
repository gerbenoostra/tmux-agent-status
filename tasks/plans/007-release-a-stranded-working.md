# 007 - Release a stranded `working` on a session boundary

Status: implemented.

Covers *how a pane that missed its turn-ending event gets its glyph back*. What the states mean and
why `working` is sticky is 001, which stays normative. How the hook set reaches a machine is 004.

## The defect

A pane whose agent never fires `Stop` or `StopFailure` keeps 🤖 **forever**, and no user action
releases it. Observed in the field: an agent finished a turn cleanly, the pane kept
`@agent_pane_status = working`, and the window entry kept 🤖 for a quarter of an hour with the
session sitting idle at its prompt.

The user then tried, in order, the three things anyone would try, and none of them worked:

1. `/clear` the session.
2. Exit the agent.
3. Start a new agent in the same pane.

That is the part that makes this a defect rather than the known limit the README already records.
The known limit says a crashed agent leaves a permanent 🤖 until a future `stale` state decays it.
It does not say the glyph survives *restarting the agent in that pane*, and a status indicator that
survives its own subject is worse than no indicator.

### Why nothing releases it today

Three clearing paths exist, and each one is closed by design:

| Path | Why it does not clear `working` |
| --- | --- |
| `set done` / `set error` | Needs the event that never fired. |
| `clear-window`, from the tmux `session-window-changed` hook | `clears()` keeps sticky states on purpose: an agent you glance at must not go blank while it is still running. 001. |
| Anything at session start or session end | There is nothing. The plugin watches six events and all six sit *inside* a turn. |

Row three is the gap. 001's hook table lists a seventh row - `SessionStart`, matcher
`startup|resume|clear|fork`, described as "register pane binding" - and 004 shipped the six that
set a state and dropped it, because it had no state to set. `tasks/todo/005-agents-beyond-claude.md`
(a todo, not a plan) already argues both boundaries are load-bearing, in its own words: session
start is a *reset* that clears a glyph left in that pane by a previous agent, and session end
resolves a lingering `working` when the agent exits cleanly, which is a partial answer to the known
limit and cheaper than the deferred `stale` state.

### What is deliberately not claimed here

The field case was a long-running agent process that predated its own plugin installation, and its
`Stop` provably did not run while its `PostToolUse` did. Running `tmux-agent-status set done` by
hand against that same pane cleared the glyph immediately, and a fresh agent session in a scratch
tmux server walked `[] → working → done` correctly, subagent included. So the tool is not what
failed, and *why that one process applied part of a hook set* is not this repository's bug to fix.

This plan fixes the consequence that is ours: a missed turn-ending event must be recoverable, by
any of the three actions above, without a timer and without the user knowing this tool exists.

## The fix

Two commands, and two hook entries that call them. The boundaries are not symmetric, so the
commands are not either: a session **starting** in a pane says nothing about the agent that was
there before, so its glyph goes; a session **ending** is itself news, so it leaves one.

### `tmux-agent-status reset`

Drop **this pane's** status whatever it holds, sticky included, then recompute the window rollup.

```
pub fn reset() -> io::Result<()>
```

- No bell. A session starting is not a turn ending; it has no news for the human.
- No glyph. It never writes a state, only removes one.
- Sticky is irrelevant here. Stickiness answers "does *looking* at the window clear this", and a
  session boundary is not looking. This is the same reasoning that made the reporting pane's own
  state superseded in 003 item 1.
- Siblings are untouched. The boundary is a fact about one agent in one pane, not about the window.
  This is the one behavioural difference from `clear-window`, which clears every pane of the window
  because all of them are on screen together.
- Idempotent, and a no-op on a pane that has no status.

`clear-window` is not reused and not widened. It means "seeing the window cleared what seeing
clears", which is exactly the rule that must not apply to `working`. Adding a flag to it would give
one command two meanings.

### `tmux-agent-status finish`

Resolve **this pane** to `done`, then recompute, exactly as `set done` does - watched-window
clearing included - with two differences.

```
pub fn finish() -> io::Result<()>
```

- **A pane already holding `error` is left alone.** ✅ is documented as "the turn ended cleanly",
  and a session ending does not make an aborted turn clean. Nothing strands: `error` is not sticky,
  so looking at the window clears it, which is the whole reason `working` needed this plan and
  `error` did not.
- **No bell.** On an ordinary exit `Stop` already rang for the same news seconds earlier, and a
  doubled bell is the one manifest mistake `tests/plugin_manifest.rs` already fails the build over.

Everything else follows the one rule *a session that has ended is finished*:

- `working` and `waiting` both become `done`. The session is neither running nor blocked on you.
- A pane with no status also gets `done`. On a window nobody is watching that is true news - this
  pane's agent finished while you were elsewhere - and on a watched window `set`'s existing rule
  clears instead of writing, so the person looking at it sees nothing new.
- Idempotent. The normal path is `Stop` → `done` (bell) then `SessionEnd` → `done` (silent), which
  reasserts what is already there.

This is the answer to "whatever way the session finished, the window says done". It is `set done`
minus the bell and minus the one case where `done` would be a lie.

### The two hook entries

```json
"SessionStart": [{ "matcher": "startup|resume|clear|fork",
                   "hooks": [{ "type": "command", "command": "tmux-agent-status reset" }] }],
"SessionEnd":   [{ "hooks": [{ "type": "command", "command": "tmux-agent-status finish" }] }]
```

Verified against the agent's own hook schema, not just its docs: `SessionStart` carries
`source: startup | resume | clear | compact | fork`, and `SessionEnd` carries
`reason: clear | resume | logout | prompt_input_exit | other`.

`SessionStart` covers actions 1 and 3 of the three that failed: `clear` is its own source value, and
`startup` fires for a newly started agent in that pane. `resume` and `fork` are included for the
same reason - each one is an agent taking over a pane whose previous glyph is not about it.

`compact` is deliberately **left out**, which is a change from the first draft of this plan.
Auto-compaction fires `SessionStart` *mid-turn*, in the same session, in the same pane. There is no
foreign glyph to clear, so the reset buys nothing, while the cost is real: the window shows no
signal at all from the compaction until the next `PostToolUse`, which can be a long tool call away.
The matcher therefore stays the `startup|resume|clear|fork` that 001's table already names.

`SessionEnd` covers action 2, and covers the ordinary clean exit that nobody thinks to check. It
takes no matcher: every reason ends the session, and the two reasons that hand the pane to a
successor session, `clear` and `resume`, are harmless. Both fire while the user is typing in that
window, so the window is watched, so `finish` takes `set`'s clear path and writes no glyph at all;
the incoming `SessionStart` then resets. The two events' order does not matter, which is why this
plan does not depend on one.

### The turn-ending events still own the bell

`Stop` → `done` and `StopFailure` → `error` are unchanged, keep their bell, and remain the only
signal that a turn ended. `finish` is the backstop that makes the *glyph* correct when they did not
run; it is not a second notification.

## What this does not fix

An agent killed outright - `SIGKILL`, a closed terminal, a machine that slept and lost the process
- fires no session-end event and leaves the 🤖 until the next agent starts in that pane. That is
the residue the deferred `stale` 💤 state of 001 exists for, and it stays deferred. The difference
after this plan is that the residue needs an *abnormal* death rather than any missed `Stop`, and
that starting an agent in the pane always clears it.

## Work

1. **`reset` and `finish` in `command.rs`.** Split the body of `set` after the bell into a shared
   write path that takes the pane and the already-read `Window`, so all three commands read tmux
   once and only the policy differs: `set` rings then writes, `finish` writes `done` unless this
   pane's entry parses as `error`, `reset` clears this pane's entry and recomputes over the window's
   panes with that entry emptied - the same in-memory correction `set` already makes, because the
   pane list is read before the write.
2. **`reset` and `finish` in `main.rs`.** Neither takes an argument; an argument is a usage error.
   Route both through `hook()` like the others: a hook must never break the agent that called it.
   Add both to `help()`.
3. **The manifest.** Add the two entries to `plugins/tmux-agent-status/hooks/hooks.json`.
4. **The README.** Add both rows to the Claude Code table. Its third column is headed `State`,
   which two of the eight rows now have no value for; rename it to **`Command`** and give every row
   the arguments verbatim - `set working`, `set waiting`, `set done`, `set error`, `reset`,
   `finish`. That keeps the table a faithful copy of what runs, which is the only property the test
   below can check. Say underneath that the two boundary events do not ring and do not report a
   turn: one clears the pane, the other resolves it. Update "the six hooks below" and "add these six
   events" to eight.
5. **`tests/plugin_manifest.rs`.** `state_of()` becomes `arguments_of()`: strip the
   `tmux-agent-status ` prefix, then accept exactly `set <one of STATES>` or exactly one of
   `["reset", "finish"]`, and panic on anything else. No sentinel value and no loosening - a typo'd
   binary name or an unknown state still fails. Update the header assertion to `Command` and
   `EXPECTED_EVENTS` to 8. The duplicate-entry check stays as is.
6. **`plugins/tmux-agent-status/commands/doctor.md`.** It tells the user the plugin registers "the
   six Claude Code events". Make it eight.
7. **The known-limits list in the README.** The first bullet currently says an agent that dies
   without firing `Stop` leaves a permanent 🤖. After this it is narrower: an agent that dies
   without firing `Stop` *or a session-end event* keeps its 🤖 until the next agent starts in that
   pane. Keep the `stale` sentence; it is still the answer for the abnormal death.
8. **001's hook table.** It is the normative list of watched events, and the manifest test's failure
   message sends the reader there first. Replace the `SessionStart` row's never-implemented
   "register pane binding" with `reset`, add the `SessionEnd`/`finish` row, `-` in the bell column
   for both, and update the status line's "still to come".
9. **`tasks/todo/005-agents-beyond-claude.md`.** Its "Session start and session end are
   load-bearing" paragraph now has a shipped reference implementation for Claude Code and stays open
   only for the other agents; its survey question "Session start / session end events?" gains the
   `reset` / `finish` pair as the two things to look for per agent. Note both there.

## Verification

Unit and CLI tests for `reset`: no pane in the environment is a silent success, an argument is a
usage error, a pane holding `working` reads back unset, a sibling holding `working` is untouched,
and the rollup recomputes rather than being cleared outright when a sibling still has a state.

Unit and CLI tests for `finish`: `working` and `waiting` and an unset pane all read back `done`,
`error` reads back `error`, an argument is a usage error, no pane in the environment is a silent
success, and the rollup follows. The bell tier gets one case: `finish` writes no BEL where
`set done` writes one.

The tmux-server tier covers the rollup after each of the two, including the two-pane case where the
window must keep the sibling's glyph.

End-to-end, in a scratch tmux server with a real agent, all three of the actions that failed, plus
the two things the fix must not break:

1. Strand a `working` on a pane (`tmux-agent-status set working`), then `/clear` in an agent
   running in that pane. The glyph goes, and no ✅ is left behind by the `SessionEnd` that `/clear`
   also fires.
2. Strand one, exit the agent. The window ends on ✅, from a window that is not the current one so
   the watched path does not hide the result.
3. Strand one, exit the agent with the boundary hook removed so the glyph survives, then start a
   fresh agent in that pane. The glyph goes.
4. A turn that ends in an abort, then an exit: the window keeps ❗.
5. A clean turn on an unwatched window, then an exit: one bell from `Stop`, not two.

This also confirms what the two entries assume about the environment, which is that both events run
in the agent's own shell and so inherit its `$TMUX_PANE`; a wrong pane would show up immediately as
a glyph on the wrong window.
