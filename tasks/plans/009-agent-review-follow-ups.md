# 009 - Review follow-ups for the agents beyond Claude Code

Status: done - every item below is implemented and covered by a test.

## Summary

A review of `005`'s branch found sixteen items: three wrong glyphs or missing bells that a user
would actually hit, four drop-in files that disagreed with the committed survey, three CLI edges,
and six gaps in the tests and docs that let the rest of them through. `cargo fmt`, `cargo clippy
--all-targets -D warnings`, `cargo test` and `just coverage` (100% lines and regions) are green.

The theme worth keeping: every wrong mapping here was in a **file**, not in code, and the test tier
that was meant to catch drift only checked JSON syntax. Item 12 closes that.

## Behaviour

### 1. An empty `--pane` painted whatever pane tmux was on

**Severity:** high - a glyph on the wrong window, silently.

`resolve_pane` filtered an empty `TMUX_AGENT_STATUS_PANE` but returned `Some("")` for an explicit
`--pane ""`. Every per-agent page documents `--pane #{pane_id}` or `--pane "$TMUX_PANE"`, and both
expand to nothing outside tmux. `tmux set-option -p -t ""` does not fail: it resolves to the
*current* pane and exits 0, so the glyph landed on whatever the server considered current.

An empty override is now no override, and falls through to the same tiers as an absent one.

### 2. Grok and Kiro never rang the bell

**Severity:** high - the notification is the point of the tool.

Both drop-ins mapped their turn-end event (`Stop`, `AgentStop`) to `finish`, which writes the glyph
and deliberately does not ring. Both docs tables labelled the row "done". They now map to
`set done`. Grok keeps `SessionEnd -> finish`, which is what `finish` is for; Kiro's CLI has no
session-end trigger at all, so it has no `finish` row.

### 3. A failed tool call is not an aborted turn

**Severity:** medium - a wrong glyph plus a bell, several times per healthy turn.

Mistral Vibe's `post_tool` with `tool_status = "failure"` and Cursor's `postToolUseFailure` both
mapped to `error`. `001` defines `error` as the turn aborting; a grep that matched nothing or a test
run that failed is an ordinary part of a turn that is still running. Both now map to `working`, and
both agents' `error` column reads "no": neither publishes a turn-abort event. This is `005`'s "a
wrong glyph is worse than no glyph" applied to the one case where the survey suggested otherwise.

### 4. Codex fired on fewer session starts than intended

**Severity:** medium - a silent no-op, which is the failure `005` wrote its "prove it fired" step for.

The `SessionStart` matcher was `startup|resume|clear|fork`. The survey records Codex's matchers as
`startup`/`resume`/`clear`/`compact`: `fork` is not one of them and `compact` was missing. Corrected
to the documented four, on the page as well as in the file.

### 5. Codex and Copilot wrapped only some entries in `printf '{}'`

**Severity:** medium - an unwrapped hook can break the agent's turn.

Both agents parse hook stdout as JSON per event, but only the turn-end and subagent entries carried
the wrapper. `PermissionRequest` was the riskiest of the unwrapped ones: a permission hook that
returns nothing may be read as a decision. Every entry in both files now carries it. Cursor already
did.

### 6. Droid missed the idle nag, and stranded a cancelled turn

**Severity:** medium - `waiting` is only useful if the repeat exists (`001`).

The `Notification` matcher was `permission_prompt` only. Droid also emits `idle_prompt`, which
repeats while it is blocked on you, including immediately after you cancel a turn - and a cancelled
turn emits `Notification` *instead of* `Stop`. With `permission_prompt|idle_prompt` a cancelled turn
now lands on 💬 rather than leaving 🤖 stranded until the next `SessionStart`.

## CLI

### 7. A disabled `notify --stdin` left the agent's payload undrained

**Severity:** low - only bites on a payload larger than the pipe buffer.

