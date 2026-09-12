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

# Run the test suite with 100% line and region coverage.
# The bell path writes to /dev/tty, so the runner needs a controlling terminal;
# `script` creates one and is available on both Linux (util-linux) and macOS.
#
# `src/install/prompt.rs` is the one file excluded, and the exception is kept to
# one named file so it stays reviewable: it is the only module that knows there
# is a terminal, and exercising it means driving a pty, which would prove that
# `dialoguer` works rather than that we do. Everything worth asserting about a
# run's decisions lives in the modules it feeds, which hold the bar.
coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    args=(--summary-only --fail-under-lines 100 --fail-under-regions 100
          --ignore-filename-regex 'src/install/prompt\.rs$')
    if [[ "{{os()}}" == "macos" ]]; then
        script -q /dev/null cargo llvm-cov "${args[@]}"
    else
        script -q /dev/null -c "cargo llvm-cov ${args[*]}"
    fi

# What CI runs.
check: fmt-check lint test

# Validate the plugin and marketplace manifests (needs the `claude` CLI).
check-plugin:
    #!/usr/bin/env bash
    set -euo pipefail
    # Not a CI job, because it needs the `claude` CLI. The check that actually
    # rots - manifest against README - is a test, so it runs everywhere.
    if ! command -v claude >/dev/null 2>&1; then
        echo "claude CLI not found; skipping manifest validation." >&2
        echo "The README/manifest drift check runs in 'just test'." >&2
        exit 0
    fi
    claude plugin validate --strict .
    claude plugin validate --strict plugins/tmux-agent-status

# Build the release binary.
build:
    cargo build --release

# Shadow the installed binary with this checkout's release build.
link:
    #!/usr/bin/env bash
    set -euo pipefail
    bin_dir="${TMUX_AGENT_STATUS_BIN_DIR:-$HOME/.local/bin}"
    mkdir -p "$bin_dir"
    ln -sf "{{justfile_directory()}}/target/release/tmux-agent-status" "$bin_dir/tmux-agent-status"
    echo "$bin_dir/tmux-agent-status now shadows any installed tmux-agent-status." >&2
    echo "The shadow is invisible: 'tmux-agent-status --version' prints the resolved path." >&2
    echo "'just unlink' removes it." >&2

# Remove the dev shadow.
unlink:
    #!/usr/bin/env bash
    set -euo pipefail
    bin_dir="${TMUX_AGENT_STATUS_BIN_DIR:-$HOME/.local/bin}"
    rm -f "$bin_dir/tmux-agent-status"

# Build the nix package from this checkout.
nix-build:
    nix build .#tmux-agent-status

# A throwaway tmux server showing all four states, for looking at.
harness:
    #!/usr/bin/env bash
    set -euo pipefail
    cd "{{justfile_directory()}}"
    cargo build
    export PATH="$PWD/target/debug:$PATH"
    socket=tmux-agent-status-harness
    tmux -L "$socket" kill-server 2>/dev/null || true
    tmux -L "$socket" -f /dev/null new-session -d -s harness -x 200 -y 50 -n no-agent 'sleep 3000'
    format='#I:#{=/25/…:#{window_name}}#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
    tmux -L "$socket" set -g window-status-format "$format"
    tmux -L "$socket" set -g window-status-current-format "$format"
    tmux -L "$socket" set -g monitor-bell on
    tmux -L "$socket" set -g bell-action other
    export TMUX="$(tmux -L "$socket" display-message -p '#{socket_path}'),0,0"
    states=(working done error waiting)
    panes=()
    for state in "${states[@]}"; do
        panes+=("$(tmux -L "$socket" new-window -d -a -t 'harness:{end}' -n "$state" -P -F '#{pane_id}' 'sleep 3000')")
    done
    for i in "${!states[@]}"; do
        TMUX_PANE="${panes[$i]}" tmux-agent-status set "${states[$i]}"
    done
    # The hooks go on last: creating a window is itself a pane change, so they
    # would clear the states this harness exists to show before you saw them.
    tmux -L "$socket" source-file share/tmux/tmux-agent-status.conf
    echo "Attaching. Switch windows to watch the non-sticky states clear on focus." >&2
    echo "Kill it with: tmux -L $socket kill-server" >&2
    exec env -u TMUX tmux -L "$socket" attach -t harness
