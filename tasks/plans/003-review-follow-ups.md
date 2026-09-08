# 003 - Walking-skeleton review follow-ups

Status: done - implemented on `fix/review-follow-ups`.

## Summary

`cargo test`, `cargo clippy`, `cargo fmt`, `cargo llvm-cov` and `nix build .#tmux-agent-status` are all green on this branch. The skeleton's separation between pure state/rollup and impure tmux calls is intact, and the test tiers cover the inheritance trap, the rollup rank and the failure policy.

The items below are gaps between the current code and `001`'s stated behaviour, or small follow-up work that surfaced during review. Items already tracked in `002`, `004` and `005` (tagging `v0.0.1`, the config file, the `stale` state, the Claude Code plugin, and agents beyond Claude Code) are not repeated here.

## Follow-up items

### 1. `set` should clear non-sticky states immediately when the pane is already focused

**Severity:** high - specified behaviour in `001`, user-visible.

`001` says the non-sticky states (`done`, `error`, `waiting`) must clear immediately if the pane is already focused, so a `done` on the window you are already watching never renders at all. The current `set` always writes the pane option and recomputes; the clear only happens later via the focus hooks.

- Detect whether `TMUX_PANE` is the active pane of the active window, or whether the target window is already the current window.
- If so, treat the event as `clear_window` for the non-sticky states instead of writing them.
- Keep the bell: a finished turn still deserves the notification even if you are already looking.

Implemented, with two corrections that the first attempt got wrong and review caught:

- **Watched is `window_active` *and* `session_attached`.** `#{window_active}` alone is 1 for a
  detached session's current window, which threw away every turn that ended while the user was
  away. `session_attached` is a client count, not a flag, so it is parsed as a number.
- **The reporting pane's own state is superseded, sticky or not.** Treating the event as a plain
  `clear_window` preserved `working` on the very pane whose turn had just ended, so watching your
  agent finish stranded a 🤖 that no later focus event ever cleared. `clear_statuses` now takes the
  pane the event arrived on and drops its state whatever it held; siblings keep the ordinary rule.

The case where a client is attached but its terminal window is not on screen is out of scope here
and is tracked in `006`.

### 2. `clear_window` lists panes twice

**Severity:** low - performance, not correctness.

`clear_window` calls `tmux::pane_statuses(target)` once to decide which panes to clear, then calls it again inside `recompute(target)`. Each `list-panes` is a tmux round-trip. `001` budgets three tmux calls for `set`; `clear_window` can be reduced from `N+3` calls to `N+2` by recomputing from the already-fetched pane data (subtracting the cleared panes from the in-memory state).

### 3. README does not document the `clear-window` pane argument

**Severity:** low - documentation.

The shipped hooks pass `#{pane_id}` to `clear-window`, and `tmux-agent-status clear-window <pane>` is the hook path. The README only shows `tmux-agent-status clear-window` without the optional argument. Add a sentence that the pane argument defaults to `$TMUX_PANE`.

### 4. README wording and snippet grammar

**Severity:** nit - documentation.

- `README.md` line 10 reads "The internal `tmux-agent-status` executable is called..."; remove "internal" or rephrase.
- `share/tmux/tmux-agent-status.conf` line 13 says "prevent it to go blank"; should be "prevent it from going blank".

### 5. `UnknownState` exposes a public tuple field

**Severity:** nit - API hygiene.

`UnknownState(pub String)` in `src/state.rs` lets callers construct an error with an empty or otherwise invalid string. Make the field private and provide a constructor, or derive it only from parsing. This matters only because the crate is also a library used by integration tests; the binary itself is unaffected.

### 6. Unix-only tests are not gated

**Severity:** nit - portability.

`tests/tmux_failure.rs` uses `std::os::unix::fs::PermissionsExt`. Add `#[cfg(unix)]` to the module so `cargo test` on Windows fails cleanly at compile time rather than with a confusing import error. The tool itself is Unix-only, so this is defensive.

### 7. Missing test: `TMUX` absent but `TMUX_PANE` present

**Severity:** nit - coverage.

`current_pane()` in `src/tmux.rs` returns `None` unless both `TMUX` and `TMUX_PANE` are set. Add a test that the hook exits silently when only `TMUX_PANE` is present, mirroring the existing "outside tmux" test that removes both variables.
