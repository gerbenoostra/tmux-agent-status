#!/usr/bin/env bash
# probe-lifecycle.sh <host>
#
# Start a disposable probe of one agent host's lifecycle events: a private
# tmux server (`tmux -L`), a scratch workspace outside this repository, and
# the host launched in a pane with probe hooks on every lifecycle event it
# documents. Each hook appends {event, ts_enter, ts_exit, stdin} as one JSONL
# record to the probe log. The user's real hook config, tmux server and this
# repository are never touched.
#
# Host support lives in probe/hosts/<host>.sh, which is sourced here and must
# define:
#   probe_binary            - the binary the probe needs on PATH
#   probe_install_hooks DIR - write the host's hook configuration under DIR
#   probe_launch_command DIR - print the shell command to run in the pane
#   probe_scenarios         - print the scenario prompts to drive
#
# The probe server and workspace stay up after this script exits; drive the
# scenarios by hand or with `tmux send-keys`, then kill the server and remove
# the workspace with the commands printed at the end.

set -euo pipefail

PROBE_DIR=$(cd "$(dirname "$0")" && pwd)

host=${1:-}
if [ -z "$host" ] || [ ! -f "$PROBE_DIR/hosts/$host.sh" ]; then
    [ -z "$host" ] || echo "probe-lifecycle: no probe installer for host '$host'" >&2
    echo "usage: probe-lifecycle <host>" >&2
    echo "hosts with a probe installer:" >&2
    find "$PROBE_DIR/hosts" -name '*.sh' -maxdepth 1 -exec basename {} .sh \; \
        | sed 's/^/  /' >&2
    exit 1
fi
host_file="$PROBE_DIR/hosts/$host.sh"

# shellcheck source=/dev/null
. "$host_file"

binary=$(probe_binary)
if ! command -v "$binary" >/dev/null 2>&1; then
    echo "probe-lifecycle: host '$host' needs '$binary' on PATH; install it and retry" >&2
    exit 1
fi
for tool in tmux jq; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "probe-lifecycle: the harness itself needs '$tool' on PATH" >&2
        exit 1
    }
done

# macOS sets TMPDIR with a trailing slash, Linux usually leaves it unset.
tmp=${TMPDIR:-/tmp}
scratch=$(mktemp -d "${tmp%/}/tas-probe-$host.XXXXXX")
mkdir -p "$scratch/workspace"
probe_install_hooks "$scratch"

socket="tas-probe-$host-$$"
tmux -L "$socket" -f /dev/null new-session -d -s probe -x 220 -y 50 -n agent \
    "$(probe_launch_command "$scratch")"
pane=$(tmux -L "$socket" list-panes -t probe -F '#{pane_id}' | head -1)

cat <<EOF
probe: $host
  socket:    $socket
  pane:      $pane
  workspace: $scratch/workspace
  hook log:  $scratch/hooks.jsonl

drive it:
  tmux -L $socket attach -t probe                  # watch it
  tmux -L $socket send-keys -t $pane -l '<prompt>' # type, then:
  tmux -L $socket send-keys -t $pane Enter
  tmux -L $socket capture-pane -t $pane -p         # read the screen

when done:
  tmux -L $socket kill-server
  rm -rf "$scratch"
EOF
probe_scenarios
