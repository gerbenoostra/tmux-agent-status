# Step 9: `src/main.rs`

## Scope

The `install` subcommand and its flags in the pico-args dispatch, plus help text. `install` does
**not** route through `run_hook`: a hook exits 0 whatever happens, and an installer that hides a
failure is worthless. `main.rs`'s comment on `hook()` already anticipates this.

## Relevant design

`main.rs` dispatches to subcommands. The `install` subcommand is a new top-level entry alongside the
existing hook commands.

## Relevant spec

From [spec.md](../spec.md):

Command surface:

```
tmux-agent-status install [step flags] [answer flags] [target flags]

step flags
  --agents[=<name>[,<name>...]]   agent hooks; with names, only those agents
  --tmux-hook                     the source-file line for the shipped snippet
  --tmux-format                   the glyph term in both window status formats
  --no-agents --no-tmux-hook --no-tmux-format

answer flags
  -y, --yes        take the recommended answer to every question; implies non-interactive
  --dry-run        print the plan and exit 0; change nothing

target flags
  --tmux-config <path>   the config file to edit, instead of discovering one
  --snippet <path>       where the sourced snippet lives, or should be written
```

## Relevant decisions

From [decisions.md](../decisions.md):

- `install` does not route through `run_hook`: a hook exits 0 whatever happens, and an installer that
  hides a failure is worthless.
- Exit codes: 0 success/already installed; 1 at least one step failed; 2 usage error.
- Non-interactive without `-y`: exit 2.
- `--dry-run`: performs no write, no rename, no directory creation, no state-changing external
  command; runs read-only probes.

## Implementation

- Add the `install` subcommand to pico-args dispatch.
- Parse step flags, answer flags, target flags.
- Validate flag combinations: positive + negative step flag is a usage error; unknown `--agents=`
  name is a usage error.
- Pass parsed options to `install::run` ([Step 2](./02-install-mod-rs.md)).
- Return the exit code produced by the install run; do not flatten failures to 0.
- Update help text to describe the new subcommand and its flags.

## Verification

CLI:

- **`--dry-run` golden output**, and an assertion that the filesystem is unchanged afterwards.
- **Non-TTY without `-y` exits 2** with a message naming `-y`.
- **Exit codes for a mixed run**: one step already installed, one applied, one failed.
- **No tmux on `PATH`**: discovery, format default and reload all degrade as described, and the run
  still installs what it can.
