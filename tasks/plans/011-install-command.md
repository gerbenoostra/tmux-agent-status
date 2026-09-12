# 011 - `install`: write the hooks and configs from the tool

Status: planned - nothing implemented. Branch `feat/install-command`.

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

## What changes about the rules

Three rules in `AGENTS.md` and one decision in 001 are **superseded here**, deliberately and with
their original reasoning answered rather than ignored. Anyone reading 001 later must land on this
section.

### "This tool never edits the user's config files"

001, *Never write to `~/.claude/settings.json` from a tool*, rejected a `setup` command. Its four
objections were specific, and each one is a requirement below rather than a refutation:

| 001's objection | Answered by |
| --- | --- |
| the file is frequently a symlink into a dotfiles repo | resolve the chain, edit the target, keep the symlink (safe-write step 1) |
| the agent itself writes it at unpredictable moments | read-precondition re-checked immediately before the rename (steps 4 and 8), an exclusive lock file (step 3), and a post-rename verify that refuses to restore over a writer that beat it (step 11) |
| reserialising reorders keys | `serde_json` with `preserve_order`; a no-op merge must be byte-identical, and a test asserts it |
| a truncate-in-place write loses the file if it loses the race | never truncate: write a sibling temp file and `rename(2)` over the target (step 9) |

The sentence 001 ends on - *"a tool that can do that to a config it did not write has no business
writing it"* - stands as the bar. This plan's answer is that the tool may write such a file **only**
under the contract below, and that the contract is the deliverable, not the subcommand.

The hook commands (`set`, `reset`, `finish`, `clear-window`, `notify`) are unchanged and still write
nothing but two tmux options and a bell. `install` is the one subcommand that touches a user file,
and only when a human types it.

### "Never write, rewrite or splice `window-status-format`"

`AGENTS.md` forbids this, and 001 explains why: writing a spliced format back through
`set-option` has to go to a **window-local** option, freezing that window's format forever.

That hazard is a property of the tmux *option*, not of the format string. **The rule keeps its
teeth and gains a boundary**: this tool still never calls `set-option` on `window-status-format` or
`window-status-current-format`, at any scope, ever. `install --tmux-format` edits the **text of the
user's config file**, which is what the README already asks the user to do by hand and has none of
the freezing behaviour. Reading the option (`show-options`) stays fine; writing it stays forbidden.

A test asserts the string `window-status-format` never appears as an argument to a `set-option`
call anywhere in `src/`.

### "Agent hook entries are documented, never written"

Superseded. They are documented **and** written, under the same contract. The preference order
(plugin > drop-in file > inline merge) exists precisely so that the riskiest route is the last
resort: on a machine with `claude` on `PATH`, `~/.claude/settings.json` is still never touched
by us.

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

"Run nothing" was the first draft and it is not implementable: detection *is* running things.
`--dry-run` cannot show a real plan without asking `tmux` which config files it would load and
`claude plugin list --json` what is already installed, and a dry run that prints a guess is worse
than one that prints the truth.

So the promise is **"changes nothing"**, stated precisely: `--dry-run` runs read-only commands and
performs no write, no rename, no directory creation, and no state-changing external command. The
read-only commands are exactly these, and the list is exhaustive by design:

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

## The shape of a run

Every step is the same five phases, and **nothing writes until every phase-3 answer is in**. That is
what makes `--dry-run` free and what stops a run from leaving two of three steps applied because the
third asked a question the user did not like.

```
detect  ->  plan  ->  confirm  ->  apply  ->  verify
```

- **detect** - read-only. What is installed, what is already configured, where the files are.
- **plan** - build a `Change` per target: path, current bytes, intended bytes, why. Pure functions
  over strings; this is where the merges and splices happen, and where the tests live.
- **confirm** - one prompt per decision, in the order below. `-y` answers all of them.
- **apply** - the safe write, per target, in a fixed order: agents, then tmux hook, then
  tmux format.
- **verify** - re-read, re-parse, compare. A failure here restores and fails the step.

A step that fails does not stop the run: the remaining steps still apply, and the summary at the end
says which of the three landed. No step reads another's output, so a failure cannot corrupt what
follows. The glyph does need all three to appear, but a run that does what it can and says exactly
what it did not beats one that abandons work it was able to finish.

## The safe write

One primitive, used by every file this tool touches. `install::write::safely(path, new_contents)`.

1. **Resolve.** `canonicalize` the path. A symlink chain resolves to its target and the edit lands
   there; the link itself is never replaced, because the rename in step 9 targets the resolved
   path. A path that does not exist canonicalizes its nearest existing ancestor instead, and the
   run continues in **create mode** (below).
2. **Refuse what must not be edited.** An existing target that is not a regular file (directory,
   fifo, socket, device): refuse. **The permission that matters is the parent directory's, not the
   file's** - verified: `rename(2)` over an `r--r--r--` file succeeds when its directory is
   writable, so testing the file's mode would refuse edits that work and permit edits that do not.
   So: parent directory not writable, refuse with the message the "Generated configs" section works
   out. Target file itself read-only but its directory writable: warn and ask, because the mode is
   the user saying "not this one". Outside `$HOME`: warn and ask, since a dotfiles checkout may
   legitimately live elsewhere. More than one hard link: warn and ask, because the rename breaks the
   link and a dotfiles setup that hardlinks would silently diverge. `-y` proceeds on every warning
   and says so.