`TMUX_AGENT_STATUS_DISABLED` was checked before stdin was read, so the process exited with the pipe
full and the agent took an `EPIPE` on a hook it had been told was a no-op. The payload is now
consumed first; being disabled has to be invisible to the caller.

### 8. `TMUX_AGENT_STATUS_DEBUG` accepted only `=1`

`is_disabled()` took any non-empty value and `debug()` required exactly `"1"`. One namespace, two
spellings, and `DEBUG=true` was indistinguishable from "nothing was dropped". Both now go through
one `flag()` helper. The documented spelling stays `=1`.

### 9. `notify --stdin junk` ignored the positional

The argv path rejects extra arguments with exit 2; `--stdin` silently swallowed them. A typo in a
hook line is meant to be loud, so it now errors the same way.

## Docs

### 10. Only the Nix route said where the drop-ins are

`docs/install.md` learned `share/agents/` in the prebuilt-tarball, `cargo install` and from-source
sections. `005` step 4 named all three; only Nix had it. `docs/agents/kiro.md` points readers there
and was previously pointing at two sections that did not mention it.

### 11. The Droid page described a `hooks` key its file does not have

`share/agents/droid/hooks.json` holds the event names at the top level, unlike the Codex file. A
reader who followed the prose and nested them under `hooks` would get a config Droid ignores with no
error.

Every page also gained a `finish` row where the agent has a session-end event, so the mapping table
lists every command its file runs - which is what item 12 checks.

## Tests

### 12. The drop-in validator checked syntax and nothing else

**This is the item that matters.** `tests/agent_configs.rs` parsed each JSON file and validated the
subcommand spelling, so items 2, 4, 5 and 6 all passed it. It now also asserts, per agent:

- a docs-table row's command matches the state it names (`done` must run `set done`, not `finish`),
- and the set of commands in the shipped file equals the set in that page's mapping table.

Verified by reverting item 2 and watching it fail. It also accepts `notify`, which it previously
panicked on - a shape B JSON drop-in would have crashed the test rather than failed it.

### 13. The terminal-stdin test passed with the guard removed

`notify_stdin_with_terminal_is_a_no_op` closed the pty master before waiting, so a read on the slave
returned `EIO` at once and the child exited either way. The master is now held open for the whole
wait, which would block forever without the `is_terminal()` guard, and the wait is bounded instead.
Verified by removing the guard and watching it fail.

### 14. `tests/tmux_failure.rs` inherited the environment it was testing

Its helpers set `TMUX`/`TMUX_PANE`/`PATH` but never cleared `TMUX_AGENT_STATUS_PANE`,
`_DISABLED` or `_DEBUG`. A developer who took the per-agent pages' advice and exported
`TMUX_AGENT_STATUS_PANE` broke `empty_tmux_pane_means_not_in_tmux`; an exported `_DISABLED=1` turned
most of the file into vacuous passes. Both files now build every invocation through one helper that
clears all three.

### 15. Claude Code had no `share/agents/` entry

Not a review finding, done in the same branch. Claude Code's hook set only existed in the plugin and
in the root `README`, so a user who installed through nix, a tarball or `cargo` had no copy of it -
the one agent with a first-class integration was the one agent missing from the install tree. It now
ships as `share/agents/claude-code/hooks.json`, byte for byte the plugin's own `hooks/hooks.json`
with a test asserting it stays that way, plus `docs/agents/claude-code.md` in the shape every other
agent page uses. Claude Code has no hooks drop-in directory, so the page documents a merge into
`settings.json` and recommends the plugin first.

The directory is `claude-code`, not `claude`, because the page it implies would be
`docs/agents/claude.md` and that collides with `CLAUDE.md` on a case-insensitive filesystem.

New coverage for items 1, 7, 8 and 9: an empty `--pane` falls back and, outside tmux, runs no tmux
command at all; a disabled `notify --stdin` still drains a 256 KiB payload; `DEBUG=true` logs;
`--stdin` with a leftover positional exits 2.
