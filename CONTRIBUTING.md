# Contributing

## Development shell

```sh
nix develop          # cargo, clippy, rustfmt, rust-analyzer, tmux, just
just check           # fmt-check + lint + test, exactly what CI runs
just harness         # a throwaway tmux server showing all four states, to look at
just link            # shadow the installed binary with this checkout's debug build
just check-plugin    # validate the Claude Code plugin manifests (needs the `claude` CLI)
```

`just check` is what CI runs. `just harness` starts a throwaway tmux server that displays all four states so you can inspect the glyphs.

## Design rules that are easy to break

These were each verified against a real tmux server and each has been the
subject of a bug. Read this section before changing behaviour.

- **Never call `set-option` on `window-status-format` or
  `window-status-current-format`, at any scope, ever.** Writing a spliced copy
  to a window-local option freezes that window's format forever: a later
  `~/.tmux.conf` reload updates only untouched windows. Reading it is fine, and
  is how a setup check tells the user whether the term is present:
  `show-options` yes, `set-option` never. A test asserts the option name never
  appears as a `set-option` argument anywhere in `src/`. The hazard is a
  property of the tmux *option*, not of the format string, so editing the
  **text of the user's config file** is allowed - it is what
  `register --tmux-format` does, the same edit `docs/register.md` asks the user
  to make by hand.
- **`@agent_pane_status` (per pane) and `@agent_status` (per window rollup)
  are two names on purpose.** tmux option inheritance makes a pane with no
  status read back as the window's value, so a rollup stored in the same option
  name it reduces can no longer tell "unset" from "inherited". They can never
  be merged into one.
- **State decisions happen inside the tmux server, in one command.** Agents
  run hooks concurrently, so a value read in one tmux call can be stale by the
  time a later call writes it. `set-option -F` expands its format against the
  target before setting it, and the server runs one command at a time, so a
  format that reads the pane's own option and picks the new value is a
  compare-and-set. Never implement this as a Rust-side read-then-write, and
  never add a lock or state file.
- **An empty value is normalised to unset atomically.** A `-F` write can only
  produce a value, so a clear produces `""`. Each touched option then gets an
  `if-shell -F '#{?OPTION,,1}' 'set-option -u ...'`, which only ever removes an
  empty value and decides on the value current at that instant. An
  unconditional unset could erase a concurrent write that landed in between.
- **Two orderings on the same states, each named for its question.** Within a
  pane, a later state replaces an earlier one only if it does not rank lower in
  precedence: `error` > `done` > `waiting` > `working`. A `waiting` after a
  finished turn must not demote a `done` nobody has seen. Across panes the
  rollup rank is `waiting` > `error` > `done` > `working`, answering which pane
  wants you most. `start` is the only write that does not defer to what the
  pane holds - typing a prompt is seeing the pane, so that event clears what
  the last turn left.
- **Watched means `window_active` *and* `session_attached`.** tmux calls a
  detached session's current window active, but nobody is looking at it, so a
  turn that ends while the client is away must leave its glyph for the next
  attach. Unreadable flags conservatively mean "not watched" rather than
  dropping the event.
- **`#W` does not expand inside a format modifier** such as
  `#{=/25/…:#W}` (observed on tmux 3.6); use `#{window_name}` there.

## The `register` write contract

`register` is the only code that edits user files, and only when a human types
it. The contract that licences it:

- Resolve the symlink chain and edit the target; the symlink must still be a
  symlink afterwards.
- Lock with an adjacent `O_CREAT|O_EXCL` lock file held through verification.
  Its record carries PID, process start time and hostname; break it only when
  that exact process is provably gone. Age or PID alone is unsafe, and a lock
  from another host or of unknown liveness is never broken.
- Back up to an adjacent, mode-preserving, fsynced, uniquely timestamped file;
  never reused or overwritten.
- Never truncate. Write a sibling temp file, fsync it, `rename(2)` over the
  target, fsync the parent directory.
- Permission checks look at the resolved target's parent directory:
  `rename(2)` can replace a read-only file when its directory is writable. A
  read-only target warns; an unwritable parent or a non-regular target is
  refused.
- Immediately before the rename, re-check the file's `(length, mtime, hash)`
  fingerprint against what the plan read. After the rename, verify; restore
  the backup automatically only if the result is empty, truncated or
  unparseable. If a complete parseable third-party write won the race, do not
  restore over it - report the current file, the backup and the temp file for
  manual reconciliation.
- "Already installed" is semantic, not byte equality: a semantically complete
  config produces no rewrite and no backup. JSON merges preserve key order.
  Marker comments identify blocks this tool owns; command/name identity
  detects equivalent unmarked manual installs and prevents duplicates.
