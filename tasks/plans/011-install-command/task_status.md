# 011 - task status / handover

## Current state

- Branch: `feat/install-command`
- All ten work items are implemented. `Cargo.toml` already carried both dependencies item 10 asks
  for, so that item was docs only: the README leads on `install` and keeps the four manual steps,
  every route in `docs/install.md` ends with the one-liner, `docs/agents/README.md` says which
  agents are covered and by which of the three deliveries, and the plugin doctor names the
  repairing command per failing check without ever running it.
- A review of the branch found nine defects, all fixed on it; each is one commit with the failure
  it removes in the message. Three were worth the review on their own:
  - a format line spread over continuations could never be rewritten, because the joined logical
    line was compared against one physical line. The step failed with "the config moved under us"
    on a file nothing had touched, and where only one of the two options wrapped it left the term
    in exactly one of them.
  - the tmux probe asked only whether anything unexpected moved, which an edit that changes nothing
    passes. A line spliced where a later assignment overrides it reported success and produced no
    glyph. The probe is now asked what the edit was *for*.
  - a relative `source-file` was followed from beside the config rather than from `$HOME`, so the
    walk could pick a winner out of a file tmux never opened.
- The coverage gate passes: every region of `src/` is reached, bar four that carry a
  `// coverage: off` marker and the reason they cannot be. The fifth, a process with no `$HOME`,
  is now covered by `tests/install_no_home.rs` rather than exempted.

## Verification

- `just fmt-check`: clean.
- `just lint`: clean.
- `cargo test --all-targets`: 417 tests across 19 suites, all passing.
- `just coverage`: passes. The summary table it prints still shows misses; those are counted per
  compilation rather than per region and are an llvm-cov artefact, which is why the bar is read
  from the merged region view instead. See [findings.md](./findings.md) and
  [decisions.md](./decisions.md).

## Next steps

The manual real-machine checks below are all that is left before this branch is ready to merge.

## Manual checks

Pending:

- A real `install` on a machine with several agents, followed by a real turn producing a real glyph,
  with `git diff` in the dotfiles repo showing exactly the intended edits and nothing else.
- The Claude Code plugin route end to end, against a throwaway `HOME`, confirming that
  `~/.claude/settings.json` still has no `hooks` entry of ours.
- A home-manager machine, both chains: a `mkOutOfStoreSymlink` `~/.tmux.conf` is edited in the
  dotfiles checkout and the symlinks survive; a genuinely store-resident file is refused and prints
  something the user can paste into their generator.
