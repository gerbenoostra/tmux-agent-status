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
| the agent itself writes it at unpredictable moments | read-precondition re-checked immediately before the rename, plus an exclusive lock file (steps 4 and 8) |
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
  --dry-run        print the plan and exit 0; write nothing, run nothing

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
   path. A path that does not exist resolves its parent instead.
2. **Refuse what must not be edited.** Not a regular file (directory, fifo, socket, device): refuse.
   The resolved file, or the directory the temp file and rename need, not writable: refuse with the
   message the "Generated configs" section works out. Outside `$HOME`: warn and ask, since a
   dotfiles checkout may legitimately live elsewhere. More than one hard link: warn and ask, because
   the rename breaks the link and a dotfiles setup that hardlinks would silently diverge. `-y`
   proceeds on both warnings and says so.
3. **Lock.** `O_CREAT | O_EXCL` on `<target>.tmux-agent-status.lock`, holding it across 4-12,
   removed on every exit path. A lock older than 60 seconds whose recorded pid is gone is offered
   for breaking. This serialises *our* concurrent runs. It does **not** serialise the agent that
   also writes the file - nothing can, since the agent takes no lock - which is what step 8 is for.
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
11. **Verify.** Re-read the target and compare byte for byte against what step 7 wrote. Then
    re-parse it in the target's own language (JSON parses and contains our entries; a tmux config
    still contains our line; a TOML file still starts with the bytes it started with). Either check
    failing means the file on disk is not what we intended - a filesystem that lied, a writer that
    raced past step 8, a bug of ours - and the response is to **restore the backup** by the same
    temp-and-rename dance, then fail the step.
12. **Release the lock**, and name the backup path in the summary.

### Restore is automatic, and this deviates from the request

The request says *"on failing to write, ask the user if it should be restored"*. It should not ask.
A failed verify means the file on disk is in a state nobody intended, and that is the worst possible
moment to block on a question - in `-y` there is nobody to ask, and in an interactive run the honest
answer to "shall I put your config back?" is always yes. So: **restore automatically, then say so
loudly**, naming the backup path either way.

The case that would have justified a prompt cannot occur: a write that lost the race is caught at
step 8 and never renames, so there is never a half-applied edit whose fate is genuinely ambiguous.

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
the commands without running them. If `claude` is absent, fall to the shared-file merge into
`~/.claude/settings.json` under `hooks` - the route the README documents as the manual paste.

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

**Mistral Vibe is TOML** and gets no TOML parser. Its three `[[hooks]]` array-of-table entries are
appended at end of file inside marker comments. Appending a table header at EOF is always valid
TOML, whatever came before, unless the file ends inside a multi-line string - which verify catches
by re-reading and confirming the original bytes are still an exact prefix. Idempotency is the
marker, or any line carrying `tmux-agent-status notify`.

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

`tmux display-message -p '#{config_files}'` against a running server gives tmux's own answer -
verified to return `/etc/tmux.conf,~/.tmux.conf,~/.config/tmux/tmux.conf` on tmux 3.6a - filtered to
files that exist, preferring the user one. With no server running, `$XDG_CONFIG_HOME/tmux/tmux.conf`
then `~/.tmux.conf`. With neither present, offer to create `~/.config/tmux/tmux.conf`.
`--tmux-config` overrides all of it.

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

If the value already contains `@agent_status` anywhere, leave it exactly as it is and report it.
As asked: wherever the user put it, they put it there on purpose.

### Finding the line

Search the config file, then - only if no line is found there - the files it `source-file`s, one
level deep, glob expanded. Frameworks put the status line in a fragment, and a tool that edits the
top-level file when the real definition is in a sourced one has written a line that never takes
effect. The file about to be edited is always named in the confirmation.

A candidate line is a tmux command whose words are:

- a command name in `set`, `set-option`, `setw`, `set-window-option`
- any of the flags tmux accepts there
- the option name `window-status-format` or `window-status-current-format`
- a value, single-quoted, double-quoted or bare

with trailing-backslash continuations joined first. **The last matching line wins**, because that is
what tmux does.

Two things the parser refuses rather than guesses: a line with `;`-separated commands, and a value
it cannot requote safely. Both fall back to the manual path below, which is the whole point of
having one.

### The splice

Insert the term immediately before the first `#{?window_flags` in the value, or at the end of the
value when there is no flags term - the position 001 specifies, after the name segment and outside
any truncation. Re-emit in the original quoting style; when the original is bare, or its quoting
cannot carry the term, re-emit single-quoted, which is always safe because the term contains no
single quote.

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
if a server is running - the same command the user would type, confirmed like everything else.
Agents still need their own restart, and the summary says so.

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

1. `src/install/mod.rs` - the five phases, the step algebra, the summary. `Change` is the type the
   whole step passes around; nothing outside `apply` writes.
2. `src/install/write.rs` - the safe write, in full, including lock, backup, fingerprint, verify and
   restore. This module is the deliverable; everything else is a caller.
3. `src/install/agents.rs` - the agent table (name, detect dir, detect command, target path, merge
   class, embedded contents), the JSON merges, the TOML append, the Claude plugin route.
