# Agent notes for tmux-agent-status

This repository is **public and open source**: nothing in it may contain local paths, machine
names, personal configuration, secrets or anything else that is not useful to a stranger who
cloned it.

`tasks/todo/` holds ideas and bug reports not being worked on; `tasks/plans/` holds the plan
currently being executed plus every plan already executed, kept as the audit trail. Status lives in
the `Status:` line inside each file.

## What this tool is

`tmux-agent-status` turns agent lifecycle events into one glyph on the tmux window entry. It writes two
tmux options and rings the terminal bell. It never touches a window name, never shells out to git,
never writes a state file, never edits the user's config files, and never spawns a daemon. The full
design is in `tasks/plans/001-agent-window-status.md`; read it before changing behaviour.

## Rules that are easy to break

- The status format is documented for the user to paste. **Never** write, rewrite or splice
  `window-status-format`. Writing a spliced copy to a window-local option freezes that
  window's format forever; see 001. Reading it is fine, and is how a setup check tells the user
  whether the term is present: `show-options` yes, `set-option` never.
- `@agent_pane_status` (per pane) and `@agent_status` (per window rollup) are two names on purpose.
  tmux option inheritance makes a pane with no status read back as the window's value, so they can
  never be merged into one.
- Agent hook entries are documented, never written. The tool must not edit
  `~/.claude/settings.json` or any equivalent.

## Development

Run development tools within the shell `nix develop` creates, or use `. "$HOME/.cargo/env" && [cmd]`.
