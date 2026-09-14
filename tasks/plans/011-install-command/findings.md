# 011 - findings

## Generated config detection

A generated config is one the user cannot usefully edit, because the next `home-manager switch`,
`nix profile upgrade` or `stow` run puts it back. Verified against a real home-manager machine, both
of these chains exist side by side:

| Case | Chain | Final target | Verdict |
| --- | --- | --- | --- |
| `mkOutOfStoreSymlink` | `~/.tmux.conf` -> `...home-manager-files/.tmux.conf` -> `...hm_.tmux.conf` -> `~/dotfiles/home/.tmux.conf` | a real, writable, git-tracked file the user maintains | **edit it** |
| a build product | `~/.tmux/tmux-agent-status.conf` -> `/nix/store/...-tmux-agent-status-0.0.1/share/tmux/...` | `r--r--r--`, root-owned, on a read-only store | **never edit** |

Both chains pass *through* `/nix/store`. Only one *ends* there. The filesystem writability of the
final target, not a path prefix, is the signal that distinguishes the two cases.

## Agent config formats

- Droid puts event names at the top level, with no wrapping `hooks` key. Nesting them under `hooks`
  gives a config Droid ignores silently.
- Devin user-scope config nests the whole hook map under `hooks` in `~/.config/devin/config.json`,
  and one unknown event key discards the entire hook map.
- Mistral Vibe is TOML.

## TOML append safety

An appended `[[hooks]]` header is valid TOML after almost anything, but a file whose last line is
inside an unclosed multi-line string (`"""` or `'''`) swallows the block into that string.

## tmux config semantics

- `tmux display-message -p '#{config_files}'` returns candidate paths, including files that do not
  exist; when the server was started with `-f` it names only that file.
- tmux executes config commands strictly in the order it encounters them, including inside
  `source-file`; the last assignment of an option wins.
- tmux expands and sorts globs for `source-file` in a deterministic way.
- The compiled-in default format on tmux 3.6a is
  `#I:#W#{?window_flags,#{window_flags}, }`.
- A bare value containing a space is discarded by tmux and the option keeps its default.
- A `;` met outside any quote separates commands; a `;` inside single or double quotes is part of
  the value.
- tmux parses the whole config file before running any of it. An unknown command name or a valid
  command with a stray extra argument causes tmux to abandon the entire config file.
- tmux resolves a relative `source-file` path against the process's working directory, not the
  config file's directory.

## Detection

- Guessing an agent's binary name gives wrong preselections; names are taken from verified installs.
- An agent is preselected when either its config directory exists or its command is on `PATH`.
- `claude plugin marketplace list --json` and `claude plugin list --json` are non-interactive and
  machine-readable. A marketplace can be registered as a `directory` source pointing at a local
  checkout.

## Cargo install

- `cargo install` ships only the binary and nothing else, so runtime path lookups for bundled data
  would fail for that install route.

## Filesystem behavior

- `rename(2)` over an `r--r--r--` file succeeds when its parent directory is writable. So the
  permission that matters for replacing a file is the directory's, not the file's mode.
- Reserialising an already-correct JSON file with `serde_json` (without `preserve_order`) changed
  2906 bytes into 3284 with no semantic change at all. This makes a byte-level "already installed"
  test rewrite a hand-maintained config that is already correct.

## llvm-cov region accounting

`cargo llvm-cov --fail-under-regions 100` and `cargo llvm-cov show` do not agree, and the
disagreement is the tool's, not the test suite's.

- `src/` is compiled twice: once with `cfg(test)` for the lib's own test binary, once as the rlib
  the integration tests link. The two builds carry different crate disambiguators
  (`Cs6EzAbLpPYWM` against `CsaeC7gsJITlt`), so every function exists twice under different mangled
  names and `llvm-cov report` leaves a handful of spans unmerged between them.
- The result: `report` counted 21 missed regions and 5 missed lines across `install/{mod,tmux_conf,
  write}.rs` for which `llvm-cov show` renders no uncovered region at all, and for which the
  exported segments hold no zero-count entry. Verified: not object-order dependent, not caused by
  any one test binary (leave-one-out over all 19 objects), and not inside the inline `#[cfg(test)]`
  modules.
- The exported segments *are* the view `show` renders, and they agree with `report` exactly for the
  files that have no duplicate-span problem (`agents.rs` 1, `probe.rs` 5, `prompt.rs` 53). A
  segment with `has-count` true, a count of zero and no gap flag is a region nothing reached. That
  is what the `coverage` recipe gates on.
- Full region coverage implies full line coverage, so the region bar alone is enough.

## Coverage exemptions

Neither line nor attribute exemptions are available on stable:

- `llvm-cov show --help` offers exactly one exclusion knob, `--ignore-filename-regex`. There is no
  `LCOV_EXCL_LINE` equivalent; that belongs to lcov and grcov.
- `#[coverage(off)]` is still unstable (`E0658`, rust-lang/rust#84605) on 1.98.1. cargo-llvm-cov's
  supported spelling, `#[cfg_attr(coverage_nightly, coverage(off))]`, needs a nightly toolchain.

Since the gate is our own script, the marker is too: a trailing `// coverage: off` on the line the
uncovered segment points at.