3. **Lock.** `O_CREAT | O_EXCL` on `<target>.tmux-agent-status.lock`, holding it across 4-13,
   removed on every exit path. A `SIGKILL` or a power cut runs no cleanup, so a stale lock is
   expected, not exceptional: the file therefore records pid, process start time and hostname, and
   is broken only when that exact process is provably gone - a pid alone is reused, and "older than
   60 seconds" alone breaks a live lock held by a slow filesystem. A lock from another host, or one
   whose liveness cannot be established, is reported rather than broken. This serialises *our*
   concurrent runs. It does **not** serialise the agent that also writes the file - nothing can,
   since the agent takes no lock - which is what steps 8 and 11 are for.
4. **Read and fingerprint.** Read the whole file. Record `(len, mtime, hash)`.
5. **Merge.** Build the new contents from the bytes read in step 4 with a pure function. If the
   result equals the input, stop here: nothing to do, **no backup written**, report "already
   installed". This is what makes a second run leave no trace at all.
6. **Back up.** Copy to `<target>.bak-<UTC RFC3339 basic>` beside the target, preserving mode,
   `fsync`ed. Never deleted, never reused, never overwritten.
7. **Write a sibling.** `<target>.tmp-<pid>-<nonce>` in the target's own directory, so the rename in
   step 9 is same-filesystem and therefore atomic. Write, `fsync`, copy the target's mode.
8. **Re-check the fingerprint.** Stat the target again. Changed since step 4 - the agent wrote it, a
   dotfiles sync ran, another editor saved - then abort: delete the temp, keep the file untouched,
   fail the step with "the file changed while we were preparing the edit; nothing was written". The
   window between this check and the rename is small and not zero, and the plan says so out loud.
9. **Rename.** `rename(2)` the temp over the resolved target. A crash at any instant leaves either
   the old file or the new one, never a truncated one.
10. **Fsync the parent directory**, so the rename itself survives a power cut.
11. **Verify, and decide what a mismatch means.** Re-read the target and compare byte for byte
    against what step 7 wrote. A match is success. A mismatch has two possible causes, and they
    want opposite responses, so the tool must tell them apart before it acts:

    | What is on disk | Cause | Response |
    | --- | --- | --- |
    | empty, truncated, or unparseable in its own language | our write did not land intact | **restore the backup**, fail the step |
    | a complete, parseable document that is neither ours nor the backup | a writer that raced past step 8 | **restore nothing**, fail the step, print both paths |

    The second row is the one a blanket restore gets wrong: overwriting a legitimate concurrent
    write with a backup taken before it is exactly the data loss this whole contract exists to
    prevent, and the reason the pre-rename fingerprint alone is not enough. The tool cannot know
    whose write is more valuable, so it stops, keeps both, and says so: the file as it now stands,
    the backup, and the temp file it wrote, all named. That is the only outcome in this plan that
    asks a human to reconcile something, and it is the only one where a human genuinely has to.
12. **Verify semantically, by asking tmux** - for the two tmux steps only; see "Letting tmux mark
    our homework". A failure here rolls the file back, and that rollback is unconditionally safe
    because step 11 just proved the bytes on disk are ours and nobody else's.
13. **Release the lock**, and name the backup path in the summary.

**Create mode**, for a target that does not exist - the new `~/.config/tmux/tmux.conf`, the written
snippet, an agent's first drop-in. Same primitive, three differences: step 4 reads an empty document
rather than a file, step 6 writes no backup because there is nothing to lose, and any missing parent
directories are created first (`0755`, and only under a path the user confirmed). Everything else -
lock, temp file, rename, verify - is identical, so a create still cannot half-land, and a target
that appears between step 1 and step 9 is caught by step 8 like any other concurrent write. The
summary distinguishes "created" from "edited", because they are different things to want to undo.

### Restore is automatic, and this deviates from the request

The request says *"on failing to write, ask the user if it should be restored"*. It should not ask,
and **this deviation is signed off**: asked directly, the answer was "good to have auto restore".

The reasoning: a write that did not land intact leaves the file in a state nobody intended, and that
is the worst possible moment to block on a question - in `-y` there is nobody to ask, and in an
interactive run the honest answer to "shall I put your config back?" is always yes. So for that
case, **restore automatically, then say so loudly**, naming the backup either way.

Automatic restore is scoped to exactly that case by step 11's table. Where the file on disk is a
complete document somebody else wrote, nothing is restored, because there the honest answer is *not*
always yes. So the tool asks no question it can answer itself, and overwrites nothing it cannot
prove was damaged.

If the restore *itself* fails, the tool prints the backup path and the literal `cp` command that
completes it, and exits 1. That is the only path where a user is left with work to do, and it
requires two filesystem failures in a row.

### Generated configs are refused, not edited - but a store path is not the test

A generated config is one the user cannot usefully edit, because the next `home-manager switch`,
`nix profile upgrade` or `stow` run puts it back. Editing it teaches the user a lie with a delayed
fuse. But **"the path is inside `/nix/store`" is not the test**, and a draft of this plan got that
wrong. Verified against a real home-manager machine, both of these exist side by side:

