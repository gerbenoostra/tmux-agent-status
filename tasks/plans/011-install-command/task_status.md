# 011 - task status / handover

## Current state

- Branch: `feat/install-command`
- Work items 1-9 are implemented and committed. Item 10, documentation, has not started.
- The coverage gate passes: every region of `src/` is reached, bar five that carry a
  `// coverage: off` marker and the reason they cannot be.

## Verification

- `just fmt-check`: clean.
- `just lint`: clean.
- `cargo test --all-targets`: 405 tests across 18 suites, all passing.
- `just coverage`: passes. The summary table it prints still shows misses; those are counted per
  compilation rather than per region and are an llvm-cov artefact, which is why the bar is read
  from the merged region view instead. See [findings.md](./findings.md) and
  [decisions.md](./decisions.md).

## Next steps

1. Item 10: documentation. README, `docs/agents/README.md`, `docs/install.md` and the plugin
   doctor, following the docs-placement rule in `AGENTS.md`.
2. The manual real-machine checks below.

## Manual checks

Pending:

- A real `install` on a machine with several agents, followed by a real turn producing a real glyph,
  with `git diff` in the dotfiles repo showing exactly the intended edits and nothing else.
- The Claude Code plugin route end to end, against a throwaway `HOME`, confirming that
  `~/.claude/settings.json` still has no `hooks` entry of ours.
- A home-manager machine, both chains: a `mkOutOfStoreSymlink` `~/.tmux.conf` is edited in the
  dotfiles checkout and the symlinks survive; a genuinely store-resident file is refused and prints
  something the user can paste into their generator.
