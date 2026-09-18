# Step 8: `src/install/prompt.rs`

## Scope

The only module that knows about a TTY. `-y` and `--dry-run` are answered here so no other module
branches on interactivity.

## Relevant design

The confirm phase asks one prompt per decision. `-y` answers all of them. `--dry-run` never reaches
a prompt because it prints the plan and exits.

## Relevant decisions

From [decisions.md](../decisions.md):

- **prompts**: `dialoguer` (`MultiSelect`, `Confirm`, `Editor`). The agent list is genuinely a
  checkbox and the format string genuinely needs an editor; `inquire` is the equivalent alternative
  if `dialoguer`'s `Editor` disappoints.
- **`HOME` resolution**: read `$HOME` / `$XDG_CONFIG_HOME` from the environment, never `getpwuid` -
  tests point a child process at a temp home; a tool that cannot be redirected cannot be tested.
- **non-interactive**: exit 2, never a silent default - a tool that edits configs unattended is a
  tool nobody asked to run.
- **the TTY module**: `src/install/prompt.rs` is the one file `just coverage` skips, by filename. A
  prompt needs a terminal to exercise, and the alternative is a pty harness that proves `dialoguer`
  works rather than that we do; keeping the exception to one named file is what makes it reviewable.

## Implementation

- Wrap `dialoguer` (or `inquire`) for:
  - agent selection: `MultiSelect` over preselected agents;
  - confirmations: `Confirm`;
  - editing the format string: `Editor` for long values, inline prompt for short values.
- Detect non-TTY and no `-y`: exit 2 before any prompt, with a message naming `-y`.
- Implement `-y`: answer every question with the recommended choice and never block.
- Implement `--dry-run`: return early / short-circuit so callers never prompt.
- Keep all environment resolution (`$HOME`, `$XDG_CONFIG_HOME`) in this module or its callers, never
  `getpwuid`.

## Verification

CLI:

- **Non-TTY without `-y` exits 2** with a message naming `-y`.
- **Exit codes for a mixed run**: one step already installed, one applied, one failed.
- **`--dry-run` golden output**, and an assertion that the filesystem is unchanged afterwards -
  including that the probe server's socket is gone and no lock, temp or backup file was left.

Coverage:

- Confirm `just coverage` skips only this file by filename.