| Case | Chain | Final target | Verdict |
| --- | --- | --- | --- |
| `mkOutOfStoreSymlink` | `~/.tmux.conf` -> `...home-manager-files/.tmux.conf` -> `...hm_.tmux.conf` -> `~/dotfiles/home/.tmux.conf` | a real, writable, git-tracked file the user maintains | **edit it** - it is exactly the file they would open by hand |
| a build product | `~/.tmux/tmux-agent-status.conf` -> `/nix/store/...-tmux-agent-status-0.0.1/share/tmux/...` | `r--r--r--`, root-owned, on a read-only store | **never edit** - and we never wanted to; this one is only ever *sourced* |

Both chains pass *through* `/nix/store`. Only one *ends* there. So the test is the one the
filesystem already answers: after `canonicalize`, **is the final target writable, and is its
directory writable enough to create the temp file and rename over it?** That question needs no
knowledge of nix, Guix, stow, chezmoi or any future dotfiles manager, and it gets the
`mkOutOfStoreSymlink` case right for free.

The store prefix is then used for **the wording only**. Unwritable and inside a package store: "this
file is generated by nix; add the fragment below to the generator that produces it, not to the
file." Unwritable for any other reason: say which reason, and name the resolved path, because the
path the user typed is not the path that refused them.

One note, not a refusal: when the resolved target sits inside a git repository other than the one we
are running in - the dotfiles checkout, in the case above - the confirmation says so, so nobody is
surprised to find an uncommitted change in a repo they were not thinking about.

`docs/install.md` already documents the nix route; the refusal message is the same advice, delivered
at the moment the user needs it.

## Step 1: agent hooks

### Detection

Two independent signals per agent, both reported so the user can see why something was preselected:

- its config directory exists (`~/.codex`, `~/.cursor`, `~/.factory`, `~/.grok`, `~/.kiro`,
  `~/.vibe`, `~/.copilot`, `~/.claude`, `~/.gemini`, `~/.config/devin`)
- its command is on `PATH`

