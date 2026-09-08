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

To get live edits applied to your system, you can symlink to the built artifact in the repo, overriding the version that you installed on the system.
Then on each new `cargo build`, your changes are immediately applied.
```sh
just link      # ~/.local/bin/agent-status -> <checkout>/target/debug/agent-status
cargo build    # every rebuild is live on the next hook fire
just unlink    # back to the installed binary
```

This assumes `~/.local/bin` comes before your nix profile on `PATH`; check with `which -a
agent-status`, and call `agent-status --version` to print the executable it actually resolved to.

Alternatively, you can manually install your local version as follows: `cargo install --path .`. This copies rather than symlinks and thus needs
re-running on every edit. This still shadows any other installs.

## Packaging

**Does my packaging work?**
To verify packaging work, build the local flake; it installs nothing.

```sh
nix run . -- --version
just nix-build
```

## Last check before tagging

A local build inside the real config, on the real `PATH`, under the real hooks:

```sh
darwin-rebuild switch --flake "$HOME/.dotfiles#hostname" \
  --override-input tmux-agent-status "path:$HOME/src/tmux-agent-status"
```

Nothing is committed and the lock is untouched; dropping the flag reverts. Do not commit a `path:`
input instead: it is an absolute machine-local path, and its lock entry records a hash of the
directory that changes on every edit.
