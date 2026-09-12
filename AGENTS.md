# Agent notes for tmux-agent-status

This repository is **public and open source**: nothing in it may contain local paths, machine
names, personal configuration, secrets or anything else that is not useful to a stranger who
cloned it.

`tasks/todo/` holds ideas and bug reports not being worked on; `tasks/plans/` holds the plan
currently being executed plus every plan already executed, kept as the audit trail. Status lives in
the `Status:` line inside each file.

## What this tool is

`tmux-agent-status` turns agent lifecycle events into one glyph on the tmux window entry. The hook
commands (`set`, `reset`, `finish`, `clear-window`, `notify`) write two tmux options and ring the
terminal bell, and nothing else: they never touch a window name, never shell out to git, never write
a state file, never edit any config file, and never spawn a daemon. The one exception is the
`install` subcommand, which a human types and which writes config files under the contract in
`tasks/plans/011-install-command.md`. The full design is in
`tasks/plans/001-agent-window-status.md`; read it before changing behaviour.

## Rules that are easy to break

- **Never call `set-option` on `window-status-format` or `window-status-current-format`, at any
  scope, ever.** Writing a spliced copy to a window-local option freezes that window's format
  forever; see 001. Reading it is fine, and is how a setup check tells the user whether the term is
  present: `show-options` yes, `set-option` never. A test asserts the option name never appears as a
  `set-option` argument anywhere in `src/`.
  The hazard is a property of the tmux *option*, not of the format string, so editing the **text of
  the user's config file** is allowed, and is what `install --tmux-format` does - the same edit the
  README asks the user to make by hand. See `tasks/plans/011-install-command.md`.
- `@agent_pane_status` (per pane) and `@agent_status` (per window rollup) are two names on purpose.
  tmux option inheritance makes a pane with no status read back as the window's value, so they can
  never be merged into one.
- Agent hook entries are documented **and** written, but only by `install` and only under the safe
  write in `tasks/plans/011-install-command.md`: resolve symlinks and edit the target, lock, back
  up, write a sibling temp file and `rename(2)` over it, never truncate, verify, and restore on a
  failed verify. No hook command writes one. The delivery order is plugin > own drop-in file >
  merging into a file the user maintains, so the riskiest route is the last resort: with `claude` on
  `PATH`, `~/.claude/settings.json` is still never touched by us, and the plugin route fails loudly
  rather than falling back to it. The Claude Code plugin in `plugins/` *ships* its hook config in
  its own directory, and Claude Code - not this tool - records the install under `enabledPlugins`.
  See `tasks/plans/004-claude-code-plugin.md`.

## Development

Run development tools within the shell `nix develop` creates, or use `. "$HOME/.cargo/env" && [cmd]`.
