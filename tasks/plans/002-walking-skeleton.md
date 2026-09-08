# 002 - Walking skeleton: layout, build, CI/CD and install

Status: done for steps 1-7 - the tool, its tests, the nix package, CI and the docs are
implemented and green. Steps 8 (tag `v0.0.1` and confirm the release workflow) and 9 (verify an
installed package end to end) are open: no remote exists yet.

Covers *how the project is built and shipped*. What it does and why is 001, which is normative;
this file never restates a behavioural decision, only the machinery around it.

## Goal

A **walking** skeleton, in Cockburn's sense: a thin slice through every layer the finished tool will
have, including build and deployment, running end to end. Not a scaffold. When it is done, a real
agent `Stop` hook must put a real ✅ on a real tmux window entry, from a binary that came out
of the packaging path the tool ships to everybody else - not from a `cargo run` in a checkout.

Concretely, the skeleton is done when all six are true:

1. `agent-status set done` writes `@agent_pane_status` and recomputes `@agent_status`.
2. `agent-status clear-window`, bound to the clear-on-focus hooks, clears the non-sticky states
   window-wide.
3. The documented format term renders the glyph, and renders nothing when no agent is present.
4. `cargo test` passes, including an integration test driving a throwaway `tmux -L` server.
5. CI is green on push, and a `v0.0.1` tag produces a GitHub release with binaries.
6. A packaged build can be installed and exercised on a supported system without a development symlink.

