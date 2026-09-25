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

# Lint the shell installer.
lint-sh:
    shellcheck -s sh install.sh

# Run the test suite.
test:
    cargo test

# Run the test suite and hold every region of `src/` to covered.
# The bell path writes to /dev/tty, so the runner needs a controlling terminal;
# `script` creates one and is available on both Linux (util-linux) and macOS.
#
# The bar is read from the exported segments rather than from
# `--fail-under-regions`, because the two do not agree. `src/` is compiled
# twice - once with `cfg(test)` for the lib's own test binary, once as the rlib
# the integration tests link - and `llvm-cov report` leaves a handful of spans
# unmerged between the two, so its summary counts regions as missed that
# `llvm-cov show` renders as covered. The segments are the view `show` renders,
# and they answer one question consistently: is there a region nothing reached?
# Full region coverage implies full line coverage, so that one bar is enough.
#
# A line that cannot be reached says so for itself, with a trailing
# `// coverage: off` and the reason it is unreachable. The marker is matched
# against the whole line, so it exempts every region on that line and not only
# the one that is uncovered today: keep it to lines that carry nothing else, or
# say in the comment what else it covers.
#
# `src/register/prompt.rs` is the one file excluded, and the exception is kept to
# one named file so it stays reviewable: it is the only module that knows there
# is a terminal, and exercising it means driving a pty, which would prove that
# `dialoguer` works rather than that we do. Everything worth asserting about a
# run's decisions lives in the modules it feeds, which hold the bar.
#
# An incremental instrumented build can reuse a codegen unit's old line info and
# report a region against lines the sources no longer have there - including a
# phantom miss beside a `// coverage: off` marker. `cargo llvm-cov clean` and
# rerun before believing one.
coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v jq >/dev/null || { echo "the coverage gate needs jq." >&2; exit 1; }
    ignore='src/register/prompt\.rs$'
    if [[ "{{os()}}" == "macos" ]]; then
        script -q /dev/null cargo llvm-cov --no-report
    else
        script -q /dev/null -c "cargo llvm-cov --no-report"
    fi
    cargo llvm-cov report --summary-only --ignore-filename-regex "$ignore"
    echo
    echo "The misses above are counted per compilation, not per region; the bar"
    echo "below is the merged region view. See the comment on this recipe."
    report="$(mktemp)"
    regions="$(mktemp)"
    trap 'rm -f "$report" "$regions"' EXIT
    cargo llvm-cov report --json --ignore-filename-regex "$ignore" --output-path "$report"
    # A segment carries [line, column, count, has-count, region-entry, gap].
    # One with a count of zero that is not a gap is a region nothing reached.
    # Written to a file rather than piped into the loop: a gate that cannot
    # fail is worse than no gate, and `set -e` does not reach into a process
    # substitution, so a `jq` that dies there would read as "nothing to report".
    jq -r '.data[].files[] | .filename as $file
           | (.segments // [])[]
           | select(.[3] and .[2] == 0 and (.[5] | not))
           | "\($file):\(.[0])"' "$report" | sort -u > "$regions"
    # The same argument: a report naming no file at all is a broken run, not a
    # clean one.
    files="$(jq -r '[.data[].files[].filename] | length' "$report")"
    if (( files == 0 )); then
        echo "the coverage report names no files; nothing was measured." >&2
        exit 1
    fi
    uncovered=0
    while IFS= read -r region; do
        [[ -n "$region" ]] || continue
        file="${region%:*}"
        line="${region##*:}"
        if [[ "$(sed -n "${line}p" "$file")" == *'// coverage: off'* ]]; then
            continue
        fi
        echo "uncovered region: $file:$line" >&2
        uncovered=$((uncovered + 1))
    done < "$regions"
    if (( uncovered )); then
        echo "$uncovered uncovered region(s) in $files file(s); the bar is all of them." >&2
        exit 1
    fi
    echo "Every region of src/ was reached, across $files files."

# What CI runs.
check: fmt-check lint lint-sh test

# Validate the plugin and marketplace manifests (needs the `claude` CLI).
check-plugin:
    #!/usr/bin/env bash
    set -euo pipefail
    # Not a CI job, because it needs the `claude` CLI. The check that actually
    # rots - manifest against the agent doc - is a test, so it runs everywhere.
    if ! command -v claude >/dev/null 2>&1; then
        echo "claude CLI not found; skipping manifest validation." >&2
        echo "The agent-doc/manifest drift check runs in 'just test'." >&2
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
