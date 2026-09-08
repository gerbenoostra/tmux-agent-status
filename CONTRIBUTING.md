# Contributing

## Development shell

```sh
nix develop          # cargo, clippy, rustfmt, rust-analyzer, tmux, just
just check           # fmt-check + lint + test, exactly what CI runs
just harness         # a throwaway tmux server showing all four states, to look at
just link            # shadow the installed binary with this checkout's debug build
```

`just check` is what CI runs. `just harness` starts a throwaway tmux server that displays all four states so you can inspect the glyphs.

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

1. Run `just check` and `just nix-build`.
2. Install the resulting package or release binary using one of the documented installation routes.
3. Confirm `tmux-agent-status --version` resolves to that installed binary.
4. Start a fresh tmux server or reload the shipped snippet, then exercise the configured agent hooks.
5. Confirm each state reaches `@agent_status` and that focusing its window clears non-sticky states.

For Nix, the checkout itself can be tested without changing another configuration:

```sh
nix build path:.#tmux-agent-status
./result/bin/tmux-agent-status --version
```

If testing through a separate system or home-manager flake, temporarily override its
`tmux-agent-status` input with `path:/absolute/path/to/this/checkout`. The exact rebuild command is
specific to that configuration. Do not commit the `path:` input: it is machine-local, and its lock
entry changes with the checkout contents.
