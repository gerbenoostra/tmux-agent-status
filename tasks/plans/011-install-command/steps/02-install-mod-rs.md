# Step 2: `src/install/mod.rs`

## Scope

The orchestration module: the five phases every step runs through, the step-selection algebra, the
`Change` type, and the final summary. Nothing outside the `apply` phase writes.

## Relevant design

From [design.md](../design.md), "The shape of a run":

Every step is the same five phases, and **nothing writes until every phase-3 answer is in**.

```
detect  ->  plan  ->  confirm  ->  apply  ->  verify
```

- **detect** - read-only. What is installed, what is already configured, where the files are.
- **plan** - build a `Change` per target: path, current bytes, intended bytes, why. Pure functions
  over strings; this is where the merges and splices happen, and where the tests live.
- **confirm** - one prompt per decision, in the order below. `-y` answers all of them.
- **apply** - the safe write, per target, in a fixed order: agents, then tmux hook, then tmux format.
- **verify** - re-read, re-parse, compare. A failure here restores and fails the step.

A step that fails does not stop the run: the remaining steps still apply, and the summary at the end
says which of the three landed. No step reads another's output, so a failure cannot corrupt what
follows. The glyph does need all three to appear, but a run that does what it can and says exactly
what it did not beats one that abandons work it was able to finish.

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

Step selection algebra:

| Flags given | Steps run |
| --- | --- |
| none | all three |
| one or more positive | exactly those |
| only negative | all three minus those |
| a positive and a negative | usage error, exit 2 |

`--agents=codex,cursor` selects the agents step *and* narrows it; a bare `--agents` selects the step
and leaves agent selection to detection.

A name that is not in the agent table is a **usage error, exit 2**, listing the valid names. A name
that is valid but *undetected* installs anyway.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | every requested step is installed, or was already |
| 1 | at least one step failed; every file it touched is back the way it was |
| 2 | usage error, including "a question needs answering and there is no TTY and no `-y`" |

## Implementation

- Define a `Change` struct: `path`, `current_bytes`, `intended_bytes`, `why`.
- Define the three steps (agents, tmux hook, tmux format) and the step-selection logic.
- Implement `detect -> plan -> confirm -> apply -> verify` for each selected step.
- The `apply` phase is the only writer; it calls `install::write::safely` ([Step 3](./03-install-write-rs.md)).
- Order of apply: agents, then tmux hook, then tmux format.
- A step failure is recorded but does not abort subsequent steps.
- Build the summary: per-step status, backup paths, managed/unmanaged destination grouping, what
could not be checked against a live tmux.
- Non-interactive without `-y` exits 2 before any prompt.
- `--dry-run` builds the plan and prints it; it performs no write, no rename, no directory creation,
and no state-changing external command (see [decisions.md](../decisions.md)).

## Verification

- **Step algebra**: every flag combination, including the positive-and-negative usage error and an
  unknown `--agents=` name (exit 2, valid names listed).
- **Exit codes** for a mixed run: one step already installed, one applied, one failed.
- **Non-TTY without `-y` exits 2** with a message naming `-y`.
