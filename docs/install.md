# Installing agent-status

Five routes. After installing you need to finish with the four configuration setup steps in the [README](../README.md):
the binary on its own does nothing until tmux and your agent know about it.

## Nix flake input (home-manager, nix)

**1. The input and the package.**

Include the tool as follows in your `flake.nix`:
```nix
inputs.tmux-agent-status.url = "github:gerbenoostra/tmux-agent-status";
inputs.tmux-agent-status.inputs.nixpkgs.follows = "nixpkgs";
```

and add it to the home-manager module:

```nix
home.packages = [ inputs.tmux-agent-status.packages.${pkgs.system}.agent-status ];
```

**2. The tmux snippet, at a stable path.**

Place the agent-status configuration at a deliberate path such that it can be imported from `.tmux.conf`:
```nix
home.file.".tmux/agent-status.conf".source =
  "${inputs.tmux-agent-status.packages.${pkgs.system}.agent-status}/share/tmux/agent-status.conf";
```

Then add the following line to `.tmux.conf`:
```tmux
source-file ~/.tmux/agent-status.conf
```

**3 and 4.**
Define a format term and configure agent hooks manually, as described in the [README](../README.md)

Verify the tool is installed with: `agent-status --version`;
Verify the tmux hooks are installed with: `tmux show-hooks -g | grep agent-status`;

Note that if your agent's hook cannot find `agent-status` on `PATH`, it will silently fail.

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
