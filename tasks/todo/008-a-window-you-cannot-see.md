# 008 - A window you are "on" but cannot see

Status: todo - nothing implemented. Split out of 006, which keeps the bell half of the same case.

Covers *the glyph for a window tmux calls watched and nobody can see*: the terminal is behind
another tab, desktop or monitor, or the pane is behind a zoom. The states, the rank and the rollup
are 001 and do not change here. The bell for the same case is 006, and lands independently.

## The case

003 taught `set` to clear rather than write when the window is watched, where watched is
`window_active && session_attached > 0` (`is_watched` in `src/tmux.rs`). Both terms hold when the
terminal is hidden, and both hold for a pane hidden behind a zoom, so the turn that ended while you
were in another tab, or in another pane of a zoomed window, leaves nothing behind. Only the bell
survives, and 006 is why even that is currently swallowed.

Both cases are documented as known limits in the README today. This task replaces the documentation
with behaviour, for the users who opt in.

## What tmux actually reports

Measured on tmux 3.6a, macOS, throwaway `-L` servers with `-f /dev/null`. The negative results are
listed because two of them decide the design.

| Question | Answer |
| --- | --- |
| `#{client_flags}` in `list-panes -F` | **Empty.** Client formats have no client in that format tree, so the focus answer cannot ride along on the line the tool already reads. |
| A focus-aware window or session format | **None.** `#{window_active_clients}` counts clients viewing the window, not focused ones; `#{session_attached_list}` yields ttys. Focus costs a second tmux call. |
| `list-clients -t <pane id>` | Works: a pane target resolves to its session. `#{client_flags}` is a comma-separated list, e.g. `attached,focused,UTF-8`. |
| Does `focused` mean "someone is looking"? | Only if the terminal reports focus. The flag is **optimistic**: a client whose terminal never sends a focus-out reads `focused` forever. Verified with a tmux client attached inside a *background* window of another tmux - definitively not on screen - reading `attached,focused,UTF-8` with `focus-events` both off and on. |
| Does it work with a real terminal? | **Yes.** Ghostty with `focus-events on`: of five attached clients, exactly the one whose terminal window was on screen read `focused`. |
| `focus-events` | Default **off**. tmux(1): focus events are requested from the terminal only when it is on, and "attached clients should be detached and attached again after changing this option". |
| `#{window_zoomed_flag}`, `#{pane_active}` in `list-panes -F` | Both render per pane, and while zoomed the zoomed pane is the active one. The zoom answer is free on the line the tool already reads. |
| `window-pane-changed` on a zoom toggle | Does not fire. |
| `after-resize-pane` | Fires on zoom **and** unzoom, with `#{pane_id}` and a `#{window_zoomed_flag}` that is already correct at hook time (1 on zoom, 0 on unzoom). Not fired by `resize-window`. |
| `select-pane` while zoomed | Unzooms, and fires `window-pane-changed` with the flag already 0, so the shipped hook already covers that path. The new hook is only needed for an explicit unzoom (`prefix-z`). |
| `client-focus-in` on attach | Does not fire. It fires only on a real focus report from the terminal, so it is silent - not merely harmless - when `focus-events` is off. |

**The failure direction, corrected.** An earlier draft of this task expected that a terminal which
never reports focus would yield no focused client, so the tool would always write the glyph. The
measurement says the opposite: every client reads focused, so the tool always clears and behaves
exactly as it does today. That is still a safe direction to fail in, but it means the feature is
opt-in behind `focus-events on` and the README has to say so. Without that option, Done when 1
cannot be met.

## The rule

One predicate, used by `set` and by `clear-window`. Two predicates, and a state cleared by one is
kept by the other.

```
seen(pane) = window_active
          && session_attached > 0
          && (!window_zoomed_flag || pane_active)
          && some client of that session is focused
```

The first three terms are per pane and free: they are four fields on the `list-panes` line the tool
already reads. The fourth is per invocation and costs one `list-clients`.

The rollup does not change: still the maximum by rank over the window's panes. Only which panes
clear changes.

