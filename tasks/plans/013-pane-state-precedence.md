# 013 - A later event may not demote a state you have not seen

Status: in progress.

Covers *which of two events on one pane wins*, and *how the write that decides it stays atomic*.
What the states mean, the rollup across panes and the focus-clear rule are 001, which stays
normative and is updated by this plan where it changes.

## The defect

A Devin session sitting on a permission prompt shows 🤖 instead of 💬. Reproduced end to end on
Devin CLI 3000.10.27 with the shipped `share/agents/devin/hooks.v1.json`: with the prompt still on
screen, `@agent_pane_status` read back `working` and `@agent_status` read back 🤖.

`PermissionRequest` is the right event and it fires. Devin runs the tool calls of one turn
concurrently, so a sibling tool that needs no permission finishes while the other waits on the
human, and its `PostToolUse` arrives after `PermissionRequest`:

```
PreToolUse         tool_name=read
PreToolUse         tool_name=exec
PermissionRequest  tool_name=exec    <- set waiting
PostToolUse        tool_name=read    <- set working, prompt still open
```

`set` is last-write-wins, so the ordering alone decides the glyph, and the last writer is the
sibling nobody is waiting for. A serial turn is correct, which is why it does not always show.

### Why reading the pane first is not enough

Devin runs the hooks of sibling events concurrently. With a 3s sleep in the `PermissionRequest`
hook, both hooks started in the same millisecond:

```
054.593 start PermissionRequest exec
054.593 start PostToolUse read
054.599 end   PostToolUse read
057.604 end   PermissionRequest exec
```

A guard that reads `@agent_pane_status` in Rust and then writes it is check-then-act: the
`PostToolUse` hook can read `working` before `waiting` lands and write `working` after it. The
rollup has the same race today - a `set-option -w` computed from a stale `list-panes` can land
last and show 🤖 over a `waiting` pane.

## Decisions

1. **Within a pane, a later state replaces an earlier one only if it does not rank lower in
   precedence: `error` > `done` > `waiting` > `working`.** So `working` never replaces anything,
   `error` always wins, and `waiting` never replaces a `done` you have not seen. The last rule is
   wanted, not tolerated: a `waiting` after a finished turn is the 💬 that turns out to have nothing
   for you. States still leave a pane only by being seen (`clear-window`), by `reset`, or by the
   watched-window rule - which is also how a pane gets back down to `working`: you cannot answer a
   prompt or type the next one without looking at the window.
2. **Across panes the rollup rank is unchanged: `waiting` > `error` > `done` > `working`.** It
   answers a different question - which pane wants you most - and a window holding one finished
   and one blocked pane must show 💬. Two orderings on one enum, each named for its question.
3. **Every decision that depends on current tmux state is taken inside the tmux server, in one
   command.** `set-option -F` expands its format against the target *before* setting, and the
   server executes one command at a time, so a format that reads the pane's own option and picks
   the new value is a compare-and-set. No lock file: that would be the state file 001 forbids.
4. **Claude Code's `Notification` is narrowed to the types that mean blocked on you.** Under 1, a
   `waiting` that fires mid-turn stays until `Stop`, and Claude Code sends twelve notification types
   to that hook, several of them not blocking (`agent_completed`, `auth_success`,
   `elicitation_complete`, `elicitation_response`, `quota_auto_resume_*`). The matcher becomes
   `permission_prompt|elicitation_dialog|elicitation_url_dialog|agent_needs_input`, and
   `idle_prompt` is dropped. This reverses 001's "must not be narrowed". Findings below.
5. **A refused write does not ring.** The bell reports a state that landed. A `waiting` refused
   because the pane shows `done` rings nothing; a write cleared by the watched-window rule still
   rings, as today.

## Findings

All probed on 2026-09-15.

- **tmux 3.6a, disposable `tmux -L` server.**
  - `set-option -p -F @agent_pane_status '#{?#{@agent_pane_status},#{@agent_pane_status},working}'`
    keeps `waiting` and fills an unset pane with `working`.
  - Nested `#{?#{==:#{@agent_pane_status},error},error,...}` chains give the full decision 1 table,
    all 24 transitions from unset, the four states and an unrecognised value.
  - An unset pane option reads back empty in a format when the window option has a different name,
    so the two-name rule from 001 holds.
  - `set-option -w -F` with `#{P:...}` and `#{m:...}` computes the rollup inside the server.
  - Races, as evidence rather than proof (the proof is the single-threaded server): 20 rounds of 300
    concurrent `-F` writers around one plain `waiting` write lost it 0 times; 10 rounds of 300
    concurrent `if-shell -F ... 'set-option -p -u ...'` clears around one `working` write wiped it
    0 times.
  - `#{?session_attached,...}` treats `0` as false.
- **Minimum tmux is unchanged.** From the tagged man pages: `#{P:}` loops, `#{m:}`, `#{==:}`,
  `if-shell -F` and `set-option -F` are all in 2.9a; pane options (`set-option -p`), which the tool
  already needs, arrived in 3.0.
- **Devin CLI 3000.10.27.**
  - `PreToolUse` fires *before* `PermissionRequest` for the same call.
  - The trust-this-directory prompt fires no hook at all, not even `SessionStart`.
  - The shipped `PreToolUse` matcher names `ask_user_question`, which Devin's tool list does not
    contain. Out of scope here.
