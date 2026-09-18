# Install the CLI

Choose one installation method below. When `tmux-agent-status` is on the `PATH` inherited by your
agent hooks, complete setup with the same command for every method:

```sh
tmux-agent-status register
```

`register` contains the tmux snippet and agent hook configurations it needs. You do not need to
find or copy those files unless you are configuring the tool manually.

## Requirements

tmux 3.0 or newer, because the per-pane state is a pane option (`set-option -p`), which 3.0 added.

## One-line installer

```sh
curl -fsSL https://raw.githubusercontent.com/gerbenoostra/tmux-agent-status/main/install.sh | sh
```

Detects your platform, downloads the matching release tarball, verifies its checksum, and installs
the binary to `~/.local/bin` (override with `TMUX_AGENT_STATUS_INSTALL_DIR`). Pin a version with
`TMUX_AGENT_STATUS_VERSION=v0.0.1`. See `install.sh` in the repository root for the full set of
environment variables.

## Nix profile

```sh
nix profile install github:gerbenoostra/tmux-agent-status
```

## Nix flake and Home Manager

Add the input to `flake.nix`:

```nix
inputs.tmux-agent-status.url = "github:gerbenoostra/tmux-agent-status";
inputs.tmux-agent-status.inputs.nixpkgs.follows = "nixpkgs";
```

Then add the package to your Home Manager module:

```nix
home.packages = [ inputs.tmux-agent-status.packages.${pkgs.system}.tmux-agent-status ];
```

Run `tmux-agent-status register` after applying the configuration. If Home Manager owns your tmux
configuration as a read-only generated file, `register` reports the changes rather than editing
it.

To keep the tmux snippet declarative too, expose it at a stable path:

```nix
home.file.".tmux/tmux-agent-status.conf".source =
  "${inputs.tmux-agent-status.packages.${pkgs.system}.tmux-agent-status}/share/tmux/tmux-agent-status.conf";
```

Then source it from the tmux configuration managed by Home Manager:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

`register` can still configure writable agent files and report the window-format change for your
Home Manager configuration.


## Prebuilt binary

The one-line installer above wraps this; use these steps directly if you want to inspect each one.
Every tagged release publishes a tarball per platform with a `.sha256` checksum beside it:

```sh
tag=v0.0.1
target=aarch64-apple-darwin      # or x86_64-apple-darwin, {x86_64,aarch64}-unknown-linux-gnu
base="https://github.com/gerbenoostra/tmux-agent-status/releases/download/$tag"
curl -fsSLO "$base/tmux-agent-status-$tag-$target.tar.gz"
curl -fsSLO "$base/tmux-agent-status-$tag-$target.tar.gz.sha256"
shasum -a 256 -c "tmux-agent-status-$tag-$target.tar.gz.sha256"
tar xzf "tmux-agent-status-$tag-$target.tar.gz"
mkdir -p "$HOME/.local/bin"
cp "tmux-agent-status-$tag-$target/tmux-agent-status" "$HOME/.local/bin/tmux-agent-status"
chmod 755 "$HOME/.local/bin/tmux-agent-status"
```

Ensure `~/.local/bin` is on the `PATH` inherited by your agent hooks, then run
`tmux-agent-status register`.

## Cargo

```sh
cargo install --git https://github.com/gerbenoostra/tmux-agent-status
```

Cargo normally installs the command into `~/.cargo/bin`. Ensure that directory is on the `PATH`
inherited by your agent hooks, then run `tmux-agent-status register`.

## Build from source

```sh
git clone https://github.com/gerbenoostra/tmux-agent-status
cd tmux-agent-status
nix develop            # or bring your own Rust >= the rust-version in Cargo.toml
just check
just build
mkdir -p "$HOME/.local/bin"
cp target/release/tmux-agent-status "$HOME/.local/bin/tmux-agent-status"
chmod 755 "$HOME/.local/bin/tmux-agent-status"
```

Ensure `~/.local/bin` is on the `PATH` inherited by your agent hooks, then run
`tmux-agent-status register`.

## Manual file reference

You only need these paths when following the README's [manual configuration](../README.md#manual-configuration):

| Installation method | Tmux snippet | Agent configurations |
| --- | --- | --- |
| Nix flake | `${inputs.tmux-agent-status.packages.${pkgs.system}.tmux-agent-status}/share/tmux/tmux-agent-status.conf` | the same package path under `share/agents/<agent>/` |
| Nix profile | `~/.nix-profile/share/tmux/tmux-agent-status.conf` | `~/.nix-profile/share/agents/<agent>/` |
| Prebuilt tarball | `tmux-agent-status-$tag-$target/share/tmux/tmux-agent-status.conf` | `tmux-agent-status-$tag-$target/share/agents/<agent>/` |
| Cargo | use a checkout or extracted release tarball | use the same checkout or tarball |
| Source checkout | `share/tmux/tmux-agent-status.conf` | `share/agents/<agent>/` |

For a manual prebuilt or source installation, copy the snippet to any stable path readable by tmux,
such as `~/.tmux/tmux-agent-status.conf`. The executable may similarly live in any directory on the
agent hooks' `PATH`; `~/.local/bin` is an example, not a requirement.