**The bell obeys the same rule.** Today `set` rings before it reads tmux at all, and 001 argued that
a finished turn deserves the bell even when you are looking. That was written when the only
suppression available was tmux's `bell-action other`, which drops the bell for the whole current
window and is the defect 006 fixes. Once `seen` exists, the tool can suppress at the source, and
more precisely than tmux can: tmux knows nothing about focus or zoom. So the glyph and the bell
become one signal on two channels, and both fire exactly when the pane is unseen. Precisely:

| Situation | Bell |
| --- | --- |
| Pane seen | silent. You are looking at the pane that would explain it. |
| Pane unseen | rings, as today. |
| No tmux, or no pane resolved | rings. Unchanged: the bell is a separate channel and does not depend on there being a tmux to write to. |
| The tmux read failed | rings. Fail towards the behaviour of every version before this one. |

## What you observe, and why it is worth writing

`working` 🤖 is sticky: always written, never rings, never cleared by looking, so it renders on the
window you are on as well. Everything below is a turn-ending state (`done` ✅, `error` ❗,
`waiting` 💬). Measured on 3.6a, including the two nested-tmux experiments this task's numbers come
from.

| # | Where you are | Glyph | tmux `!` and bell style | BEL out to the terminal | What that gets you |
| --- | --- | --- | --- | --- | --- |
| 1 | Looking at the window, pane visible | none | none | none | Nothing on the status bar and no sound: you are reading the pane that would have explained it. |
| 2 | Looking at the window, pane behind a zoom | ✅ | none | rings | The one signal for a sibling agent you cannot see. Clears on unzoom or on any pane or window switch. |
| 3 | That window is current, terminal on another tab, desktop or monitor | ✅ | none - tmux drops the flag because a client is "on" that window | yes, once 006 sets `bell-action any` | The terminal's own marker asks for the tab (on Ghostty defaults a 🔔 on the title, no sound), the glyph says which window once you are there. `client-focus-in` clears the glyph on return, with no window switch. |
| 4 | Another tmux window of that session | ✅ | `!` and the bell style | rings | Two durable signals plus the sound. Both clear when you switch to that window. |
| 5 | Session fully detached | ✅ | flag set, nothing renders it | no client, so nowhere to send it | See below. |
| 6 | Agent not inside tmux | none | none | rings | The pre-existing bell-only behaviour, unchanged. |

Row 3 is the case this task exists for, and row 1 is the reason the rule has to be exact: getting
`seen` wrong in that direction means a glyph on every window you are working in.

**Row 5, the detached session, is worth writing even though nothing can be observed at the time.**
Measured: while detached, both the bell flag and the glyph are stored. On re-attach tmux clears the
bell flag of the window you land on, keeps it on the others, and **none of the shipped hooks fire**,
so the glyph survives the attach. So the glyph is the only channel that carries a detached
session's result back to the user, and it does so by doing nothing special: you re-attach and the
entry still says what happened while you were away, until your first window or pane switch or
focus-in. This is also the standing argument against adding a `client-attached` clear hook: it would
delete the one signal this row has.

## Decided: the focus call is lazy

The focus answer only ever changes what a code path about to **clear** does, so only those paths pay
for it:

| Path | Calls |
| --- | --- |
| `set working` (sticky, every `PostToolUse`) | 3, unchanged. A sticky state never clears, so it never asks. |
| `set done`/`error`/`waiting` on a pane the free terms already call unseen | 3, unchanged. It rings and writes the glyph either way. |
| `set done`/`error`/`waiting` on a pane the free terms call seen | 4. Asks once, then clears or writes. |
| `clear-window` (the hooks) | +1. It exists to clear, so it always asks. Window switches and focus changes are human-paced and the hooks run `run-shell -b`. |

The answer is memoized for the invocation; it cannot change inside one.

**Reading a failure.** `is_watched` reads an unparseable flag as "not watched", which costs the
immediate clear and nothing else. Focus is the mirror image: no clients listed is a real answer and
means not focused, but a `list-clients` that fails or cannot be read means **focused**, because that
is precisely the behaviour of every version before this one and nobody can observe the error - a
hook exits 0 whatever happens.

## Work

