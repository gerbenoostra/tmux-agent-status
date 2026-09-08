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
