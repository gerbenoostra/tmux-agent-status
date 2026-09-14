# Step 7: `src/install/probe.rs`

## Scope

The throwaway-server probe: unique socket, cleared `$TMUX`, cwd `$HOME`, timeout, kill on every exit
path, and the baseline-versus-candidate dump comparison. Used for the compiled-in default, the
baseline, and the semantic verify, so it exists once.

## Relevant design

The verify phase of the two tmux steps uses a throwaway tmux server to ask tmux what it actually
read from the config.

## Relevant decisions

From [decisions.md](../decisions.md):

### Verification by tmux probe

After the byte-level verify, start a throwaway tmux on the edited file and ask it what the format
actually is. Compare the full option dump against a baseline taken before the edit. The only allowed
differences are the intended format values and, for the hook step, the two hooks. This catches the
abandoned-config case, where tmux still answers `show-options -gwv` with the default. The write lands
first, then the probe runs; a failure rolls back through the backup.

The probe runs with `$TMUX` cleared, a unique socket name, cwd `$HOME`, and is killed on every exit
path. It is bounded by a timeout and can be skipped with `--no-tmux-probe`.

## Relevant findings

From [findings.md](../findings.md):

- tmux parses the whole config file before running any of it. An unknown command name or a valid
  command with a stray extra argument causes tmux to abandon the entire config file.
- tmux resolves a relative `source-file` path against the process's working directory, not the config
  file's directory.
- An abandoned config still answers `show-options -gwv` with the default, so a full dump diff is
  required to detect it.

## Implementation

- Provide a single probe helper that:
  1. Starts `tmux -L <unique> -f <entry-point config> new-session -d`.
  2. Clears `$TMUX` in the child environment.
  3. Sets cwd to `$HOME`.
  4. Dumps `show-options -g`, `show-options -gw`, and `show-hooks -g`/`-gw`.
  5. Kills the server on every exit path (success, failure, panic, interrupt).
  6. Enforces a timeout.
- Use the helper for:
  - reading the compiled-in default format ([Step 6](./06-install-format-rs.md));
  - taking a baseline dump before editing;
  - taking a candidate dump after editing and diffing against baseline.
- Detect a pre-broken config: if the baseline dump already shows tmux defaults while the config file
  plainly sets things, refuse to edit and report it.
- If the candidate diff contains anything other than the intended changes, fail the step and roll
  back through the backup (provided [Step 3](./03-install-write-rs.md)'s byte-level verify already proved the file is ours).

## Verification

### The tmux probe, extending `tests/tmux_server.rs`

23. **The probe catches an abandoned config.** Hand the writer a deliberately malformed splice (a
    stray argument after the value). Assert the dump comparison fails, the file is rolled back, the
    step reports failure, and the original config still produces its original options.
24. **The probe passes a good splice**, and the dump diff contains *only* the two format values.
25. **A pre-broken config is refused**, not edited: the baseline is defaults, the tool says so, and
    nothing is written.
26. **The probe cleans up**: no server on the probe socket afterwards, in the success, failure and
    interrupt cases.
27. **Relative `source-file`** in the config: the probe runs with cwd `$HOME`, and the case is
    reported rather than silently mis-resolved.
28. **No tmux on `PATH`**: the probe is skipped, the step still installs, the summary says the edit
    was not checked against tmux.
