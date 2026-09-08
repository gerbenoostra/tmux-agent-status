# The recipes CI runs are the recipes a human types; that is the whole point of this file.

# List the recipes.
default:
    @just --list

# Format the sources.
fmt:
    cargo fmt

# Fail if the sources are not formatted.
fmt-check:
    cargo fmt --check

# Lint, warnings are errors.
lint:
    cargo clippy --all-targets -- -D warnings

# Run the test suite.
test:
    cargo test

# What CI runs.
check: fmt-check lint test

# Build the release binary.
build:
    cargo build --release

# Shadow the installed binary with this checkout's debug build (dev loop).
link:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p ~/.local/bin
    ln -sf "{{justfile_directory()}}/target/debug/agent-status" ~/.local/bin/agent-status
    echo "~/.local/bin/agent-status now shadows any installed agent-status." >&2
    echo "The shadow is invisible: 'agent-status --version' prints the resolved path." >&2
    echo "'just unlink' removes it." >&2

# Remove the dev shadow.
unlink:
    rm -f ~/.local/bin/agent-status

# Build the nix package from this checkout.
nix-build:
    nix build .#agent-status

# A throwaway tmux server showing all four states, for looking at.
harness:
    #!/usr/bin/env bash
    set -euo pipefail
    cd "{{justfile_directory()}}"
    cargo build
    export PATH="$PWD/target/debug:$PATH"
    socket=agent-status-harness
    tmux -L "$socket" kill-server 2>/dev/null || true
    tmux -L "$socket" -f /dev/null new-session -d -s harness -x 200 -y 50 -n no-agent 'sleep 3000'
    tmux -L "$socket" source-file share/tmux/agent-status.conf
    format='#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
    tmux -L "$socket" set -g window-status-format "$format"
    tmux -L "$socket" set -g window-status-current-format "$format"
    tmux -L "$socket" set -g monitor-bell on
    tmux -L "$socket" set -g bell-action other
    export TMUX="$(tmux -L "$socket" display-message -p '#{socket_path}'),0,0"
    for state in working done error waiting; do
        pane=$(tmux -L "$socket" new-window -d -a -t 'harness:{end}' -n "$state" -P -F '#{pane_id}' \
            'bash -c "exec -a claude sleep 3000"')
        TMUX_PANE="$pane" agent-status set "$state"
    done
    echo "Attaching. Switch windows to watch the non-sticky states clear on focus." >&2
    echo "Kill it with: tmux -L $socket kill-server" >&2
    exec env -u TMUX tmux -L "$socket" attach -t harness
