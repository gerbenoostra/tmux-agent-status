# Installing tmux-agent-status

Below we list the different installation options.
After installing you need to finish with the configuration setup steps in the [README](../README.md):
the binary on its own does nothing until tmux and your agent know about it.

## Choose installation paths

The examples below use `~/.local/bin` for the executable and `~/.tmux` for the tmux snippet. These
are examples, not requirements:

- Linux and other POSIX systems commonly use `~/.local/bin` or `~/bin` for user-installed commands.
- macOS users can use the same directories. A Homebrew-managed command directory such as
  `$(brew --prefix)/bin` is another possibility, but manually installing into a package manager's
  prefix makes that file the user's responsibility.
- Cargo normally installs commands into `~/.cargo/bin`.
- Nix profiles provide their own command directories and should normally be used through the Nix
  installation routes below.
- `/usr/local/bin` is a common system-wide destination when all users need the command, but it
  generally requires administrator permissions.

Whichever directory you choose must be on the `PATH` inherited by the agent hooks. The tmux snippet
can live anywhere readable by tmux; its `source-file` line must use the same path. The prebuilt and
source examples use variables so either location can be changed:

```sh
bin_dir="$HOME/.local/bin"
tmux_conf_dir="$HOME/.tmux"
```

## Nix flake input (home-manager, nix)

**1. The input and the package.**

Include the tool as follows in your `flake.nix`:
```nix
inputs.tmux-agent-status.url = "github:gerbenoostra/tmux-agent-status";
inputs.tmux-agent-status.inputs.nixpkgs.follows = "nixpkgs";
```

and add it to the home-manager module:

```nix
home.packages = [ inputs.tmux-agent-status.packages.${pkgs.system}.tmux-agent-status ];
```

**2. The tmux snippet, at a stable path.**

Place the tmux-agent-status configuration at a deliberate path such that it can be imported from `.tmux.conf`:
```nix
home.file.".tmux/tmux-agent-status.conf".source =
  "${inputs.tmux-agent-status.packages.${pkgs.system}.tmux-agent-status}/share/tmux/tmux-agent-status.conf";
```

Then add the following line to `.tmux.conf`:
```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

**Steps 3 and 4.**
Define a format term and configure agent hooks manually, as described in the [README](../README.md).

Verify the tool and hooks are installed:

```sh
tmux-agent-status --version
tmux show-hooks -g | grep tmux-agent-status    # session-window-changed
tmux show-hooks -gw | grep tmux-agent-status   # window-pane-changed
```

Note that if your agent's hook cannot find `tmux-agent-status` on `PATH`, it will silently fail.

## nix profile

```sh
nix profile install github:gerbenoostra/tmux-agent-status
```

The snippet is then at
`~/.nix-profile/share/tmux/tmux-agent-status.conf`, which `source-file` can read directly.

## Prebuilt binary

Every tagged release publishes a tarball per platform with a `.sha256` checksum file beside it:

```sh
tag=v0.0.1
target=aarch64-apple-darwin      # or x86_64-apple-darwin, {x86_64,aarch64}-unknown-linux-gnu
base="https://github.com/gerbenoostra/tmux-agent-status/releases/download/$tag"
curl -fsSLO "$base/tmux-agent-status-$tag-$target.tar.gz"
curl -fsSLO "$base/tmux-agent-status-$tag-$target.tar.gz.sha256"
shasum -a 256 -c "tmux-agent-status-$tag-$target.tar.gz.sha256"
tar xzf "tmux-agent-status-$tag-$target.tar.gz"
bin_dir="$HOME/.local/bin"
tmux_conf_dir="$HOME/.tmux"
mkdir -p "$bin_dir" "$tmux_conf_dir"
cp "tmux-agent-status-$tag-$target/tmux-agent-status" "$bin_dir/tmux-agent-status"
chmod 755 "$bin_dir/tmux-agent-status"
cp "tmux-agent-status-$tag-$target/share/tmux/tmux-agent-status.conf" "$tmux_conf_dir/tmux-agent-status.conf"
chmod 644 "$tmux_conf_dir/tmux-agent-status.conf"
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
bin_dir="$HOME/.local/bin"
tmux_conf_dir="$HOME/.tmux"
mkdir -p "$bin_dir" "$tmux_conf_dir"
cp target/release/tmux-agent-status "$bin_dir/tmux-agent-status"
chmod 755 "$bin_dir/tmux-agent-status"
cp share/tmux/tmux-agent-status.conf "$tmux_conf_dir/tmux-agent-status.conf"
chmod 644 "$tmux_conf_dir/tmux-agent-status.conf"
```