Everything else in 001 is deliberately out: see [Skeleton scope](#skeleton-scope).

## Decisions taken here

| Decision | Choice | Why |
| --- | --- | --- |
| Language | Rust | typed state and rank, a real test harness for the rollup, one static binary with no runtime deps beyond tmux |
| Repo name | `tmux-agent-status` | says what it is and where it lives; discoverable; `agent-status` alone is generic and probably taken |
| Executable | `agent-status` | what the hook lines and the docs say; the tmux option prefix `@agent_*` follows it |
| Edition / MSRV | 2024 / 1.85 | MSRV asserted in `Cargo.toml` and verified by a CI job |
| Dependencies | **none** in the skeleton, std only | the whole tool is argument dispatch plus three `tmux` invocations; a hook that runs on every `PostToolUse` should not carry a dependency tree. Revisit when the config file lands - that is the first honest reason for `serde`/`toml`, and possibly `clap`. |
| Licence | MIT | shortest thing that makes it reusable; no CLA, no contributor friction |
| Nix | `rustPlatform.buildRustPackage` with `cargoLock.lockFile` | in nixpkgs, no extra flake input, no vendor hash to regenerate on every dependency change |
| Toolchain source | the flake devShell | local development uses the flake's Rust toolchain; CI uses a released toolchain action, so both paths are exercised |

## Repository layout

```
tmux-agent-status/
├── AGENTS.md               # public-repo rule, tasks/ convention, the traps
├── CLAUDE.md -> AGENTS.md
├── README.md               # what it is, install, the format term, the hook table, interop
├── LICENSE                 # MIT
├── .editorconfig
├── .gitignore
├── justfile                # the only entry point a human types
├── Cargo.toml
├── Cargo.lock              # committed: this is a binary crate
├── rustfmt.toml
├── flake.nix
├── flake.lock
├── default.nix             # non-flake fallback: import ./nix/package.nix
├── nix/
│   └── package.nix         # the derivation, imported by both flake.nix and default.nix
├── src/
│   ├── main.rs             # argument dispatch and exit codes, nothing else
│   ├── state.rs            # State, its rank, parsing, icon
│   ├── rollup.rs           # pure: [Option<State>] -> Option<State>
│   ├── tmux.rs             # the only impure module: the three calls
│   └── bell.rs             # BEL to /dev/tty
├── tests/
│   ├── rollup.rs           # table-driven, no tmux
│   └── tmux_server.rs      # throwaway `tmux -L` server, serialised
├── share/tmux/
│   └── agent-status.conf   # the pane-focus-in hook, plus the format term as a comment
├── docs/
│   └── install.md          # nix flake, nix profile, cargo, prebuilt binary, from source
├── tasks/
│   ├── plans/
│   └── todo/
└── .github/
    ├── dependabot.yml
    └── workflows/
        ├── ci.yml
        └── release.yml
```

Deferred until they have a reason to exist: `man/agent-status.1` (README first), a Homebrew formula
(after demand demonstrates that it is worth maintaining), and `CHANGELOG.md` (generated from
conventional commits when there is a second release, never hand-edited).

## Crate structure: one rule

**`rollup.rs` and `state.rs` know nothing about tmux, and `tmux.rs` knows nothing about policy.**

The rollup is where the design's one non-obvious rule lives - `working` ranks *lowest*, and a pane
with no state is not the same as a pane inheriting the window's state - and it is the part most
likely to be got wrong twice. Keeping it a pure function over `[Option<State>]` makes every ordered
pair of states a table row in `tests/rollup.rs` that runs in microseconds with no server. `tmux.rs`
is then thin enough to be checked by the handful of integration tests that do need a server.

`main.rs` maps arguments to a call and an exit code, and nothing else.

### CLI surface for the skeleton

| Command | Called by | Behaviour |
| --- | --- | --- |
| `agent-status set <state>` | agent hooks | write the pane option, recompute the rollup, ring the BEL for the hard-coded default states (`waiting`, `error`, `done`; not `working`); making the list configurable is deferred |
| `agent-status clear-window [<pane>]` | the clear-on-focus hooks | derive the window from the pane (`$TMUX_PANE` when no argument), clear `@agent_pane_status` on every pane of that window, recompute |
| `agent-status --version` | humans | version **and `std::env::current_exe()`** |
| `agent-status --help` | humans | the two commands, the four states |

`--version` printing the resolved executable path is not decoration. The dev loop below deliberately
shadows the installed binary via `PATH`, and a shadow you cannot see is a shadow that wastes an
afternoon.

### Failure policy

**A hook must never break the agent that called it.** `set` exits 0 when `$TMUX` is unset, when
`$TMUX_PANE` is unset, when the server is gone, or when `tmux` is not on `PATH` - silently, writing
nothing to stdout. It exits non-zero only for a genuinely wrong invocation (an unknown state name),
which is a bug in the user's hook config and should be loud. Anything written for humans goes to
stderr, because agents that capture hook stdout would swallow it otherwise.

## Build tooling

`just` is the single entry point; every recipe is one line a human could have typed, and CI calls
the same recipes so the two cannot drift.

| Recipe | Does |
| --- | --- |
| `just fmt` / `just fmt-check` | `cargo fmt` |
| `just lint` | `cargo clippy --all-targets -- -D warnings` |
| `just test` | `cargo test` |
| `just check` | fmt-check + lint + test - what CI runs |
| `just build` | `cargo build --release` |
| `just link` | symlink `target/debug/agent-status` into `${AGENT_STATUS_BIN_DIR:-$HOME/.local/bin}` (dev shadow) |
| `just unlink` | remove it |
| `just nix-build` | `nix build .#agent-status` |
| `just harness` | bring up the throwaway two-server tmux harness from 001 for manual inspection |

The devShell provides `cargo`, `rustc`, `clippy`, `rustfmt`, `rust-analyzer`, `tmux` and `just`.
There is no `rust-toolchain.toml`: it would only be read by rustup, which is not how the flake
devShell delivers a toolchain, and a pin that nothing enforces is worse than no pin. The MSRV lives in
`Cargo.toml`'s `rust-version` and is enforced by a CI job that builds with exactly that toolchain.

`.envrc` with `use flake` is left to the user rather than committed, since not every contributor has
direnv.

## Testing

Two tiers, and the split matters more than either tier.

**Pure (`tests/rollup.rs`)** - the rank order, the "empty is not inherited" rule, every ordered pair
of two-pane states, the three-pane case from 001. No tmux, no I/O, fast enough to run on every save.

**Against a real server (`tests/tmux_server.rs`)** - the harness from 001: a throwaway
`tmux -L <unique>` server per test, torn down in a guard so a panicking test cannot leak a server.
Agent panes are faked with `bash -c "exec -a claude /bin/sleep 900"`, and the setter is driven with
`TMUX_PANE=%N` in the environment.

Three facts about this tier, each of which will otherwise cost a debugging session:

- **The inheritance trap is a test, not a comment.** `list-panes -F '#{@agent_pane_status}'` must
  print empty for a pane with no state *while* `@agent_status` is set on the window. That single
  assertion is the entire reason two option names exist; if it ever passes with one name, the design
  changed.
- **Rendering needs an attached client.** `#()` jobs in the status format never run without one, so
  the render tests need the two-server sandwich (`-L wmhost` running an attach to `-L wmtest`) and
  `capture-pane -e` to read styles back. Tests that only check option values do not.
- **Serialise anything that shares a socket name.** Distinct `-L` names per test is the cheap fix;
  `cargo test` is threaded and two tests on one socket will interleave.

Skeleton coverage is the first four bullets of 001's verification list plus all three rollup
bullets and the inheritance bullet. The bell, style and truncation cases follow the features they
test.

## CI/CD on GitHub

`.github/workflows/ci.yml`, on push and pull_request, `permissions: contents: read`, with
`concurrency` cancelling superseded runs on the same ref:

| Job | Runner | Does |
| --- | --- | --- |
| `check` | ubuntu | `just fmt-check`, `just lint` |
| `test` | ubuntu + macos | install tmux (`apt-get` / preinstalled on macOS runners; assert `tmux -V` rather than assuming), `just test` |
| `msrv` | ubuntu | build with the exact `rust-version` toolchain |
| `nix` | ubuntu + macos | `nix flake check`, `nix build .#agent-status`, then run the built binary's `--version` |

macOS is not optional. The tool's entire job is talking to tmux, and tmux behaves differently
enough across supported platforms to require coverage on both macOS and Linux.

`.github/workflows/release.yml`, on `push: tags: v*`, `permissions: contents: write`: build
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` and
`aarch64-apple-darwin`, tar each with its README and LICENSE, publish checksums, create the release.

`.github/dependabot.yml` for `cargo` and `github-actions`, weekly. Third-party actions are pinned to
a commit SHA with the version in a trailing comment; dependabot is what keeps those fresh. That is
the standard supply-chain posture for a public repo and it costs one comment per action.

A CI matrix is not a substitute for the manual harness - `just harness` stays, because the things
that actually look wrong (a glyph one column off, a dim quiet window) are seen, not asserted.

## Local development and installation verification

The repository supports three distinct workflows:

| Goal | Method |
| --- | --- |
| make each rebuild live on the next hook event | symlink the debug binary into an early `PATH` directory with `just link` |
| verify the Nix package without installing it | run `nix build` or `nix run` against the checkout |
| verify a packaged installation end to end | install through a documented route, then exercise it through tmux and real agent hooks |

`just link` defaults to `~/.local/bin`, but that path and its precedence are not universal.
`AGENT_STATUS_BIN_DIR` selects any other writable directory already on `PATH`. `just unlink` must be
called with the same value. `agent-status --version` prints the resolved executable so an unexpected
shadow is visible.

`cargo install --path .` is also available, but copies the binary into Cargo's configured binary
directory and must be rerun after every edit. The symlink is therefore the faster development loop.

A Nix build can consume the checkout directly with `nix build path:.#agent-status` or
`nix run path:.#agent-status -- --version`. A separate Nix configuration may temporarily override a
pinned `tmux-agent-status` input with an absolute `path:` URL for end-to-end testing. Such a URL must
not be committed because it is machine-local and its lock entry changes with the checkout contents.

Installation has four independently verifiable parts:

1. Install the package and confirm `agent-status --version` resolves to it.
2. Source the shipped tmux snippet from whichever stable path the selected installation method provides.
3. Add the documented term to both window status formats and verify a manually set glyph renders.
4. Add the agent hooks, exercise a real turn, and read `@agent_status` back to confirm the hook's
   environment can find the command on `PATH`.

Package first, snippet second, format term third, and hooks last. This order keeps each failure
isolated from the next integration layer.

## What implementing it corrected

Each of these was verified on a throwaway server and changed something this plan or 001 had
assumed. They are recorded here rather than silently applied.

| Assumed | Actually | Consequence |
| --- | --- | --- |
| `pane-focus-in` clears on focus | it fires only with `focus-events on` *and* a client attached | ship `session-window-changed` + `window-pane-changed`, which fire in every setup |
| the hook can read `$TMUX_PANE` | tmux's `run-shell` sets `$TMUX` but no `$TMUX_PANE`; it *does* expand `#{...}` in the command | `clear-window [<pane>]` takes the pane as an argument, defaulting to `$TMUX_PANE` |
| `@agent_status` holds - unstated | it must hold the **glyph**, since the documented term renders it directly | `@agent_pane_status` holds the state name; the opt-in recolour compares the glyph, not `error` |
| `#{=/25/…:#W}` truncates the name | `#W` does not expand inside a modifier (tmux 3.6); it yields empty | the documented example format uses `#{window_name}` |
| emoji render anywhere | a tmux **client** with no UTF-8 locale renders them as underscores | the stored option is unaffected; the harness forces `tmux -u`, and the README says so |

Two files exist that the layout above does not list, both to keep the one crate rule intact:
`src/lib.rs`, so the pure tier can be an integration test rather than a `#[cfg(test)]` module, and
`src/command.rs`, so `main.rs` stays argument dispatch and `tmux.rs` stays free of policy.

## Skeleton scope

**In:** `set` and `clear-window`, the four states, the rank, the rollup, the clear-on-focus hooks,
the BEL, the format term, the two test tiers, the flake package, CI, one tagged release, and
end-to-end installation verification.

**Out, and each for a reason:**

| Deferred | Until |
| --- | --- |
| config file (`nerdfont`, the `status_icons` overrides, which states ring) | the defaults are settled in 001 and ship as constants; the file that makes them overridable is the first real dependency (`serde`/`toml`) and can wait |
| agents beyond the first supported one | the per-agent tables are data; adding a second agent is what shows whether the shape is right, and it is not what the skeleton is proving |
| the optional `error` recolour snippet | documented in the README as opt-in, shipped as a comment in the tmux snippet - no code |
| the `stale` 💤 state | decided in 001 and explicitly not first-version: it needs a stored timestamp and age arithmetic in the format string |
| man page, Homebrew formula, CHANGELOG | README first, one shared tap later, generated changelog at the second release |
| the process-ancestry fallback for pane resolution | `$TMUX_PANE` is present in every hook environment that matters; the fallback is for cases none of which have been observed |

## Order of work

1. `cargo init`, `Cargo.toml`, `rustfmt.toml`, `.editorconfig`, `.gitignore`, LICENSE, `justfile`.
2. `state.rs` + `rollup.rs` + `tests/rollup.rs`. Pure, complete, table-driven. No tmux yet.
3. `tmux.rs` + `main.rs` + the failure policy. `agent-status set` works by hand under a real server.
4. `tests/tmux_server.rs`, including the inheritance assertion.
5. `share/tmux/agent-status.conf`, `clear-window`, the focus behaviour.
6. `flake.nix` + `nix/package.nix` + `default.nix`; `nix build` produces a working binary.
7. CI, then README and `docs/install.md`.
8. Tag `v0.0.1`, confirm the release workflow.
9. Install a packaged build through a documented route and read `@agent_status` back after a real hook event.

Steps 1-5 are the tool, 6-8 are the delivery path, 9 is what makes it walking rather than standing.

## Open

- **No remote is configured yet.** The repo will live at
  `github.com/gerbenoostra/tmux-agent-status`; nothing has been pushed, so steps 8 and 9 of the
  order of work are untouched and CI has never run.
- Whether the release workflow should also publish to crates.io. `cargo install agent-status` is a
  cheap extra install route, but it claims a name on a shared registry and the binary is useless
  without tmux config, so the README has to carry the rest anyway.