1. **`src/tmux.rs`, the pane line.** Extend the format to
   `#{pane_id}\t#{@agent_pane_status}\t#{window_active}\t#{session_attached}\t#{window_zoomed_flag}\t#{pane_active}`
   and keep taking the pane from the left and the flags from the right, so a status containing tabs
   still survives: the existing `rsplit_once` chain grows by two. `PaneStatus` gains
   `on_screen: bool` - the first three terms of the rule - and `Window.watched` goes away.
   This reverses 003's second-pass item 2, and the comment at `src/tmux.rs:27-33` arguing that being
   watched is a fact about the window, not about any one pane. Zoom is what makes it a pane fact.
   Replace that comment with the new reason; do not leave both standing.
2. **`src/tmux.rs`, the focus fact.** `pub fn any_client_focused(target: &str) -> io::Result<bool>`,
   running `list-clients -t <target> -F '#{client_flags}'` and returning true when any line has
   `focused` as a comma-separated **element**, not as a substring. Empty output is false. The module
   keeps knowing tmux and not policy: the failure reading above belongs in `command.rs`.
3. **`src/command.rs`, one predicate.** A small helper holding the memoized focus answer, with
   `seen(&mut self, pane: &PaneStatus) -> bool` as the only place the rule is written down.
   `write_status` tests the reporting pane with it; `clears()` gains "and the pane is seen".
   An `io::Error` from the focus call is caught here and read as focused. The superseded-pane rule
   from 003 is unchanged: the pane an event arrived on loses its old state whatever it held.
4. **`src/command.rs`, the bell moves.** `set` currently rings first thing, before it knows anything
   about the world. It now rings after the pane and the window are resolved, and only when the pane
   is unseen. The three paths that still ring unconditionally are the ones that cannot know better:
   no pane resolved (no tmux), a `tmux::window` call that failed, and a focus call that failed.
   Keep the bell out of `finish` and `reset`, which is 007's rule and does not change. Note in the
   code why the order changed, because "ring first, then do the work" reads like the safer order and
   was deliberate until now. `working_does_not_ring` in `tests/tmux_server.rs` reasons from the old
   order ("the setter rings before it touches tmux at all"); its assertion still holds, its comment
   does not.
5. **`share/tmux/tmux-agent-status.conf`, two hooks.** Both verified to parse from a sourced conf
   file and to fire as described:

   ```tmux
   set-hook -g 'client-focus-in[50]' 'run-shell -b "tmux-agent-status clear-window #{pane_id}"'
   set-hook -g 'after-resize-pane[50]' "if-shell -F '#{!=:#{window_zoomed_flag},1}' 'run-shell -b \"tmux-agent-status clear-window #{pane_id}\"'"
   ```

   Comment both. `client-focus-in` never fires unless `focus-events` is on, which is the same
   condition that makes the whole feature work, so it costs nothing when unused. The guard on the
   resize hook is what keeps zooming **in** from clearing the siblings it just hid; the hook still
   fires on ordinary resizes, including every step of a mouse border drag, where the window is fully
   visible and clearing is what looking at it would do anyway. As everywhere else, the pane id is
   only used to find the window.
6. **README, setup step 2.** The snippet now sets four hooks across three scopes; update the
   sentence and the two `show-hooks` confirmations. Add the opt-in paragraph: `set -g focus-events on`
   (and detach/reattach every client afterwards) is what makes a hidden terminal keep its glyph;
   it makes tmux request focus reporting from the terminal and pass focus events into panes, which
   some full-screen applications react to; without it everything behaves exactly as it does today.
7. **README, "How it works" and "The bell, and colour".** Watched becomes seen, defined once with
   all four conditions. Replace the "a turn that ends on the window you are already watching"
   paragraph with the six-row table above in user wording: it is the whole user-visible contract of
   this tool and it currently lives in three prose fragments that do not agree. The bell section
   says which states ring and now also *when*: not when you are looking at the pane.
8. **README, known limits.** The zoom bullet and the hidden-terminal bullet are both conditional
   now: they still describe the behaviour with `focus-events off`. Rewrite them as such rather than
   deleting them, and drop the "we're planning to read the client's focus flag" sentence.
