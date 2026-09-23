# Register the hooks

With `tmux-agent-status` on your `PATH` ([install it](install.md) first), one command configures tmux and your agents:

```sh
tmux-agent-status register
```

`register` detects your agents, previews and confirms every change, backs up every file it edits,
and checks the final tmux configuration against a throwaway server; which loads your config, so whatever
it runs (`run-shell`, `if-shell`, a plugin manager) runs there too. If it cannot edit a generated or
read-only file safely, it prints the change for you to apply instead.

Run `tmux-agent-status register --dry-run` to inspect the plan without changing anything.

Run `tmux-agent-status register -y` to accept all changes.

Use `tmux-agent-status --help` for step selection, non-interactive operation, and target
overrides.

## Manual configuration

`register` is the recommended route.
If you prefer to manage every file yourself, configure the same three parts manually.

### Tmux hooks

Copy [`share/tmux/tmux-agent-status.conf`](../share/tmux/tmux-agent-status.conf) somewhere readable
by tmux, then source that path from your tmux configuration. For example:

```tmux
source-file ~/.tmux/tmux-agent-status.conf
```

The package-specific locations of the shipped file are listed in the [manual file reference](install.md#manual-file-reference).

### Tmux window format

Add this term to **both** `window-status-format` and `window-status-current-format`:

```tmux
#{?@agent_status, #{@agent_status},}
```

Place it after the name segment, outside any truncation, and before the window flags. For example:

```tmux
set -g window-status-format '#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The name segment stays whatever you already had.

### Agent hooks

Follow the [agent setup guide](agents/README.md) and the linked page for your agent.
Depending on the agent, manual delivery means installing a plugin, copying a drop-in file, or merging settings.

## Optional bell settings

When a turn ends (`waiting`, `error`, or `done`, but not `working`), the tool writes a bell (`\a`)
to its tmux pane. tmux's defaults already do the right thing, so these settings matter only if you
or your tmux framework changed them. Check with `tmux show-options -g bell-action`.

| Setting | What it decides | Suggested |
| --- | --- | --- |
| `monitor-bell` | Whether tmux notices the bell at all. `off` means no highlight, and nothing reaches your terminal either. | `on` (tmux default) |
| `bell-action` | Which windows may pass a bell to your terminal. `other` ignores the window currently selected in tmux; `any` also passes bells from that window. | `any` (tmux default) |
| `visual-bell` | Whether the bell stays a bell. `on` replaces it with a tmux message, so your terminal never sees it. | `off` (tmux default) |
| `window-status-bell-style` | How a window that rang is painted until you visit it. | to taste |

For example:

```tmux
setw -g monitor-bell on
set -g bell-action any
setw -g window-status-bell-style 'fg=magenta,bold,nodim'
```

`bell-action any` is useful when the tmux window running the agent is current inside a terminal tab
that is itself hidden. The trade-off is that other bells from that window also reach your terminal.

What the terminal does with the bell is up to the terminal. Ghostty, for example, prefixes the tab
title with 🔔 and asks for your attention while it is unfocused, and stays silent unless you enable
a sound in `bell-features`. The bell tells you *which terminal tab*; the glyph tells you
*which tmux window*.

## The bell, and colour

For the end states (`waiting`, `error` and `done`, thus not `working`) a terminal bell (`\a`) is printed.
With `monitor-bell on`, tmux gives you the window highlight, in whatever way you configure it, and
`bell-action` decides whether the bell also reaches your terminal: see the
[optional bell settings](#optional-bell-settings).
To not interfere with your own highlight format, this tool deliberately does not colour or name windows.

If you want `error` to stand out further, paste this in front of the name segment, in both formats:

```tmux
#{?#{==:#{@agent_status},❗},#[fg=colour160#,bold#,nodim],}
```

For example:

```tmux
set -g window-status-format '#I:#{?#{==:#{@agent_status},❗},#[fg=colour160#,bold#,nodim],}#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
```

The style runs to the end of the entry, so the name and flags turn red, and on that window it replaces
the `window-status-bell-style` colour. `#,` escapes a comma inside `#{...}`.
It is also documented in the [shipped snippet](../share/tmux/tmux-agent-status.conf).

## Validation

Now you should be ready to go.
If you want to verify the parts, that can be done as follows.

`tmux-agent-status register --dry-run` reports the configuration state without changing anything, which is the quickest check.

To verify the tool is installed and on your path:
```sh
tmux-agent-status --version
```

The tmux hooks and the `focus-events` option can be confirmed with:
```sh
tmux show-hooks -g | grep tmux-agent-status    # session-window-changed
tmux show-hooks -gw | grep tmux-agent-status   # pane-focus-in, window-pane-changed
tmux show-options -g focus-events              # on
```

`focus-events on` is what lets `pane-focus-in` fire when your terminal regains focus after showing
another tab, desktop or monitor - without it, switching windows or panes inside tmux still clears
the pane you land on, but returning to the terminal from elsewhere does not. To decline it, add
`set -g focus-events off` to your own tmux config, after the line that sources the shipped snippet;
`register` still owns and can rewrite the snippet itself, so an edit inside it may be undone or
refused on the next run.

If you are upgrading, re-run `tmux-agent-status register`. It reads the snippet your config already
sources, compares it to the one this version ships, and offers to replace it when the two differ -
so an older copy still setting the previous hooks is found even though the `source-file` line is
already in place. Your own `window-status-format` edits are untouched. A snippet that belongs to a
package manager is read-only: one that already matches this version is simply reported as
registered, and one that does not names both ways out - upgrade the package, or point the
`source-file` line at a path of your own and run `register` again, which writes this version's
snippet wherever that line points. A tmux server already running keeps the hook values it read at `source-file` time, so the
new hooks and options only take effect on the next `source-file` or server restart.

The tmux window status glyph rendering can be verified by setting a glyph by hand in tmux: `tmux set-option -w @agent_status ✅`, then `tmux set-option -w -u @agent_status`.