- Delivery order is plugin > own drop-in file > merging into a file the user
  maintains, so the riskiest route is the last resort. With `claude` on
  `PATH`, `~/.claude/settings.json` is still never touched by us, and the
  plugin route fails loudly rather than falling back to it. The Claude Code
  plugin in `plugins/` *ships* its hook config in its own `hooks/hooks.json`
  (not inline in `plugin.json`, whose `hooks` field Claude's validator does
  not check), and Claude Code - not this tool - records the install under
  `enabledPlugins`. If the plugin is already installed, registration stops
  there without touching the marketplace: a local `directory` marketplace is a
  legitimate development setup that re-adding the GitHub marketplace would
  silently replace.
- tmux config commands execute in encounter order across `source-file`, last
  assignment wins, and relative `source-file` paths resolve against the tmux
  process working directory - not the containing config's directory. Format
  registration must reconstruct that order and edit the winning assignment.
  tmux parses the whole config before executing it, so a single bad line can
  abandon everything while option queries still return defaults; verification
  diffs a throwaway server's full before/after option and hook dumps rather
  than trusting exit codes.

## Working on it against your real config

To apply live edits to your system, symlink the built artifact from the checkout into a writable
directory that appears on `PATH` before any installed copy. The recipes default to `~/.local/bin`:

```sh
just link      # ~/.local/bin/tmux-agent-status -> <checkout>/target/debug/tmux-agent-status
cargo build    # every rebuild is live on the next hook fire
just unlink    # back to the installed binary
```

`~/.local/bin` is only a default. If your system uses another user executable directory, set the
same destination for both commands:

```sh
TMUX_AGENT_STATUS_BIN_DIR="$HOME/bin" just link
TMUX_AGENT_STATUS_BIN_DIR="$HOME/bin" just unlink
```

Use any writable directory already on your `PATH`, or add one to `PATH` first. Check precedence with
`command -v tmux-agent-status` or `which -a tmux-agent-status`; `tmux-agent-status --version` prints the executable
that actually ran.

Alternatively, `cargo install --path .` copies the local version into Cargo's configured binary
directory, normally `~/.cargo/bin`. It must be rerun after every edit and can still shadow another
installation.

## Debugging tmux hooks
Your tmux is configured with two tmux hooks, both calling `tmux-agent-status clear-window <pane>`,
which clears the window's non-sticky states (`working` survives). The pane argument is optional and
positional: the hooks pass `#{pane_id}`, which expands to an empty value when no pane is available.
For manual calls the pane resolves in this order: an explicit argument (`--pane` or the positional),
`$TMUX_AGENT_STATUS_PANE`, `$TMUX_PANE`.

## Debugging dropped events

Set `TMUX_AGENT_STATUS_DEBUG=1` to log diagnostic messages to stderr. When the `notify`
subcommand receives a payload that does not match any known event mapping, it silently ignores it
(exits 0, writes nothing). With this flag, those dropped events are printed to stderr with a
`tmux-agent-status:` prefix, so you can tell what arrived and why it was ignored.

This is useful when capturing fixtures for a new agent, chasing an upstream payload change, or
verifying that a mapping table entry is spelled correctly. Never logs to stdout, which agents may
parse. Any non-empty value counts; the documented spelling is `=1`.

## Packaging

**Does my packaging work?**
To verify packaging works, build the local flake; it installs nothing.

```sh
nix run . -- --version
just nix-build
```

## Last check before tagging

Verify the release build end to end on a supported system without relying on the development
symlink:

1. Bump `version` in `Cargo.toml` **and** in `plugins/tmux-agent-status/.claude-plugin/plugin.json`;
   `just test` fails if only one of them moves.
2. Run `just check`, `just check-plugin` and `just nix-build`.
3. Install the resulting package or release binary using one of the documented installation routes.
4. Confirm `tmux-agent-status --version` resolves to that installed binary.
5. Start a fresh tmux server or reload the shipped snippet, then exercise the configured agent hooks.
6. Confirm each state reaches `@agent_status` and that focusing its window clears non-sticky states.

For Nix, the checkout itself can be tested without changing another configuration:

```sh
nix build path:.#tmux-agent-status
./result/bin/tmux-agent-status --version
```

If testing through a separate system or home-manager flake, temporarily override its
`tmux-agent-status` input with `path:/absolute/path/to/this/checkout`. The exact rebuild command is
specific to that configuration. Do not commit the `path:` input: it is machine-local, and its lock
entry changes with the checkout contents.