9. **`plugins/tmux-agent-status/commands/doctor.md`.** The hook check covers three scopes and four
   hooks. Add `focus-events` to the options it reports - it is inside the existing
   `Bash(tmux show-options:*)` allowance, so `allowed-tools` does not change - with the note that
   `off` means a hidden terminal still clears on the spot. Its closing paragraph about a turn ending
   on the watched window gains the zoom and hidden-terminal cases.
10. **001, the normative text.** Three things it fixes as normative are changed here. "**Focused
    means a client is attached**" becomes the four-term rule. The zoomed-window exception - "the
    alternative is a per-pane rule that misfires in the common case to be right in the rare one" -
    is now that per-pane rule, and the paragraph should say why the measurement changed the answer
    rather than being deleted. And "**Every turn-ending state rings by default**" gains "unless the
    pane is seen", with the reason: 001 kept the bell for the watched case because the only
    alternative then was `bell-action other`, which is exactly what 006 removes. The pointers in
    001 and 003 that used to name 006 already point here.

## Verification

**Unit, `src/tmux.rs`.** The six-field line parses; a status containing tabs still survives;
`on_screen` for zoomed/not against active/inactive; an unreadable flag still means not on screen.
For the focus parse: `attached,focused,UTF-8` is true, `attached,UTF-8` is false, and the match is
on the comma-separated element, so a future flag that merely contains the word does not read as
focused.

**Fake-tmux tier (`tests/tmux_failure.rs` shape).** The only way to produce an unfocused client in a
test: a `list-clients` that prints a client without the flag. Two cases - a turn ending on the
current window with an unfocused client leaves its glyph, and a `list-clients` that exits non-zero
clears exactly as before.

**tmux-server tier (`tests/tmux_server.rs`).** Its `attach()` helper runs a nested client, and a
nested client always reads focused (measured), so this tier covers the seen direction and all of
zoom: a `done` in a hidden sibling of a zoomed window keeps its glyph; a `done` in the zoomed pane
itself clears; `clear-window` on a zoomed window clears the zoomed pane and leaves the siblings;
unzooming through the new hook clears them.

**The bell is testable in the same tier, which it was not before.** The `attach()` helper's host
server is a terminal as far as the tested server is concerned, so `#{window_bell_flag}` on the
*host* window says whether a bell left the inner tmux. Verified by hand as the technique this task's
bell numbers come from. Two cases: a `done` on a seen pane leaves the host flag at 0 (nothing rang),
and a `done` on a hidden sibling of a zoomed window raises it. Both need `bell-action any` set on
the inner server, which is what 006 recommends and what the test should set explicitly rather than
inherit.

**Manual, real terminal, one agent, `focus-events on`.**

1. Terminal hidden, turn ends: the glyph is on the window entry. Alt-tab back: it goes, with no tmux
   window switch.
2. The same with `focus-events off`: no glyph at any point, which is today's behaviour.
3. `prefix-z`, a sibling pane finishes: the glyph stays. `prefix-z` again: it goes.
4. Two clients on one session, one focused: the window counts as seen and clears, with no special
   case in the code.
5. A terminal with no focus reporting at all behaves as in 2.
6. The bell, by ear, against the six rows: silent in row 1, audible in rows 2 to 6. Row 3 is the one
   to check twice, because it is the row where the bell is the only signal that leaves the machine.
7. A detached session: run a turn to completion detached, re-attach, and confirm the glyph is still
   on the window you land on and clears on your first window or pane switch.

## Done when

1. A turn that ends while the terminal window is hidden leaves its glyph on the tmux window entry,
   with `focus-events on`.
2. Returning to that terminal window clears the non-sticky states, with no tmux window switch.
3. With `focus-events off`, or a terminal that never reports focus, behaviour is bit-for-bit what it
   is today, and the README says so where it asks for the option.
4. A turn that ends in a pane hidden behind a zoom leaves its glyph, and unzooming clears it.
5. `set working` still costs three tmux calls; nothing but a clearing path pays for the fourth.
6. The bell rings exactly when the glyph is written: silent on a seen pane, audible on every unseen
   one, and still audible when there is no tmux or when a tmux call fails.
7. The six-row table is in the README in user wording, and 001's three normative sentences agree
   with it.
