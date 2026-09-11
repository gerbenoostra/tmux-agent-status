# 006 - A bell that never leaves tmux

Status: completed - the README, shipped tmux snippet and plugin doctor document the full bell path.
Documentation only: no code or test-tier changes.

Covers *the notification the terminal never hears*. The glyph half of the same case - a window tmux
calls watched that nobody can see - is 008. The states, the rank and the rollup are 001 and do not
change here.

## The defect

`set` writes `\a` to the pane's tty (`src/bell.rs`). tmux takes that BEL, raises the window's bell
flag, and then decides whether to pass it on to the terminal. Two options make that decision, and
`bell-action other` answers "not for the window you are on":

> `any` means activity in any window linked to a session causes a bell or message (depending on
> `visual-bell`) in the current window of that session [...] `other` means activity in the current
> window is ignored but not those in other windows.

That is exactly this case. The agent runs in window W, W is the session's current window, and the
terminal holding the client is behind another tab, desktop or monitor. tmux swallows the one signal
that could have reached the human.

The README recommends that value itself, in the optional highlight block of setup step 3. tmux does
not. Measured on 3.6a with `tmux -f /dev/null` (no user config):

| Option | tmux default | What the README block sets |
| --- | --- | --- |
| `bell-action` | `any` | `other` |
| `monitor-bell` | `on` | `on` |
| `visual-bell` | `off` | not mentioned |

So the tool is correct out of the box and our own documentation breaks it. This is a README fix, not
a research task.

Measured end to end, with a nested tmux standing in for the terminal - the inner server holds the
pane that rings, the outer server's `#{window_bell_flag}` says whether the bell got out:

| inner `bell-action` | BEL raised in the **current** window | reached the terminal |
| --- | --- | --- |
| `other` | yes | **no** |
| `any` | yes | **yes** |

The inner window's own bell flag read 0 in both cases: a bell in a window a client is sitting on
leaves no `!` and no lasting highlight. Getting the BEL out to the terminal is the only thing `any`
changes.

## The fix, and what it costs the user

`bell-action any` forwards the bell from the current window too. The cost is real and belongs in the
same paragraph: **every** program in the current window that rings - a shell completion beep, vim
hitting the end of a search - now reaches the terminal as well. That is a preference, so the README
recommends and explains rather than dictates.

It also un-mutes one case of our own: a turn ending on the window you are actively watching now
reaches the terminal too, because `bell-action other` was the only thing suppressing it. That
suppression was always the wrong instrument - it drops the bell for the whole current window, hidden
terminal and zoomed sibling included - and 008 replaces it with suppression at the source, where the
tool knows about focus and zoom and can be silent only when you are really looking.

**Between this task and 008 landing, a turn ending on the window you are watching signals the
terminal.** On a current terminal that is a tab marker rather than a sound (next section), so the
interim cost is small. Say it in the README rather than leaving it to be discovered, and keep the
two tasks in this order: `any` first, precise suppression second. The reverse order leaves the
hidden-terminal case silent for longer, which is the case that has no other channel.

## What the terminal does with the BEL

"Bell" is the wrong mental model for what the user actually gets, and the README should not promise
a sound. What tmux forwards is one `\a` to the client's tty; the terminal decides the rest, and a
current one decides mostly visually. Measured on Ghostty 1.3.1, whose defaults are
`bell-features = no-system,no-audio,attention,title,no-border`:

| Feature | Default | Effect |
| --- | --- | --- |
| `title` | on | Prepends 🔔 to the title of the alerted surface, until it is re-focused or receives keyboard input |
| `attention` | on | Only while the application itself is unfocused: on macOS, one dock icon bounce |
| `system`, `audio` | off | No sound at all unless the user opts in |
| `border` | off | No border highlight |

Three consequences for the wording:

- The marked surface is the one running the tmux **client**, so the right tab is marked by
  construction, and every tab attached to that session is marked.
- The signal says *which tab*. It cannot say which window, which is what the glyph is for. Naming
  that division once is more useful than describing either half.
- It lowers the price of `any`. On these defaults the cost in the you-are-looking case is a 🔔 on
  the tab you already have in front of you, cleared by your next keystroke - not a beep. Say that,
  and name `bell-features` (or the equivalent) as where a user turns it into a sound if they want
  one, rather than implying this tool controls it.

Terminals differ, so the README states the mechanism and gives Ghostty as the worked example rather
than claiming behaviour for all of them.

Two neighbouring options have to be named with it, because `bell-action` alone is not sufficient:

- `monitor-bell on`. tmux(1) defines `bell-action` as the action "on a bell in a window **when
  monitor-bell is on**". With it off, tmux ignores the bell and no value of `bell-action` matters.
