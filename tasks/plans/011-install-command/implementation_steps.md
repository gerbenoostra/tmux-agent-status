# 011 - Implementation steps

These are the steps to implement the `install` subcommand, in order. Each step is a self-contained
file under `steps/` with scope, relevant design, implementation notes, and verification for that
file or task.

1. **[Update `AGENTS.md`](./steps/01-agents-md.md)** - prerequisite rule rewrite so the always-on
   rules no longer forbid what `install` does.
2. **[Implement `src/install/mod.rs`](./steps/02-install-mod-rs.md)** - orchestration: the five
   phases, the step-selection algebra, the `Change` type, and the summary.
3. **[Implement `src/install/write.rs`](./steps/03-install-write-rs.md)** - the safe-write
   primitive used by every file this tool touches.
4. **[Implement `src/install/agents.rs`](./steps/04-install-agents-rs.md)** - agent table,
   detection, JSON merges, TOML append, and the Claude Code plugin route.
5. **[Implement `src/install/tmux_conf.rs`](./steps/05-install-tmux-conf-rs.md)** - tmux config and
   snippet discovery, and the `source-file` block.
6. **[Implement `src/install/format.rs`](./steps/06-install-format-rs.md)** - tmux format line
   parser, splice, and requoting.
7. **[Implement `src/install/probe.rs`](./steps/07-install-probe-rs.md)** - throwaway tmux server for
   the compiled-in default, baseline, and semantic verify.
8. **[Implement `src/install/prompt.rs`](./steps/08-install-prompt-rs.md)** - the only module that
   knows about a TTY; handles `-y` and `--dry-run`.
9. **[Update `src/main.rs`](./steps/09-main-rs.md)** - `install` subcommand and flags in the
   pico-args dispatch, plus help text.
10. **[Update `Cargo.toml` and docs](./steps/10-cargo-and-docs.md)** - add `serde_json`
    `preserve_order` and the prompt crate; update README and install docs.

The high-level design, user-facing spec, decisions, and external-tool findings are in the sibling
files:

- [design.md](./design.md)
- [spec.md](./spec.md)
- [decisions.md](./decisions.md)
- [findings.md](./findings.md)

## Verification note

The original numbered verification list has been distributed into the step files it belongs to.
Each step file contains the tests and checks required to verify that step.
