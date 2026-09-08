# Installing agent-status

Five routes. Whichever you pick, finish with the four setup steps in the [README](../README.md):
the binary on its own does nothing until tmux and your agent know about it.

## Nix flake input (home-manager, nix-darwin)

The way this tool is meant to be installed on a machine whose config is tracked. Four changes.

**1. The input and the package.** In your `flake.nix`:

```nix
inputs.tmux-agent-status.url = "github:gerbenoostra/tmux-agent-status";
inputs.tmux-agent-status.inputs.nixpkgs.follows = "nixpkgs";
```

and in the home-manager module:

```nix
home.packages = [ inputs.tmux-agent-status.packages.${pkgs.system}.agent-status ];
```

The input is pinned and updated on its own, so a bad bump here never blocks an unrelated change.

**2. The tmux snippet, at a stable path.** If `~/.tmux.conf` is an out-of-store symlink into a
dotfiles repo it cannot interpolate a nix store path, so let home-manager place the snippet at a
fixed location:

```nix
home.file.".tmux/agent-status.conf".source =
  "${inputs.tmux-agent-status.packages.${pkgs.system}.agent-status}/share/tmux/agent-status.conf";
```

and add one line to `.tmux.conf`:

```tmux
source-file ~/.tmux/agent-status.conf
```

**3 and 4.** The format term and the agent hooks, pasted by hand - see the README. Neither is
automated on purpose: this tool rewrites neither your tmux format nor your agent settings file.

Verify each step before starting the next: `agent-status --version`;
`tmux show-hooks -g | grep agent-status`; a hand-set `@agent_status` renders; a real turn sets it,
read back with `tmux display-message -p '#{@agent_status}'` rather than by looking at the status bar.

Agent hooks must run with a `PATH` that includes the profile the package landed in. A hook that
cannot find `agent-status` exits 0 in silence - correct, and invisible.

## nix profile

```sh
nix profile install github:gerbenoostra/tmux-agent-status
```

The snippet is then at
`~/.nix-profile/share/tmux/agent-status.conf`, which `source-file` can read directly.

## Prebuilt binary

Every tagged release publishes a tarball per platform with a `.sha256` beside it:

```sh
tag=v0.0.1
target=aarch64-apple-darwin      # or x86_64-apple-darwin, {x86_64,aarch64}-unknown-linux-gnu
base="https://github.com/gerbenoostra/tmux-agent-status/releases/download/$tag"
curl -fsSLO "$base/agent-status-$tag-$target.tar.gz"
curl -fsSLO "$base/agent-status-$tag-$target.tar.gz.sha256"
shasum -a 256 -c "agent-status-$tag-$target.tar.gz.sha256"
tar xzf "agent-status-$tag-$target.tar.gz"
install -Dm755 "agent-status-$tag-$target/agent-status" ~/.local/bin/agent-status
install -Dm644 "agent-status-$tag-$target/share/tmux/agent-status.conf" ~/.tmux/agent-status.conf
```

## cargo

```sh
cargo install --git https://github.com/gerbenoostra/tmux-agent-status
```

This installs the binary only; take the tmux snippet from the checkout or the release tarball.

## From source

```sh
git clone https://github.com/gerbenoostra/tmux-agent-status
cd tmux-agent-status
nix develop            # or bring your own Rust >= the rust-version in Cargo.toml
just check
just build
install -Dm755 target/release/agent-status ~/.local/bin/agent-status
install -Dm644 share/tmux/agent-status.conf ~/.tmux/agent-status.conf
```

## Working on it against your real config

**An edit live on the next hook fire.** Not `pip install -e`, because the artifact is a compiled
binary and a nix store path is immutable - but `PATH` gives the same effect:

```sh
just link      # ~/.local/bin/agent-status -> <checkout>/target/debug/agent-status
cargo build    # every rebuild is live on the next hook fire
just unlink    # back to the installed binary
```

This assumes `~/.local/bin` comes before your nix profile on `PATH`; check with `which -a
agent-status`, and `agent-status --version` prints the executable it actually resolved to.

Rejected as an alternative: `cargo install --path .`, which copies rather than symlinks and so needs
re-running on every edit, for the same shadowing risk.

**Does my packaging work?** Build the local flake; it installs nothing.

```sh
nix run . -- --version
just nix-build
```

**A local build inside the real config**, on the real `PATH`, under the real hooks - the last check
before tagging:

```sh
darwin-rebuild switch --flake "$HOME/.dotfiles#hostname" \
  --override-input tmux-agent-status "path:$HOME/src/tmux-agent-status"
```

Nothing is committed and the lock is untouched; dropping the flag reverts. Do not commit a `path:`
input instead: it is an absolute machine-local path, and its lock entry records a hash of the
directory that changes on every edit.
