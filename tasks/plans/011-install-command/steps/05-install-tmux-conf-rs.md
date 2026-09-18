# Step 5: `src/install/tmux_conf.rs`

## Scope

Config discovery, snippet discovery, and the tmux `source-file` block.

## Relevant decisions

From [decisions.md](../decisions.md):

### tmux config discovery

`tmux display-message -p '#{config_files}'` is a source of candidates, not a decision, because it
lists non-existent files and is constrained to `-f` when that was used. The chosen resolution order
is:

1. `--tmux-config <path>`, which overrides everything below
2. `$XDG_CONFIG_HOME/tmux/tmux.conf` if it exists
3. `~/.config/tmux/tmux.conf` if it exists
4. `~/.tmux.conf` if it exists
5. none exist: offer to create `~/.config/tmux/tmux.conf`

`/etc/tmux.conf` is never chosen, even when it is the only one that exists and even under `-y`. It
needs root and installs the tool for every user of the machine, which is not what anyone typing this
command meant. It is reported with the suggestion to pass `--tmux-config` if that really was the
intent.

With no tmux on `PATH` or no running server, every tmux invocation is optional and its failure is not
an error. Discovery falls to the file-existence order, the format default falls to its hard-coded
value, and the reload offer is not made. An earlier revision also had the summary say plainly which
checks could not be run against a live tmux; dropped, see [decisions.md](../decisions.md).

### Idempotency for the source-file line

Any `source` or `source-file` command whose last argument has the basename
`tmux-agent-status.conf`, at any path, means installed. The location is the user's business, and a
second source line is a second set of hooks.

## Relevant findings

From [findings.md](../findings.md):

- `tmux display-message -p '#{config_files}'` returns candidate paths, including files that do not
  exist; when the server was started with `-f` it names only that file.

## Implementation

### Finding the snippet

In order, first hit wins:

1. `--snippet <path>`
2. relative to the running executable, resolving symlinks first: `../share/tmux/...` covers the nix
   profile and the release tarball, and `../../share/tmux/...` covers a binary run straight out of
   `target/release` in a checkout. A dev-loop `~/.local/bin` shadow pointing into a build directory
   finds nothing at `../share`, and must fall through rather than fail.
3. the usual prefixes: `$PREFIX/share`, `~/.nix-profile/share`, `/usr/local/share`,
   `/opt/homebrew/share`
4. nothing found - offer to write the embedded copy to `~/.config/tmux/tmux-agent-status.conf`
   (or `~/.tmux/` when `~/.tmux.conf` is the config in use), which is what `cargo install` needs.

### Finding the config

- Use `tmux display-message -p '#{config_files}'` when tmux is available.
- Fall back to the file-existence order above.
- Never auto-select `/etc/tmux.conf`.

### The edit

Append at end of file, inside markers:

```tmux
# >>> tmux-agent-status >>>
source-file ~/.config/tmux/tmux-agent-status.conf
# <<< tmux-agent-status <<<
```

The markers are what makes the future `uninstall` mechanical. Position does not matter here: the
snippet sets hooks only, and a hook set late is a hook set.

## Verification

CLI:

- **No tmux on `PATH`**: discovery, format default and reload all degrade as described, and the run
  still installs what it can.

Real tmux, extending `tests/tmux_server.rs`:

- Write a temp config with a known format, `install --tmux-format --tmux-hook -y
  --tmux-config <path>`, then start `tmux -L <name> -f <path>` and assert both hooks are
  registered.
- The same against a config with **no** format line, proving the default probe produces a working
  pair of lines.

By hand, on a real machine:

- A home-manager machine, the `mkOutOfStoreSymlink` `~/.tmux.conf` chain: the symlink survives and the
  edit lands in the dotfiles checkout.
