# 002 - Walking skeleton: layout, build, CI/CD and install

Status: todo - plan written, nothing implemented

Covers *how the project is built and shipped*. What it does and why is 001, which is normative;
this file never restates a behavioural decision, only the machinery around it.

## Goal

A **walking** skeleton, in Cockburn's sense: a thin slice through every layer the finished tool will
have, including build and deployment, running end to end. Not a scaffold. When it is done, a real
agent `Stop` hook must put a real ✅ on a real tmux window entry, from a binary that came out
of the packaging path the tool ships to everybody else - not from a `cargo run` in a checkout.

Concretely, the skeleton is done when all six are true:

1. `agent-status set done` writes `@agent_pane_status` and recomputes `@agent_status`.
2. `agent-status clear-window`, bound to `pane-focus-in`, clears the non-sticky states window-wide.
3. The documented format term renders the glyph, and renders nothing when no agent is present.
4. `cargo test` passes, including an integration test driving a throwaway `tmux -L` server.
5. CI is green on push, and a `v0.0.1` tag produces a GitHub release with binaries.
6. It is installable on macOS through the dotfiles flake input, not through a dev symlink.

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
(one shared tap across the sibling tools, not a tap per tool), `CHANGELOG.md` (generated from
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
| `agent-status clear-window` | `pane-focus-in` | read `$TMUX_PANE`, derive the window, clear `@agent_pane_status` on every pane of that window, recompute |
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
| `just link` | symlink `~/.local/bin/agent-status` at `target/debug/agent-status` (dev shadow) |
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

macOS is not optional. The tool's entire job is talking to tmux, tmux behaves differently enough
across platforms to matter, and this tool is used on both.

`.github/workflows/release.yml`, on `push: tags: v*`, `permissions: contents: write`: build
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` and
`aarch64-apple-darwin`, tar each with its README and LICENSE, publish checksums, create the release.

`.github/dependabot.yml` for `cargo` and `github-actions`, weekly. Third-party actions are pinned to
a commit SHA with the version in a trailing comment; dependabot is what keeps those fresh. That is
the standard supply-chain posture for a public repo and it costs one comment per action.

A CI matrix is not a substitute for the manual harness - `just harness` stays, because the things
that actually look wrong (a glyph one column off, a dim quiet window) are seen, not asserted.

## Installing it the dotfiles way

The two questions this plan was asked to answer, in order.

### Can nix install a package from a local folder?

Yes, three ways - but they are not three candidates for one job. Each answers a different question,
two are in routine use, one is rejected outright, and exactly one carries a rule about where it may
appear. Together with the dev symlink from the next section:

| Want | Reach for | Verdict |
| --- | --- | --- |
| an edit live on the next hook fire | `just link`, which is not nix at all - next section | **the everyday loop** |
| "does the packaging build and run?" | `nix build` / `nix run` on the checkout | **use freely** |
| the real config, built once from a local checkout | `--override-input` at switch time | **use when it matters, never committed** |
| a local checkout wired into the config permanently | a `path:` flake input | **rejected** |

**1. A `path:` flake input.** `inputs.tmux-agent-status.url = "path:/home/you/src/tmux-agent-status"`.
The obvious thing to reach for, and wrong for a config shared by two machines: the URL is an
absolute machine-local path, so committing it breaks the other machine, and the lock records a
`narHash` of the directory that changes on **every edit**, so each rebuild needs a
`nix flake lock --update-input` first. **Rejected** for the tracked config. Fine in a throwaway
flake, where neither problem exists.

**2. `nix run` / `nix build` against the checkout.** `nix run ~/src/tmux-agent-status -- --version`
builds and runs the local flake with the real derivation and touches no config at all. **Use
freely** - it installs nothing, so there is nothing to clean up or forget. This is how "does my
packaging work" gets answered, and it is what `just nix-build` wraps.

**3. `--override-input` at switch time.** The dotfiles flake pins the GitHub input as normal, and a
local checkout is swapped in for one rebuild:

```sh
sudo darwin-rebuild switch --flake "$HOME/.dotfiles#hostname" \
  --override-input tmux-agent-status path:$HOME/src/tmux-agent-status
```

**Use when it matters, never committed.** This is the only way to see a local build inside the real
config - the installed binary, on the real PATH, under the real hooks - so it is what a change gets
verified with before it is tagged. Nothing is committed, the lock is untouched, and dropping the
flag reverts. It costs a full rebuild per iteration, which is why it is not the everyday loop and
why option 1 is not worth the persistence it would buy.

### Can it be installed editable?

Not in the `pip install -e` sense, and no packaging trick changes that: the artifact is a compiled
binary and a nix store path is immutable by design. What "editable" has to mean here is *an edit is
live on the next hook fire without a rebuild and without sudo*, and `PATH` already provides it.

On a typical home-manager + nix-darwin setup, `~/.local/bin` sits ahead of the profile path where
home-manager installs packages, so a symlink there shadows the installed binary. Adjust if your
`PATH` ordering differs. So:

```sh
just link     # ~/.local/bin/agent-status -> <checkout>/target/debug/agent-status
cargo build   # every rebuild is live for the next hook fire
just unlink   # back to the installed binary
```

No rebuild, no sudo, instantly reversible, and it shadows the installed binary rather than replacing
it. The one hazard is that the shadow is invisible, which is exactly what `--version` printing
`current_exe()` is for. `just link` prints the same warning.

Rejected: `cargo install --path .` into `~/.cargo/bin`. Same shadowing effect but it copies rather
than symlinks, so it needs re-running on every edit - strictly worse than the symlink, for the same
risk.

### What the dotfiles repo has to add

Four changes, each small and each reviewable in a diff. This is the whole install surface.

**1. A flake input and its package.** In `flake.nix`:

```nix
tmux-agent-status.url = "github:gerbenoostra/tmux-agent-status";
tmux-agent-status.inputs.nixpkgs.follows = "nixpkgs";
```

and in the shared home-manager file, the package added to `home.packages`. The input is pinned and
updated on its own, so a bad bump here never blocks an unrelated change - the argument for one repo
per tool, paying off at the first update.

**2. The tmux snippet, at a stable path.** `~/.tmux.conf` is an out-of-store symlink into the
dotfiles repo, so it cannot interpolate a nix store path. Home-manager places the shipped snippet at
a fixed location instead:

```nix
home.file.".tmux/agent-status.conf".source =
  "${inputs.tmux-agent-status.packages.${pkgs.system}.agent-status}/share/tmux/agent-status.conf";
```

and `.tmux.conf` gains one line, `source-file ~/.tmux/agent-status.conf`, which brings the
`pane-focus-in` hook with it. This is the same pattern used for other out-of-store tmux includes.

**3. The format term, pasted by hand.** Deliberately *not* automated - 001's hardest decision is
that the tool does string surgery on nobody's format. It is one insertion into each of
`window-status-format` and `window-status-current-format`, between the closing `}}` of the
truncation and `#{?window_flags,...}`:

```tmux
#{?@agent_status, #{@agent_status},}
```

**4. The agent hooks, pasted by hand.** The entries from 001's hook table call `agent-status` from
`PATH`. The setter rings the bell itself, so any separate `printf '\a'` hooks for the same events
should be removed rather than kept alongside.

One thing to verify rather than assume during step 4: that agent hooks run with a `PATH` that
includes the home-manager profile. A hook that silently exits 0 when it cannot find `agent-status`
- which is the correct failure policy - is also a hook that fails invisibly. The first `set done`
must be confirmed by reading `@agent_status` back, not by looking at the status bar and believing it.

### Order of installation

Package first, snippet second, format term third, hooks last. Each step is verifiable on its own
(`agent-status --version`; `tmux show-hooks -g | grep focus`; a hand-set `@agent_status` renders;
a real turn sets it), and doing them in this order means no step is ever debugged through another.

## Skeleton scope

**In:** `set` and `clear-window`, the four states, the rank, the rollup, the pane-focus-in hook, the
BEL, the format term, the two test tiers, the flake package, CI, one tagged release, the dotfiles
install.

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
9. The four dotfiles changes, in the order above, and read `@agent_status` back.

Steps 1-5 are the tool, 6-8 are the delivery path, 9 is what makes it walking rather than standing.

## Open

- **No remote is configured yet.** The repo will live at
  `github.com/gerbenoostra/tmux-agent-status`; nothing has been pushed.
- Whether the release workflow should also publish to crates.io. `cargo install agent-status` is a
  cheap extra install route, but it claims a name on a shared registry and the binary is useless
  without tmux config, so the README has to carry the rest anyway.