- **Claude Code.** The `Notification` matcher values, verbatim from the hooks reference:
  `permission_prompt`, `idle_prompt`, `auth_success`, `elicitation_dialog`,
  `elicitation_url_dialog`, `elicitation_complete`, `elicitation_response`, `agent_needs_input`,
  `agent_completed`, `quota_auto_resume_fired`, `quota_auto_resume_stale`,
  `quota_auto_resume_disabled`. The reference does not say when `idle_prompt` fires, so it was
  probed on 2.1.273, with a hook on all eight events and a permission prompt held open:

  | Seconds | Event |
  |---|---|
  | 4.6 | `PreToolUse` Write |
  | 10.6 | `Notification` `permission_prompt` |
  | 11 - 91 | prompt held open, **no `idle_prompt`** |
  | 91 | prompt rejected with Esc - **no `Stop`**, and no `idle_prompt` in the 80s after |
  | 180 | a later turn ends: `Stop` |
  | 1274 | `Notification` `idle_prompt`, ~18 minutes after that `Stop` |

  So the nag is not the "still blocked" repeat 001 took it for: it never fires while a prompt is
  open, and when it does fire the pane holds a `done` that now outranks it. Dropping it strands no
  🤖 either, because a rejected prompt leaves the pane on `waiting`, not `working`. Decision 4
  stands. What is still unprobed is an Esc *during* a running tool call; the session-boundary reset
  of 007 remains the backstop there.

## Design

### The formats (`src/formats.rs`, pure)

Built from `State`, so the ranks live in one place. `S` is `#{@agent_pane_status}`.

- **Watched**: `#{?window_active,#{?session_attached,1,},}` - `1` when the window is on screen.
- **Pane write of state `N`**: a chain over the states whose precedence exceeds `N`'s, each keeping
  itself, ending in `N`:
  `#{?#{==:S,error},error,#{?#{==:S,done},done,N}}` for `waiting`. For a non-sticky `N` the chain is
  wrapped as `#{?WATCHED,,CHAIN}`: on a watched window the reporting pane is cleared, whatever it
  held, exactly as today.
- **Sibling on a set**: `#{?WATCHED,CLEAR,S}`, where `CLEAR` is the focus rule: empty for each
  non-sticky state, `S` otherwise, so `working` and an unrecognised value survive.
- **Clear on focus**: `CLEAR` alone.
- **Rollup**: a `#{P:...}` loop emitting one rank digit per pane that holds a recognised state, and
  a chain from the highest rank down: `#{?#{m:*4*,LOOP},💬,#{?#{m:*3*,LOOP},❗,...}}`. Digits come
  only from the loop body, so no option value can be mistaken for one.

### Empty means unset, and is normalised atomically

A `-F` write can only produce a value, so a clear produces `""`. After the writes, each touched
option gets `if-shell -F -t T '#{?OPTION,,1}' 'set-option -u ...'`. It only ever removes an empty
value, and decides on the value current at that instant, so a concurrent write that lands between
the two commands is never lost. The nested command string holds a pane id and an option name and
nothing to quote.

### Calls per event

- `set`: `list-panes -F '#{pane_id}'` for the siblings, then one tmux invocation holding the pane
  write, the sibling writes, the normalisers, the rollup, and `display-message -p` of this pane's
  option for the bell. Two processes, down from three.
- `finish`: the same as `set done`, without the readback and without the bell. Its "keeps `error`"
  rule is now decision 1 and needs no code of its own.
- `reset`: one invocation - unset this pane, rollup, normaliser. No `list-panes`.
- `clear-window`: `list-panes`, then one invocation with the clear per pane, normalisers, rollup.

### The bell

`set` rings after the write when `N` rings and the readback is `N` or empty (cleared as watched).
When there is no tmux, or the tmux call fails, it rings as today: the bell is a separate channel
and an unreachable server must not silence it.

### What goes

`src/rollup.rs` and `tests/rollup.rs`, whose rule moves into the rollup format and is tested
against a real server; `Window.watched` and the flag parsing in `tmux.rs`, which tmux now evaluates
itself.

## Work

1. `State::precedence()` beside `rank()`, with unit tests for the order.
2. `src/formats.rs`: the five formats above, with unit tests pinning their shape.
3. `src/tmux.rs`: `pane_ids()`, and one `run()` that sends a command list as a single invocation.
4. `src/command.rs`: `set`, `finish`, `reset`, `clear_window` on top of 2 and 3; bell after the
   readback.
5. Remove `src/rollup.rs`, `tests/rollup.rs`, the flag parsing and their tests.
6. `tests/tmux_server.rs`: the full transition table against a real server, every ordered pair of
   two panes for the rollup, the watched variants, a refused `waiting` that does not ring, and the
   regression: many concurrent `set working` processes around one `set waiting` end on 💬. Adjust
   `every_state_reaches_the_window_as_its_own_glyph`, which walks one pane through all four states.
7. `tests/tmux_failure.rs`: the fake tmux scripts match the new invocations; drop the unreadable
   flag test, which has no subject left.
8. Claude Code matcher in `plugins/tmux-agent-status/hooks/hooks.json` and the Supported states row
   in `docs/agents/claude-code.md` (the manifest test holds them together).
9. Docs: the README states table and the precedence sentence; `docs/agents/README.md`'s "`waiting`
   must repeat" bullet, whose reason - a later `working` hiding the prompt - this plan removes;
   001's states table, hook table and the un-narrowed `Notification` paragraph.
10. Delete `tasks/todo/013-sibling-tool-clobbers-waiting.md`; this plan carries its content.

## Verification

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and the coverage
  gate in the justfile.
- End to end, with the shipped hook files and the built binary on `PATH`, from a window that is not
  the current one:
  1. Devin, one turn issuing a read and a permission-gated `exec` in parallel: 💬 while the prompt
     is open, 🤖 after approving and looking away, ✅ at `Stop`.
  2. Claude Code, a permission prompt: 💬; a finished turn left unseen for over a minute stays ✅.
  3. A two-pane window, one pane finished, one blocked: 💬.