4. `src/install/tmux_conf.rs` - config discovery, snippet discovery, the source-file block.
5. `src/install/format.rs` - the line parser, the splice, the requoting, the default probe.
6. `src/install/prompt.rs` - the only module that knows about a TTY; `-y` and `--dry-run` are
   answered here so no other module branches on interactivity.
7. `src/main.rs` - the `install` subcommand and its flags in the pico-args dispatch, plus help text.
   `install` does **not** route through `run_hook`: a hook exits 0 whatever happens, and an
   installer that hides a failure is worthless. `main.rs`'s comment on `hook()` already
   anticipates this.
8. `Cargo.toml` - `serde_json` gains `preserve_order`; `dialoguer` is added.
9. Docs: README gets `tmux-agent-status install` as the lede of the setup section with the manual
   four steps kept below it; `docs/agents/README.md` notes which agents the installer covers;
   `docs/install.md` ends each route with the one-liner; the plugin's `/tmux-agent-status:doctor`
   suggests `install` for each failing check.
10. `AGENTS.md` - rewrite the three superseded rules per "What changes about the rules", pointing at
    this file, keeping the `set-option` half of the format rule intact and explicit.

## Verification

The bar is the repo's: only test-and-e2e-verified code lands, and a bug fix without a reproduction
solves the wrong problem. Written before the code where it can be.

### Pure, no filesystem

1. **Format line parser** against a corpus of real lines: bare, single-quoted, double-quoted,
   `setw`, `set -gw`, backslash continuation, `;`-joined (refused), a value containing `#(...)` with
   embedded double quotes and nested `#{}` (the shape a real config has), a value already carrying
   `@agent_status` (untouched), no line at all.
2. **Splice**: before `#{?window_flags`, at the end when absent, requoting each way, and the
   round-trip property *parse then emit with no change is byte-identical*.
3. **JSON merge**, per merge class: empty file, no `hooks` key, unrelated hooks preserved, our
   entries already present (**byte-identical output**), our entries present but stale (replaced, not
   duplicated), key order preserved, Droid's top-level shape, Devin's eight-key constraint.
4. **Step algebra**: every flag combination, including the positive-and-negative usage error.

### Filesystem, against a temp `HOME`

5. **Symlink**: a chain two deep; the edit lands on the final target and the links are still links.
6. **Hard link**: detected, and the warning fires.
7. **Read-only parent**: clean failure, original intact, exit 1.
8. **Writability, both directions.** A chain ending in a genuine read-only store file: refused, with
   the generator fragment printed. A chain that *passes through* a read-only store and ends in a
   writable file: **edited normally** - this is the `mkOutOfStoreSymlink` case, and it is a
   regression test for a rule this plan got wrong once already.
9. **Backup**: created, matches the pre-state byte for byte, named in the output.
10. **Fault injection** - the single most important test here. A test-only switch
    (`TMUX_AGENT_STATUS_TEST_FAULT=<stage>`, documented as unstable and unsupported) makes the write
    produce truncated, empty or scrambled bytes. Assert verify catches every one, the restore runs,
    and the file afterwards is **byte-identical to before the run**. Repeat with a fault in the
    restore itself: the exit is 1 and the message names the backup.
11. **Concurrency**: N processes installing into the same file at once. Afterwards the file is valid
    in its own language and carries exactly one copy of our entries; every loser reported a clean
    precondition or lock failure and wrote nothing.
12. **Idempotency**: two runs. The second writes no file, creates no backup, and reports every step
    as already installed. Asserted on file mtimes, not just on output.

### CLI

13. `--dry-run` golden output, and an assertion that the filesystem is unchanged afterwards.
14. Non-TTY without `-y` exits 2 with a message naming `-y`.
15. Exit codes for a mixed run: one step already installed, one applied, one failed.

### Real tmux, extending `tests/tmux_server.rs`

16. Write a temp config with a known format, `install --tmux-format --tmux-hook -y
    --tmux-config <path>`, then start `tmux -L <name> -f <path>` and assert: the term is in both
    options, both hooks are registered, and a hand-set `@agent_status` renders in the window entry.
17. The same against a config with **no** format line, proving the default probe produces a working
    pair of lines.
18. The `set-option`-never test: `window-status-format` never appears as a `set-option` argument
    anywhere in `src/`.

### Drift, extending `tests/agent_configs.rs`

19. Every directory under `share/agents/` has a row in the installer's agent table, and every row's
    embedded contents equal the shipped file. A new agent cannot be added without the installer
    learning about it.

### By hand, on a real machine

The steps a checkout cannot fake, run once before merge and recorded here:

20. A real `install` on a machine with several agents, followed by a real turn producing a real
    glyph, with `git diff` in the dotfiles repo showing exactly the intended edits and nothing else.
21. The Claude Code plugin route end to end, against a throwaway `HOME`, confirming that
    `~/.claude/settings.json` still has no `hooks` entry of ours (004's promise).
22. A home-manager machine, both chains: a `mkOutOfStoreSymlink` `~/.tmux.conf` is edited in the
    dotfiles checkout and the symlinks survive; a genuinely store-resident file is refused and
    prints something the user can paste into their generator.
