---
description: Read-only check of the tmux-agent-status setup - binary, tmux snippet, format term, hooks.
allowed-tools: Bash(printenv TMUX), Bash(tmux-agent-status --version), Bash(command -v tmux-agent-status), Bash(tmux show-hooks:*), Bash(tmux show-options:*), Bash(tmux display-message:*), Read(~/.claude/settings.json)
disable-model-invocation: true
---

Diagnose why `tmux-agent-status` is or is not putting a glyph on the tmux window entry.

## This command is read-only. That is a hard constraint.

Run **nothing** that writes. No `set-option`, no `setw`, no `set-hook`, no `set-window-option`, no
edit of `~/.tmux.conf`, and no edit of `~/.claude/settings.json` - not even to "fix" something you
just diagnosed. Report the problem and the exact line the user should add; they apply it.

Two of these are not style preferences:

- Writing a spliced copy of `window-status-format` to a window-local option freezes that window's
  format forever, surviving reloads and uninstall. Read it, never write it.
- `~/.claude/settings.json` is hand-maintained, is often a symlink into a dotfiles repo, and is
  written by Claude Code itself at unpredictable moments. Read it, never write it.

If a check needs a command that is not in `allowed-tools` above, report the check as "could not
run" rather than reaching for a wider tool.

Run every command **bare**. Do not pipe one into `grep`, `head` or anything else: a pipeline is
authorised only if *every* stage matches a rule, and no rule above grants a filter. Read the whole
output and pick out what you need yourself - these commands print a handful of lines.

## The checks, in order

Run them all, then report. A later step failing is usually explained by an earlier one.

**0. Is this session inside tmux?**
`printenv TMUX`. Empty output or a non-zero exit means this Claude session is not inside tmux: say
so and stop, because every other symptom follows from it and the tool is a deliberate no-op there.

Do **not** test this with `tmux display-message`. That reaches the default tmux server whether or
not *this* process is inside tmux, so it succeeds in a plain terminal while a tmux server runs in
another one - a false pass on the single condition that best explains a missing glyph. `$TMUX` is
what the tool itself tests before doing anything, so it is what the diagnostic must test.

Once `$TMUX` is set, `tmux display-message -p '#{session_name}:#{window_index}.#{pane_index}'` says
where you are, which is useful context for the rest of the report.

**1. The binary.**
`tmux-agent-status --version`, which prints the version *and* the executable that actually ran.
Also run `command -v tmux-agent-status`.

- Not found: the hooks are firing and exiting silently. Point at `docs/install.md`, and note that
  the directory holding the binary must be on the `PATH` the agent's hooks inherit, which is not
  always the `PATH` of an interactive shell.
- Found under a build directory such as `target/debug` or `target/release`: a development shadow is
  in front of the installed copy. Report the path; it is not an error, but it explains stale
  behaviour.

**2. The tmux snippet.**
Two hooks, in **two different scopes** - checking only one scope is the easy mistake here:

```sh
tmux show-hooks -g     # expect session-window-changed
tmux show-hooks -gw    # expect window-pane-changed
```

Both lists are short; read them and find the `tmux-agent-status` entries yourself. Both should call
`tmux-agent-status clear-window`.

- Neither present: the snippet is not sourced. The user adds
  `source-file <path>/tmux-agent-status.conf` to their tmux configuration and reloads.
- Only one present: report which is missing. Without `window-pane-changed`, switching panes inside
  a window will not clear its `done`/`error`/`waiting`; without `session-window-changed`, switching
  windows will not either.

**3. The format term.**
`tmux show-options -g window-status-format` and `tmux show-options -g window-status-current-format`.
Report both values, and whether `@agent_status` appears in **each**.

- Missing from either: the glyph is invisible on exactly the windows using that format - a term
  present only in `window-status-format` means the glyph vanishes on the window you are looking at.
  Give the term to paste, after the name segment (outside any `#{=/N/…:}` truncation) and before the
  window flags:

  ```tmux
  #{?@agent_status, #{@agent_status},}
  ```

**4. The agent hooks.**
This plugin owns them: installing it is what registers the six Claude Code events. Say so.

Then read `~/.claude/settings.json` and check whether its `hooks` section *also* contains
`tmux-agent-status` commands.

- Present: the user has both the plugin and the manual paste, so every event fires twice. The writes
  are idempotent so the glyph stays correct, but `done` rings the terminal bell twice. Tell them to
  delete the `tmux-agent-status` entries from `~/.claude/settings.json` and keep the plugin, and
  show which keys to remove. Do not remove them yourself.
- Absent: correct. Nothing to do.

Report what the `hooks` section contains and nothing else about that file. Do not describe how it is
managed - a symlink there may point into a dotfiles repo, a Nix store path, a home-manager
generation or nothing at all, and each of those has different write semantics. Guessing produces a
confident, wrong instruction about where the user should make changes.

## Reporting

One line per step: the step, pass or fail, and the evidence you read. For the first failing step,
add what to change and where. Finish with what to expect once it is fixed - 🤖 while a turn runs,
✅ when it ends, ❗ on an aborted turn, 💬 when the agent is blocked on the user - and that a window
with no agent renders exactly as it did before.

If every step passes and the user still sees no glyph, the likely cause is that the turn ended on
the window they were already watching: that state is cleared on the spot by design, and the bell is
the only signal. Say that rather than inventing a further check.