An agent is **preselected** when either hits, listed but unselected when neither does. Never
installed without appearing in the list. The per-agent command names are taken from 010's verified
installs where 010 recorded them and **confirmed during implementation otherwise** - a guessed
binary name is a wrong detection, and this repo does not ship guesses (005, "a wrong glyph is worse
than no glyph"; the same logic applies to a wrong preselection).

### Delivery, in preference order

Per the request: plugin > drop-in file > inline merge. Concretely, and these are the only three:

| Class | Agents | What we do | Risk |
| --- | --- | --- | --- |
| **plugin** | Claude Code | shell out to `claude plugin marketplace add` + `claude plugin install -y --json` | none of ours: Claude Code writes its own bookkeeping (004) |
| **own file** | Copilot, Grok, Kiro | write a whole file that is ours alone | lowest; nothing to merge |
| **shared file** | Codex, Cursor, Droid, Mistral Vibe, Gemini, Devin (user scope), Claude Code (fallback) | merge our entries into a file the user maintains | the safe write exists for this row |

### The plugin route, verified

Both commands are fully non-interactive and machine-readable, confirmed against the installed CLI:

```sh
claude plugin marketplace list --json      # idempotency: is our marketplace known?
claude plugin list --json                  # idempotency: id starts "tmux-agent-status@"
claude plugin marketplace add gerbenoostra/tmux-agent-status
claude plugin install tmux-agent-status@tmux-agent-status -y --json
```

`marketplace add` clones from GitHub, so its prompt says so; `-y` covers it, and `--dry-run` prints
the commands without running them.

**When the plugin route fails, the step fails. It does not fall back.** The routes are chosen by
what is *available*, not by what *worked*:

| Situation | What happens |
| --- | --- |
| `claude` absent from `PATH` | fall to the merge into `~/.claude/settings.json`; the plugin route was never available |
| `claude` present, plugin commands succeed | plugin installed; `settings.json` untouched, as 004 promises |
| `claude` present, plugin commands **fail** (old CLI, no network, marketplace down) | **fail the step**, print the command and its stderr verbatim, suggest `--claude-route=settings` |

Falling back on failure is the tempting choice and the wrong one: it would edit
`~/.claude/settings.json` on a machine where the user was promised it would not be, as the silent
consequence of a network blip. A user who wants that outcome can have it by asking -
`--claude-route=settings` forces the merge, and `--claude-route=plugin` forces the plugin and fails
if it cannot - but nobody gets it by accident. The failure is loud, the step is retryable, and no
other step is affected.

### The repository slug

`gerbenoostra/tmux-agent-status` is the project's own identity, not personal configuration: it is
already in `Cargo.toml`, the README and `docs/agents/claude-code.md`, and a stranger who cloned the
repo needs exactly this string to install the plugin. So it is not a leak.

It is, however, wrong for a **fork**, and the fix is free: derive the default from
`env!("CARGO_PKG_REPOSITORY")`, which `Cargo.toml` already carries, so a fork gets its own slug by
changing the manifest it was going to change anyway. `--marketplace <source>` overrides it, which is
also how a developer points the installer at a local checkout.

### Where the drop-in contents come from

**Embedded in the binary with `include_str!`, not read from `share/agents/` at runtime.**
`cargo install` ships the binary and nothing else (`docs/install.md`), so a runtime path lookup
would leave the largest install route unable to install anything. Embedding also removes a whole
class of "found the wrong copy" bugs, costs a few kilobytes, and keeps `tests/agent_configs.rs`'s
drift checking meaningful, because the embedded bytes *are* the shipped files.

### Merge rules

Our entries are identified by a command string beginning `tmux-agent-status `. For each event key we
own: drop every existing entry whose command matches, then insert ours. Everything else in the
document - other events, other keys, key order, unrelated hooks on the same event - is preserved.
`serde_json`'s `preserve_order` feature is what makes "preserved" true rather than aspirational.

Two shapes need naming because they are not the common one:

- **Droid** puts the event names at the top level, with no wrapping `hooks` key
  (`docs/agents/droid.md`). Nesting them under `hooks` gives a config Droid ignores silently.
- **Devin** user-scope nests the whole drop-in under `hooks` in `~/.config/devin/config.json`, and
  *one* unknown event key discards the entire hook map (`docs/agents/devin.md`). So the Devin merge
  must never contribute a key outside its eight documented events, and the verify step re-checks
  that after writing.

**Mistral Vibe is TOML** and gets no full TOML parser, but "the original bytes are still a prefix"
is not the check it needs: that proves the old content survived, not that the new content is valid.
An appended `[[hooks]]` header is valid TOML after *almost* anything, and the exception is real - a
file whose last line is inside an unclosed multi-line string swallows our block into that string.

So the append is guarded by a **lexical top-level scan**: walk the file tracking only what changes
where a table header may legally appear - `#` comments to end of line, `"` and `'` single-line
strings, and `"""` / `'''` multi-line strings. If the scan does not end at top level, **refuse the
step** and print the block for the user to place by hand. This is perhaps sixty lines, it is
exhaustively testable from fixtures, and it is the difference between "probably fine" and "checked".

Two mechanical details that are easy to get wrong and are therefore written down: the block is
preceded by a newline if the file does not already end in one, or the marker lands on the tail of
the user's last line; and the block ends in a newline of its own.

Idempotency is **the marker comments alone**. A line merely carrying `tmux-agent-status notify`
could be a comment about the tool, a wrapper hook of the user's own, or a stale entry they meant to
delete, and treating any of those as "installed" silently does nothing while reporting success.
Instead such a line outside our markers is a **warning**: "this file already mentions
tmux-agent-status outside the block we manage; review it". Deleting the markers therefore causes a
reinstall, which is the documented and correct behaviour.

### Scope

User scope for every agent, because a per-project install is a per-project surprise. Devin has no
user-scope drop-in, so Devin gets the `~/.config/devin/config.json` merge and the project file is
mentioned, not written.

## Step 2: the tmux `source-file` line

### Finding the snippet

In order, first hit wins:

1. `--snippet <path>`
2. relative to the running executable: `../share/tmux/tmux-agent-status.conf`, which covers the
   nix profile, the release tarball and a source install
3. the usual prefixes: `$PREFIX/share`, `~/.nix-profile/share`, `/usr/local/share`,
   `/opt/homebrew/share`
4. nothing found - offer to write the embedded copy to `~/.config/tmux/tmux-agent-status.conf`
   (or `~/.tmux/` when `~/.tmux.conf` is the config in use), which is what `cargo install` needs

### Finding the config

`tmux display-message -p '#{config_files}'` against a running server gives tmux's own answer,
verified on 3.6a to return `/etc/tmux.conf,~/.tmux.conf,~/.config/tmux/tmux.conf`. Two things about
that list, both verified: it names **candidates**, including files that do not exist, and when the
server was started with `-f` it names only that file. So it is a source of candidates, not a
decision.

The decision is this order, and "preferring the user one" is not a good enough specification:

1. `--tmux-config <path>`, which overrides everything below
2. `$XDG_CONFIG_HOME/tmux/tmux.conf` if it exists
3. `~/.config/tmux/tmux.conf` if it exists
4. `~/.tmux.conf` if it exists
5. none of them exist: offer to **create** `~/.config/tmux/tmux.conf`, the location tmux documents
   and the one that does not clutter `$HOME`

`/etc/tmux.conf` is never chosen, even when it is the only one that exists and even under `-y`. It
needs root, and it installs the tool for every user of the machine, which is not what anyone typing
this command meant. It is reported, with the suggestion to pass `--tmux-config` if that really was
the intent.

**With no tmux at all** - not installed, or no server running - every tmux invocation in this plan
is optional and its failure is not an error. Discovery falls to steps 2-5 above, the format default
falls to its hard-coded value, the reload offer is not made, and the summary says plainly which
checks could not be run against a live tmux. Installing the config before installing tmux is a
legitimate order to do things in.

### Idempotency

Any `source` or `source-file` command whose last argument has the basename
`tmux-agent-status.conf`, at any path, means installed. As asked: the location is the user's
business, and a second source line is a second set of hooks.

### The edit

Append at end of file, inside markers:

```tmux
# >>> tmux-agent-status >>>
source-file ~/.config/tmux/tmux-agent-status.conf
# <<< tmux-agent-status <<<
```

The markers are what makes the future `uninstall` mechanical. Position does not matter here: the
snippet sets hooks only, and a hook set late is a hook set.

## Step 3: the format term

The one term, unchanged from 001 and the README:

```tmux
#{?@agent_status, #{@agent_status},}
```

### Idempotency

If the value already references `@agent_status`, leave it exactly as it is and report it. As asked:
wherever the user put it, they put it there on purpose.

"References" is a **token match, not a substring match**: `@agent_status` not followed by a
`[A-Za-z0-9_]`, so a user's unrelated `#{@agent_status_colour}` does not read as ours. Matching the
full literal term instead would be the opposite error - it would miss anyone who dropped the space
or wrapped the term in styling, and then install a second copy beside their first.

### Finding the line

"Search the main file, and only look at sourced files if it has none" was the first draft, and it is
**wrong**. Verified on tmux 3.6a: a main file that sets the format and *then* sources a fragment
that sets it ends up with the fragment's value, and reversing the two reverses the winner. tmux
executes config commands strictly in the order it encounters them, `source-file` included, and the
last assignment wins - so the file that owns the value is decided by position, not by depth.

So the search reconstructs **tmux's own command order**: walk the config from the top, descend into
each `source-file` at the point it appears (globs expanded and sorted the way tmux sorts them,
confirmed by the same probe), depth-limited with a visited set against a cycle, and collect every
matching line in that order. **The last one is the one to edit.** Editing any earlier one produces a
line tmux discards, which is the worst outcome available: a successful-looking install with no glyph
and nothing to see in the diff.

The file about to be edited is always named in the confirmation, and when the winning line is in a
file we may not write (generated, unwritable), the step reports that rather than editing a loser
that would have no effect.

A candidate line is a tmux command whose words are:

- a command name in `set`, `set-option`, `setw`, `set-window-option`
- any of the flags tmux accepts there
- the option name `window-status-format` or `window-status-current-format`
- a value, single-quoted, double-quoted or bare

with trailing-backslash continuations joined first. **The last matching line wins**, because that is
what tmux does.

Two things the parser refuses rather than guesses: a line carrying a **second command**, and a value
it cannot requote safely. Both fall back to the manual path below, which is the whole point of
having one.

The `;` test must be lexical, not `contains(';')`. Verified: `set -g window-status-format
'SEMI;INSIDE...'` loads fine and the semicolon is part of the value, so a naive scan would refuse a
perfectly ordinary line. Only a `;` the tokenizer meets **outside** quotes separates commands - the
same tokenizer that found the value, which is why the two questions are answered together and not by
two different pieces of code.

### The splice

Insert the term immediately before the first `#{?window_flags` in the value, or at the end of the
value when there is no flags term - the position 001 specifies, after the name segment and outside
any truncation. Re-emit in the original quoting style, with one case that is not optional:

**A bare value must be requoted, always.** The term contains a space, and a bare value ends at the
first space. Verified: a bare `set -g window-status-format BARE#{?window_flags,#{window_flags}, }`
is discarded by tmux and the option keeps its default - silently, with the line still sitting in the
config looking correct. Single quotes are always safe here because the term contains no single
quote; a value that already contains one is re-emitted double-quoted, and a value that defeats both
falls to the manual path.

### When there is no line at all

The user is on tmux's compiled-in default. Read it version-correctly rather than from memory:

```sh
tmux -L tmux-agent-status-probe -f /dev/null start-server \; show-options -gwv window-status-format
```

`-f /dev/null` means the user's config is not loaded and the probe server has no side effects; kill
it afterwards. Asking tmux, rather than hard-coding a default, is the point: the default has changed
between tmux versions and the one in *this* tmux is the only one that is right. On tmux 3.6a it
returns `#I:#W#{?window_flags,#{window_flags}, }`, which is also the fallback if tmux cannot be run
at all.

That default then goes through the same splice as any other value, and the step writes a **new
pair** of lines - both options, because a term in only one of them makes the glyph vanish the
moment the window becomes current:

```tmux
# >>> tmux-agent-status >>>
set -g window-status-format '#I:#W#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
set -g window-status-current-format '#I:#W#{?@agent_status, #{@agent_status},}#{?window_flags,#{window_flags}, }'
# <<< tmux-agent-status <<<
```

Written inside markers, because unlike a spliced line this block is entirely ours, which makes the
later `uninstall` a deletion rather than a second splice. A user who afterwards writes their own
format line below this block wins, exactly as tmux's last-one-wins rule says they should.

### The confirmation

Show the current and proposed value for **both** options, then: accept, edit, or skip. Editing a
300-character format string in a one-line prompt is hostile, so "edit" opens `$EDITOR` on the
proposed value; a short value can be edited inline. Whatever comes back is re-checked for the term
before it is written.

### When the parser bails, or the user says no

The step does not fail and nothing is written. The tool prints the term, the file and line it found,
and - where it got far enough to build one - the whole proposed line, ready to paste. The summary
marks the format step as **not installed, here is what to do**, and the run's exit code is 0: a
refusal the user chose is not an error.

This path is the reason the parser is allowed to be narrow. It is also the state a user is in today,
so falling into it leaves them no worse off than before they ran anything.

### Reloading

Never `set-option`. After a successful format or hook step, offer to run `tmux source-file <config>`
if a server is running - the same command the user would type, confirmed like everything else. By
this point the probe has already loaded that exact file in a throwaway server and found it sound, so
the reload is being offered on a file that is known to parse, not hoped to.

Agents still need their own restart, and the summary says so.

## Letting tmux mark our homework

Our own parser round-trip proves our parser agrees with itself. It does not prove tmux agrees. So
after editing a tmux config, **start a throwaway tmux on the edited file and ask it what the format
actually is**. This is the difference between "we believe we spliced correctly" and "tmux read it
back and it says what we meant".

### Why this is required, not a nicety

Verified on tmux 3.6a, and it is worse than a wrong glyph:

| What our edit could produce | What tmux does with it |
| --- | --- |
| an unknown command name | **abandons the entire config file**, including lines *before* the error |
| a valid command with a stray extra argument - exactly what a quoting bug produces | **abandons the entire config file** |
| an unterminated quote at end of file | keeps everything else; that one value is odd |
| a bare value containing a space | discards that line, keeps the rest |

The first two are the ones that matter. tmux parses the whole file before running any of it, so one
malformed line does not cost the user our glyph - it costs them **their entire tmux configuration**,
silently, with no error they will see and a config file that still looks right. A tool that can do
that to a file it did not write has no business writing it, and this probe is the price of the
licence.

### How

Both tmux steps, after the byte-level verify and before the lock is released:

1. **Baseline**, taken before anything is written: `tmux -L <unique> -f <the entry-point config>
   new-session -d`, then dump `show-options -g`, `show-options -gw` and `show-hooks -g`/`-gw`. Kill
   the server.
2. **Candidate**: the same dump against the edited file.
3. **Compare the dumps, not just our option.** The assertion is that the *only* differences between
   baseline and candidate are the ones we intended: the two format values gaining exactly our term,
   and for the hook step our two hooks appearing. Anything else changing - or everything reverting
   to tmux's defaults, which is what an abandoned config looks like - fails the step.

Comparing whole dumps rather than reading back one option is what catches the abandoned-config case,
because an abandoned config still answers `show-options -gwv window-status-format` perfectly
happily. It just answers with the default.

### The order is validate-after-write, with rollback

Tempting to probe the temp file before renaming it. That works only when the file we edited *is* the
entry point tmux loads, and it often is not: the winning format line can live in a sourced fragment,
and probing a fragment on its own tests a config the user does not have. Copying the tree to a
sandbox does not help either, since a config is full of absolute paths.

So the write lands first, the probe runs against the real config, and a failure rolls back through
the backup. The exposure is a bad config on disk for the length of one `new-session -d`, and tmux
reads a config only at server start or on an explicit `source-file`, so nothing is affected unless a
server happens to start inside that window. That is a far better trade than shipping an edit nobody
checked.

### What it costs, stated plainly

- **It loads the user's real config in a throwaway server**, which means their `run-shell`,
  `if-shell` and any plugin manager bootstrap actually execute. That is a side effect, it can be
  slow, and it can touch the network. It is disclosed in the confirmation, bounded by a timeout, and
  skippable with `--no-tmux-probe`, which downgrades the step to the parser's own word.
- **cwd matters.** Verified: tmux resolves a relative `source-file` against the process's working
  directory, not the config's. The probe runs with cwd set to `$HOME`, and a config using relative
  source paths is reported as such, since it is already fragile for the user's own tmux.
- **`$TMUX` is cleared** in the probe's environment, and the socket name is unique per run.
- **The server is killed on every exit path**, including panic and interrupt. A leaked tmux server
  is exactly the kind of residue this tool promises not to leave.
- **No tmux, no probe.** The step still installs and the summary says the edit could not be checked
  against a live tmux.

### When the config was already broken

The baseline is what makes this honest. If the baseline dump is tmux's defaults while the config
file plainly sets things, the user's config was **already** abandoned before we arrived. Then: do
not edit, and say so. Editing would produce an install that cannot be validated, and the user would
reasonably blame the tool that touched the file last for a breakage it inherited.

## Decisions taken here

| Decision | Choice | Why |
| --- | --- | --- |
| write mechanism | temp file plus `rename(2)`, never truncate | the only way a crash cannot lose the file; answers 001 directly |
| backup location | beside the target, `.bak-<timestamp>`, never pruned | discoverable without knowing a state directory exists, and `install` is not the hook path that 001 forbids state files on |
| concurrency | `O_EXCL` lock plus a fingerprint re-checked before the rename | the lock stops *our* races; the fingerprint is the only defence against a writer that takes no lock, and it is honest about its small window |
| restore | automatic on a failed verify, never a prompt | the request asked for a prompt; a broken config is the worst moment to block, and `-y` has nobody to ask |
| drop-in contents | `include_str!` into the binary | `cargo install` ships no `share/`, and that is the largest install route |
| generated configs | refuse on **writability of the resolved target**, never on a path prefix | verified: a `mkOutOfStoreSymlink` chain passes through `/nix/store` and ends in a writable dotfiles checkout, which a prefix test would have wrongly refused |
| format string | edit the config file text; never `set-option` | 001's freezing hazard is a property of the option, not the string |
| checking the format edit | a throwaway `tmux -f <edited config>` whose whole option dump is diffed against a baseline | our parser agreeing with itself proves nothing; and a malformed line costs the user their entire config, not just our glyph |
| format parser | narrow, and bails to manual when unsure | a general tmux parser is a project; a parser that knows when to stop is a feature |
| JSON | `serde_json` with `preserve_order` | a merge that reorders a user's keys is a diff they did not ask for |
| TOML | append marked tables, no parser | valid for any TOML file, and one agent does not justify a formatting-preserving TOML dependency |
| prompts | `dialoguer` (`MultiSelect`, `Confirm`, `Editor`) | the agent list is genuinely a checkbox and the format string genuinely needs an editor; `inquire` is the equivalent alternative if `dialoguer`'s `Editor` disappoints |
| `HOME` resolution | read `$HOME` / `$XDG_CONFIG_HOME` from the environment, never `getpwuid` | tests point a child process at a temp home; a tool that cannot be redirected cannot be tested |
| non-interactive | exit 2, never a silent default | a tool that edits configs unattended is a tool nobody asked to run |

## Uninstall, designed not shipped

Not in this plan, but every choice above is made so it is mechanical later, and a follow-up plan
writes it:

- marker comments around the tmux `source-file` block
- marker comments around the Vibe TOML block
- JSON entries identified by their `tmux-agent-status ` command prefix, which is already the merge
  key
- own-file targets are a delete
- Claude Code is `claude plugin uninstall` + `marketplace remove`
- the format term is a known literal to remove, with the same parser

The one case uninstall cannot do cleanly is a format term the user has since moved or edited by
hand, which is exactly the case where it should refuse and print.

## Work items

**Item 0 is a prerequisite, not a doc task.** `AGENTS.md` is an always-on rule file that currently
forbids what items 1-9 do, so it is updated and committed *first*, on its own. Any other order asks
every future session to read a rule and then violate it, and makes the review of the code a
referendum on the rule instead of on the code.

0. `AGENTS.md` - rewrite the three superseded rules per "What changes about the rules", pointing at
   this file, keeping the `set-option` half of the format rule intact and explicit. Its own commit,
   before any code.
1. `src/install/mod.rs` - the five phases, the step algebra, the summary. `Change` is the type the
   whole step passes around; nothing outside `apply` writes.
2. `src/install/write.rs` - the safe write, in full, including lock, backup, fingerprint, verify and
   restore. This module is the deliverable; everything else is a caller.
3. `src/install/agents.rs` - the agent table (name, detect dir, detect command, target path, merge
   class, embedded contents), the JSON merges, the TOML append, the Claude plugin route.
4. `src/install/tmux_conf.rs` - config discovery, snippet discovery, the source-file block.
5. `src/install/format.rs` - the line parser, the splice, the requoting.
6. `src/install/probe.rs` - the throwaway-server probe: unique socket, cleared `$TMUX`, cwd `$HOME`,
   timeout, kill on every exit path, and the baseline-versus-candidate dump comparison. Used for
   the compiled-in default, the baseline and the semantic verify, so it exists once.
7. `src/install/prompt.rs` - the only module that knows about a TTY; `-y` and `--dry-run` are
   answered here so no other module branches on interactivity.
8. `src/main.rs` - the `install` subcommand and its flags in the pico-args dispatch, plus help text.
   `install` does **not** route through `run_hook`: a hook exits 0 whatever happens, and an
   installer that hides a failure is worthless. `main.rs`'s comment on `hook()` already
   anticipates this.
9. `Cargo.toml` - `serde_json` gains `preserve_order`; `dialoguer` is added.
10. Docs: README gets `tmux-agent-status install` as the lede of the setup section with the manual
   four steps kept below it; `docs/agents/README.md` notes which agents the installer covers;
   `docs/install.md` ends each route with the one-liner; the plugin's `/tmux-agent-status:doctor`
   suggests `install` for each failing check.

## Verification

The bar is the repo's: only test-and-e2e-verified code lands, and a bug fix without a reproduction
solves the wrong problem. Written before the code where it can be.

### Pure, no filesystem

11. **Format line parser** against a corpus of real lines: bare, single-quoted, double-quoted,
   `setw`, `set -gw`, backslash continuation, `;`-joined (refused), a value containing `#(...)` with
   embedded double quotes and nested `#{}` (the shape a real config has), a value already carrying
   `@agent_status` (untouched), no line at all.
12. **Splice**: before `#{?window_flags`, at the end when absent, requoting each way, and the
   round-trip property *parse then emit with no change is byte-identical*.
13. **JSON merge**, per merge class: empty file, no `hooks` key, unrelated hooks preserved, our
   entries already present (**byte-identical output**), our entries present but stale (replaced, not
   duplicated), key order preserved, Droid's top-level shape, Devin's eight-key constraint.
14. **Step algebra**: every flag combination, including the positive-and-negative usage error and an
   unknown `--agents=` name (exit 2, valid names listed).
15. **Config-order resolution**: a main file that sets the format then sources a fragment that sets
   it, and the reverse; the last assignment in tmux's own order is the one selected. This is the
   defect a draft of this plan had, and it is the test that keeps it fixed.
16. **The TOML top-level scan**, from fixtures: a file ending inside `"""`, inside `'''`, inside a
   comment, inside a single-line string, and cleanly at top level; only the last is appended to.
17. **Tokenizer**: a `;` inside single quotes is part of the value, a `;` outside separates commands
   and refuses the line.

### Filesystem, against a temp `HOME`

18. **Symlink**: a chain two deep; the edit lands on the final target and the links are still links.
19. **Hard link**: detected, and the warning fires.
20. **Read-only parent**: clean failure, original intact, exit 1.
21. **Writability, both directions.** A chain ending in a genuine read-only store file: refused,
    with the generator fragment printed. A chain that *passes through* a read-only store and ends
    in a writable file: **edited normally** - the `mkOutOfStoreSymlink` case, a regression test for
    a rule this plan got wrong once already. Plus a read-only file in a writable directory, which
    `rename(2)` can replace and which must therefore warn and ask rather than refuse.
22. **Backup**: created, matches the pre-state byte for byte, named in the output.
23. **Fault injection** - the single most important test here. A test-only switch
    (`TMUX_AGENT_STATUS_TEST_FAULT=<stage>`, documented as unstable and unsupported) makes the write
    produce truncated, empty or scrambled bytes. Assert verify catches every one, the restore runs,
    and the file afterwards is **byte-identical to before the run**. Repeat with a fault in the
    restore itself: the exit is 1 and the message names the backup.
24. **The concurrent writer is not clobbered.** The counterpart to 13, and the case a blanket
    restore gets wrong: a fault that replaces the target, after our rename, with a *complete and
    parseable* document that is neither ours nor the backup. Assert nothing is restored, the step
    fails, and the message names the file, the backup and the temp file. Then assert the damaged
    variants of the same test still *do* restore, so the two rows of step 11 are both pinned.
25. **Create mode**: a target that does not exist, with a missing parent directory. Assert the
    directory and file are created, no backup is written, the summary says "created", and a second
    run reports already installed.
26. **Concurrency**: N processes installing into the same file at once. Afterwards the file is valid
    in its own language and carries exactly one copy of our entries; every loser reported a clean
    precondition or lock failure and wrote nothing.
27. **Idempotency**: two runs. The second writes no file, creates no backup, and reports every step
    as already installed. Asserted on file mtimes, not just on output.

### CLI

28. `--dry-run` golden output, and an assertion that the filesystem is unchanged afterwards -
    including that the probe server's socket is gone and no lock, temp or backup file was left.
29. Non-TTY without `-y` exits 2 with a message naming `-y`.
30. Exit codes for a mixed run: one step already installed, one applied, one failed.
31. **The Claude route does not fall back on failure**: with a stub `claude` on `PATH` that exits
    non-zero, the step fails, `~/.claude/settings.json` is untouched, and the message names
    `--claude-route=settings`. With `claude` absent, the same run merges into `settings.json`.
32. **No tmux on `PATH`**: discovery, format default and reload all degrade as described, the run
    still installs what it can, and the summary says what could not be checked.

### The tmux probe, extending `tests/tmux_server.rs`

These are the tests that make the probe worth its cost. Each one is a config the parser might get
wrong, checked by the thing that actually decides:

33. **The probe catches an abandoned config.** Hand the writer a deliberately malformed splice (a
    stray argument after the value). Assert the dump comparison fails, the file is rolled back, the
    step reports failure, and the original config still produces its original options.
34. **The probe passes a good splice**, and the dump diff contains *only* the two format values.
35. **A pre-broken config is refused**, not edited: the baseline is defaults, the tool says so, and
    nothing is written.
36. **The probe cleans up**: no server on the probe socket afterwards, in the success, failure and
    interrupt cases.
37. **Relative `source-file`** in the config: the probe runs with cwd `$HOME`, and the case is
    reported rather than silently mis-resolved.
38. **No tmux on `PATH`**: the probe is skipped, the step still installs, the summary says the edit
    was not checked against tmux.

### Real tmux, extending `tests/tmux_server.rs`

39. Write a temp config with a known format, `install --tmux-format --tmux-hook -y
    --tmux-config <path>`, then start `tmux -L <name> -f <path>` and assert: the term is in both
    options, both hooks are registered, and a hand-set `@agent_status` renders in the window entry.
40. The same against a config with **no** format line, proving the default probe produces a working
    pair of lines.
41. The `set-option`-never test: `window-status-format` never appears as a `set-option` argument
    anywhere in `src/`.

### Drift, extending `tests/agent_configs.rs`

42. Every directory under `share/agents/` has a row in the installer's agent table, and every row's
    embedded contents equal the shipped file. A new agent cannot be added without the installer
    learning about it.

### By hand, on a real machine

The steps a checkout cannot fake, run once before merge and recorded here:

43. A real `install` on a machine with several agents, followed by a real turn producing a real
    glyph, with `git diff` in the dotfiles repo showing exactly the intended edits and nothing else.
44. The Claude Code plugin route end to end, against a throwaway `HOME`, confirming that
    `~/.claude/settings.json` still has no `hooks` entry of ours (004's promise).
45. A home-manager machine, both chains: a `mkOutOfStoreSymlink` `~/.tmux.conf` is edited in the
    dotfiles checkout and the symlinks survive; a genuinely store-resident file is refused and
    prints something the user can paste into their generator.
