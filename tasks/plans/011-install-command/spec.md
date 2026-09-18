# 011 - spec

011 - install: write the hooks and configs from the tool

Covers *how the last three of the README's four setup steps stop being a manual paste*. What the
states mean, what the two tmux options are and which events map to which state stay 001, 005 and
each `docs/agents/<agent>.md` page; this plan never restates a behavioural decision and never
invents a mapping. It adds one subcommand that writes what those documents already describe.

## Goal

```sh
tmux-agent-status install
```

asks a handful of questions, and when it returns the tool is working. Done when all of these hold:

1. On a machine with agents installed and a tmux config, `install` followed by a tmux reload makes a
   real turn end put a real glyph on a real window, with no file edited by hand.
2. Running it a second time writes nothing at all: every step reports "already installed" and no new
   backup appears.
3. Every file it edits is restored byte for byte if the write does not land as intended, and the
   backup that proves it is named in the output.
4. A file that is a symlink is still a symlink afterwards, and the edit landed on its target.
5. `--dry-run` prints exactly what the run would do, and touches nothing.
6. `-y` runs the whole thing with no prompt and no TTY.

## Scope boundary

| In | Out |
| --- | --- |
| an `install` subcommand with three steps | an `uninstall` subcommand (designed for, shipped later - see "Uninstall, designed not shipped") |
| user-scope config for every agent in `docs/agents/README.md` | project-scope config, except Devin, which has no user-scope drop-in |
| the tmux `source-file` line and the two format strings | any other tmux option; anything in `share/tmux/tmux-agent-status.conf` beyond sourcing it |
| following `source-file` when hunting for the format line | a general tmux config parser or evaluator |
| installing the binary itself | `docs/install.md`'s job, and unchanged |
| reloading tmux, offered and confirmed | restarting an agent so its hooks load |
| backups, locks, verify-after-write, restore | pruning old backups |

## Command surface

```
tmux-agent-status install [step flags] [answer flags] [target flags]

step flags
  --agents[=<name>[,<name>...]]   agent hooks; with names, only those agents
  --tmux-hook                     the source-file line for the shipped snippet
  --tmux-format                   the glyph term in both window status formats
  --no-agents --no-tmux-hook --no-tmux-format

answer flags
  -y, --yes        take the recommended answer to every question; implies non-interactive
  --dry-run        print the plan and exit 0; change nothing (see "What --dry-run may run")

target flags
  --tmux-config <path>   the config file to edit, instead of discovering one
  --snippet <path>       where the sourced snippet lives, or should be written
```

### Step selection algebra

| Flags given | Steps run |
| --- | --- |
| none | all three |
| one or more positive | exactly those |
| only negative | all three minus those |
| a positive and a negative | usage error, exit 2 |

`--agents=codex,cursor` selects the agents step *and* narrows it; a bare `--agents` selects the step
and leaves agent selection to detection. This is what makes `--agents` alone mean "only the agents",
as asked, without a second "only" flag.

A name that is not in the agent table is a **usage error, exit 2**, listing the valid names - a
typo'd `--agents=cursur` must never be read as "install nothing, successfully". A name that is valid
but *undetected* installs anyway: naming an agent explicitly is a stronger signal than the absence
of its config directory, and installing hooks before the agent is a legitimate order to do things
in.

### What `--dry-run` may run

`--dry-run` must produce a real plan without changing anything. It performs no write, no rename,
no directory creation, and no state-changing external command. It may run a small, fixed set of
read-only probes to do that. The read-only commands are exactly these, and the list is exhaustive
by design:

| Command | Why | Side effect |
| --- | --- | --- |
| `tmux display-message -p '#{config_files}'` | which config tmux would load | none; needs a running server |
| `tmux show-options -gwv window-status-*` | the effective format values | none |
| `tmux -L <probe> -f /dev/null start-server` + `show-options` + `kill-server` | this tmux's compiled-in default format | a socket in `$TMUX_TMPDIR`, created and killed inside the call |
| `claude plugin list --json`, `claude plugin marketplace list --json` | is the plugin already installed | none |

The probe server is the only one that creates anything, and it is the one place where suppressing it
would force the plan to print a hard-coded default that may not be this tmux's. It runs under
`--dry-run` and the output says it did. Everything that installs - `claude plugin install`,
`marketplace add`, every file write - is printed, never executed.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | every requested step is installed, or was already |
| 1 | at least one step failed; every file it touched is back the way it was |
| 2 | usage error, including "a question needs answering and there is no TTY and no `-y`" |

Exit 2 rather than a guess for the non-interactive case: a hook that silently picks defaults on a CI
box is how a config gets edited by something nobody asked.