- `visual-bell off`. With it on, tmux shows a message instead of passing the bell through, so the
  terminal stays silent. It is off by default, and the highlight block is exactly where a user goes
  to experiment, so say it.

## Rejected alternatives

- **Write the BEL to each attached client's tty** (`list-clients -F '#{client_tty}'`) instead of the
  pane's. It bypasses `bell-action` entirely, and pays for it: one extra tmux call on every
  turn-ending event against 001's three-call budget, a bell for clients that are looking at a
  different window, and a second implementation of a job tmux already does. No.
- **Leave the bell to the agent's own notification channel.** Already documented for Claude Code as
  an alternative, and no answer at all for the agents that have no such channel - which is why 001
  has the tool ring in the first place.

## Work

1. **README, the highlight block in setup step 3. Done.** The block now recommends
   `bell-action any` with `monitor-bell on` and `visual-bell off`, as a four-row table of which
   setting decides what, followed by the price of `any` (every other bell from the window you are
   on, a turn ending on the window you are watching included) and one paragraph on what the terminal
   does with the BEL, with Ghostty's `bell-features` as the worked example and the tab/window
   division named.
2. **README, "The bell, and colour". Done**, by pointing at that table rather than restating the
   chain: two half-explanations of the same four options is how the old text went wrong.
3. **README, known limits. Done.** Both bullets described the glyph, which this task does not
   change, so they stay - with two corrections the measurements forced: the bell they offer as
   compensation only arrives with `bell-action any`, and closing a pane clears a window just as
   creating or splitting one does (`kill-pane` fires `window-pane-changed` on the pane that becomes
   active). "How it works" also stated that a detached session's glyph lasts "until you come back";
   it survives the re-attach and goes on the first window or pane switch, which is what makes that
   case worth anything.
4. **`share/tmux/tmux-agent-status.conf`. Done.** A third numbered comment block below the line
   carries the same three settings and the same one-line reason. The file's contract is unchanged:
   two hooks, and comments the user pastes themselves. It sets none of these.
5. **`plugins/tmux-agent-status/commands/doctor.md`. Done.** A new read-only check at step 4 covers the
   bell path, and the agent-hook step is now 5. It runs `tmux show-options -g bell-action`,
   `tmux show-options -gw monitor-bell` and `tmux show-options -g visual-bell`, all already inside
   the existing `Bash(tmux show-options:*)` allowance, so `allowed-tools` does not change. What it
   reports: `bell-action other` means bells from the window you are on are dropped, which is the
   "everything else passes and I still hear nothing" report; `monitor-bell off` means no bell
   reaches the terminal at all; `visual-bell on` means a tmux message instead of a bell.
6. **`doctor.md`, the closing paragraph. Done.** The remaining terminal signal is conditional on
   the bell-path check passing; otherwise the report names the setting that prevented it.

No code changes, so no test tier moves. The binary's behaviour is identical before and after.

## Verification

Partly automatable, and worth doing that way: a nested tmux is a terminal as far as the inner server
is concerned, so asserting on the **host** server's `#{window_bell_flag}` measures whether the bell
left the inner tmux. That is how the table above was produced, and `tests/tmux_server.rs` already
has the `attach()` helper that builds the pair. It belongs to 008, which is the task that changes
what the binary does; here it is only evidence for a documentation change.

The rest is manual, in a real terminal, because what is being checked is whether something outside
tmux hears anything.

1. `bell-action other`, terminal visible. From a pane in the **current** window,
   `tmux-agent-status set done`. No bell. Hide the terminal window and repeat: still no bell.
2. `set -g bell-action any`, both again: bell, visible and hidden.
3. `bell-action other`, from a pane in a **non-current** window: bell. This is the case `other` was
   chosen for and it must not regress under `any` either - check it a second time with `any`.
4. `monitor-bell off` with `any`: no bell. `visual-bell on` with `any`: a tmux message, no bell.
   These two are what the doctor's new report lines claim, so they are measured, not assumed.
5. With `any`, a bell in the current window raises and immediately drops that window's bell flag
   (already measured: it reads 0 straight after), so no `window-status-bell-style` highlight should
   be left on the window you are on. Confirm by eye, together with the glyph still composing with
   the highlight the way 001 verified.

## Done when

1. The README recommends `bell-action any` with `monitor-bell on` and `visual-bell off`, says in one
   sentence what each does, and states what `any` costs.
2. The README says what the BEL becomes on the far side: a terminal-side tab marker whose form is
   the terminal's business, with Ghostty's `bell-features` as the worked example, and that the
   marker says which tab while the glyph says which window.
3. The shipped snippet carries the same three lines as comments.
4. `/tmux-agent-status:doctor` reports the bell path and names `bell-action other` as the cause of
   "no glyph and nothing from the terminal on the window I am on".
5. All five verification steps done by hand in a real terminal, hidden and visible.
