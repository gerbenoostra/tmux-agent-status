# Installing tmux-agent-status

The `tmux-agent-status` cli can be installed using your package manager of choice.

After installing, you need to either run `tmux-agent-status install`, or manually finish with the configuration setup steps in the [README](../README.md)
to capture agent events, trigger tmux events, and present the glyph.
The documentation below shows where to find the files needed for manual tmux and agent events.
To add the glyph to the window name, follow the [README](../README.md)

## Requirements

tmux 3.0 or newer. The per-pane state is a pane option (`set-option -p`), which 3.0 added; every
format the tool writes is older than that. Nothing checks the version: on an older tmux the writes
fail and the hooks stay silent, the same as when tmux cannot be reached at all.


## Choose installation paths

The examples below use `~/.local/bin` for the executable and `~/.tmux` for the tmux snippet. These
are examples, not requirements:

- Linux and other POSIX systems commonly use `~/.local/bin` or `~/bin` for user-installed commands.
- macOS users can use the same directories. A Homebrew-managed command directory such as
  `$(brew --prefix)/bin` is another possibility, but manually installing into a package manager's
   edit prefix makes that file the user's responsibility.
- Cargo normally installs commands into `~/.cargo/bin`.
- Nix profiles provide their own command directories and should normally be used through the Nix
  installation routes below.
- `/usr/local/bin` is a common system-wide destination when all users need the command, but it
  generally requires administrator permissions.

Whichever directory you choose must be on the `PATH` inherited by the agent hooks. The tmux snippet
can live anywhere readable by tmux; its `source-file` line can just import from there. The prebuilt and
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

**2. Recommended configuration of tmux and agent hooks.**

Place the tmux-agent-status configuration at a deliberate path such that it can be imported from `.tmux.conf`:
```nix
home.file.".tmux/tmux-agent-status.conf".source =
  "${inputs.tmux-agent-status.packages.${pkgs.system}.tmux-agent-status}/share/tmux/tmux-agent-status.conf";
```

Then to install tmux hooks, use the following line in `.tmux.conf` to register the tmux hooks:
```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The `tmux-agent-status install` is able to locate the `tmux-agent-status.conf` file in the package, but would then edit your `.tmux.conf` with full paths.

Then install glyph and agents:
```sh
tmux-agent-status install
```

**Manual agent hooks.**

The agent configs can be found in the package at
`${inputs.tmux-agent-status.packages.${pkgs.system}.tmux-agent-status}/share/agents/<agent>/`.
Copy or merge the appropriate file as described on [your agent's page](agents/README.md).
## nix profile

```sh
nix profile install github:gerbenoostra/tmux-agent-status
```

**2. Recommended configuration of tmux and agent hooks.**
Complete configuration with:
```sh
tmux-agent-status install
```

**Manual configuration of tmux and agent hooks.**

If installing manually, use the following line in `.tmux.conf` to register the tmux hooks:
```tmux
source-file ~/.nix-profile/share/tmux/tmux-agent-status.conf
```

The agent configs can be found under `~/.nix-profile/share/agents/<agent>/`.
Copy or merge the appropriate file as described on [your agent's page](agents/README.md).

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

**2. Recommended configuration of tmux and agent hooks.**
Point the installer at the snippet you just copied:

```sh
tmux-agent-status install --snippet "$tmux_conf_dir/tmux-agent-status.conf"
```

**Manual configuration of tmux and agent hooks.**
Use the following line in `.tmux.conf` to register the tmux hooks:
```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The tarball also carries `share/agents/<agent>/`. Copy the file for your agent to the location [its page](agents/README.md) describes, for example:

```sh
mkdir -p ~/.codex
cp "tmux-agent-status-$tag-$target/share/agents/codex/hooks.json" ~/.codex/hooks.json
```


## cargo

```sh
cargo install --git https://github.com/gerbenoostra/tmux-agent-status
```

This installs the binary only. Take the tmux snippet and agent configs from a checkout or release
tarball. Copy `share/tmux/tmux-agent-status.conf` to `~/.tmux/tmux-agent-status.conf`, then use that
path:
```sh
tmux-agent-status install --snippet ~/.tmux/tmux-agent-status.conf
```

**Manual configuration of tmux and agent hooks**
Configure the tmux hooks manually:
```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

For the agent hooks, use `share/agents/<agent>/` from the same checkout or extracted tarball.
Copy or merge the appropriate file as described on [your agent's page](agents/README.md).

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

**Recommended configuration of tmux and agent hooks**
Point the installer at the source snippet:
```sh
tmux-agent-status install --snippet "$tmux_conf_dir/tmux-agent-status.conf"
```

**Manual configuration of tmux and agent hooks**
Source the copied snippet from `.tmux.conf`:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The configs required for the agent hooks are in the checkout under `share/agents/<agent>/`.
Copy or merge the appropriate file as described on [your agent's page](agents/README.md).
