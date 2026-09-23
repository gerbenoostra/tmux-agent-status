# Agent notes for tmux-agent-status

This repository is **public and open source**: nothing in it may contain local paths, machine
names, personal configuration, secrets or anything else that is not useful to a stranger who
cloned it.

## What this tool is

`tmux-agent-status` turns agent lifecycle events into one glyph on the tmux window entry. The hook
commands (`set`, `reset`, `finish`, `clear-pane`, `notify`) write two tmux options
and ring the terminal bell, and nothing else: they never touch a window name, never shell out to
git, never write a state file, never edit any config file, and never spawn a daemon. The one
exception is the `register` subcommand, which a human types and which writes config files under the
safe-write contract. The design rules that are easy to break - including that contract - are in
`CONTRIBUTING.md`; read it before changing behaviour.

## Development

Run development tools within the shell `nix develop` creates, or use `. "$HOME/.cargo/env" && [cmd]`.

## Docs Placement

Agent/tool setup documentation goes in the agent-setup section (`docs/agents/`), not `docs/install.md`.
Per-agent config docs follow the naming of the sibling agent docs (e.g. `claude-code.md`), not `CLAUDE.md`.
