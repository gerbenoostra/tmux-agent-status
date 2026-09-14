# 011 - decisions

## What changes about the rules

Three rules in [AGENTS.md](../../AGENTS.md) and one decision in 001 are **superseded here**, deliberately and with
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

[AGENTS.md](../../AGENTS.md) forbids this, and 001 explains why: writing a spliced format back through
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

## Restore on failure

The request asked to *"ask the user if it should be restored"* when a write fails. This plan
deviates from that request, and the deviation is signed off: asked directly, the answer was "good to
have auto restore".

A write that did not land intact leaves the file in a state nobody intended, and that is the worst
possible moment to block on a question. In `-y` there is nobody to ask, and in an interactive run
the honest answer to "shall I put your config back?" is always yes. So for that case the tool
restores automatically, then says so loudly, naming the backup either way.

The automatic restore is scoped to exactly the failed-verify case where the file is empty,
truncated or unparseable. Where the file on disk is a complete document somebody else wrote, nothing
is restored; the tool cannot prove whose write is more valuable, so it stops, keeps both, and names
the file, the backup and the temp.

If the restore itself fails, the tool prints the backup path and the literal `cp` command that
completes it, and exits 1.

## Generated configs: writability, not path prefix

The test for whether a config is generated is whether the resolved target and its parent directory
are writable enough to create a temp file and rename over it. A path prefix such as `/nix/store` is
not the test: a chain created with `mkOutOfStoreSymlink` passes through the store and ends in a real,
writable, git-tracked file (see [findings.md](./findings.md)). The store prefix is used only in the refusal wording
when the final target really is unwritable and inside a package store.

When the resolved target sits inside a git repository other than the one we are running in, the
confirmation says so, so nobody is surprised to find an uncommitted change in a repo they were not
thinking about.

## `--dry-run` runs read-only probes

"Run nothing" was the first draft and it is not implementable: detection *is* running things.
`--dry-run` cannot show a real plan without asking `tmux` which config files it would load and
`claude plugin list --json` what is already installed, and a dry run that prints a guess is worse
than one that prints the truth.

So the compromise is: `--dry-run` performs no write, no rename, no directory creation, and no
state-changing external command, but it does run the small fixed set of read-only probes listed in
[spec.md](./spec.md).

## Agent hook delivery

### Plugin route does not fall back on failure

The delivery order is plugin > own drop-in file > inline merge. The routes are chosen by what is
available, not by what worked.

- `claude` absent from `PATH`: fall to the merge into `~/.claude/settings.json`; the plugin route was
  never available.
- `claude` present, plugin already installed: stop. Do not check where the marketplace points; see
  "Never repoint an existing marketplace".
- `claude` present, plugin commands succeed: plugin installed; `settings.json` untouched.
- `claude` present, plugin commands fail (old CLI, no network, marketplace down): fail the step,
  print the command and its stderr verbatim, suggest `--claude-route=settings`.

Falling back on failure would edit `~/.claude/settings.json` on a machine where the user was promised
it would not be, as the silent consequence of a network blip. A user who wants that outcome can ask
for it with `--claude-route=settings`; nobody gets it by accident.

### Never repoint an existing marketplace

Idempotency keys on the plugin being installed - an id starting `tmux-agent-status@` in
`claude plugin list --json` - and stops there. The tool must not check where the marketplace points
or re-run `marketplace add` to "correct" it.

Verified as a real setup: a contributor's marketplace is registered as a `directory` source pointing
at their local checkout, which is how they test the plugin they are developing. Re-adding it from
GitHub would silently swap their working copy for a released one, and the symptom - a plugin that no
longer reflects their edits - is maddening to trace back to an installer they ran once. If the plugin
is installed, the step is done.

### Repository slug

The default marketplace source is derived from `env!("CARGO_PKG_REPOSITORY")` so a fork gets its own
slug by changing the manifest it was going to change anyway. `--marketplace <source>` overrides it,
which is also how a developer points the installer at a local checkout.

### Drop-in contents

Drop-in contents are embedded in the binary with `include_str!`, not read from `share/agents/` at
runtime. `cargo install` ships the binary and nothing else, so a runtime path lookup would leave the
largest install route unable to install anything. Embedding also removes a class of "found the wrong
copy" bugs and keeps `tests/agent_configs.rs`'s drift checking meaningful, because the embedded bytes
are the shipped files.

### Merge idempotency

Two levels, not one:

1. Marker comments identify the block the tool manages. Only a marked block is ours to rewrite or,
   later, to remove.
2. Our own `name = "tmux-agent-status-..."` keys identify the hook set being present, marked or not.

Level 2 catches a user who copied the shipped drop-in by hand before this subcommand existed - the
file is byte-identical to the shipped file and carries no markers. Marker-only idempotency would
append a second copy of every hook to that file, so the append is gated on level 2. If an equivalent
set is already there unmarked, the tool reports "already installed, not in a block we manage" and
offers to adopt it - rewrap the existing entries in markers, changing no behaviour - rather than
duplicating it. Declining leaves the file untouched and the step reports success, because the hooks
are installed.

## Reporting managed and unmanaged paths

The summary groups every edit by its resolved destination: which edits landed in a git repository and
which landed in unmanaged `$HOME`. Refusing to write unmanaged paths would make the tool useless on
the machines it is most for, and guessing which dotfiles manager should adopt a file is not something
a status-glyph installer has any business doing.

## tmux config discovery

`tmux display-message -p '#{config_files}'` is a source of candidates, not a decision, because it
lists non-existent files and is constrained to `-f` when that was used. The chosen resolution order
is:

1. `--tmux-config <path>`, which overrides everything below
2. `$XDG_CONFIG_HOME/tmux/tmux.conf` if it exists
3. `~/.config/tmux/tmux.conf` if it exists
4. `~/.tmux.conf` if it exists
5. none exist: offer to create `~/.config/tmux/tmux.conf`

`/etc/tmux.conf` is never chosen, even when it is the only one that exists and even under `-y`. It
needs root and installs the tool for every user of the machine, which is not what anyone typing this
command meant. It is reported with the suggestion to pass `--tmux-config` if that really was the
intent.

With no tmux on `PATH` or no running server, every tmux invocation is optional and its failure is
not an error. Discovery falls to the file-existence order, the format default falls to its
hard-coded value, the reload offer is not made, and the summary says plainly which checks could not
be run against a live tmux.

## Implementation decisions

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
| coverage | `src/install/` holds the repo's 100% line and region bar, reached by widening the test fault switch to every fallible syscall | the gate predates this plan and applies to the whole crate; lowering it for the one subcommand that writes user files is the wrong place to start making exceptions |
| the TTY module | `src/install/prompt.rs` is the one file `just coverage` skips, by filename | a prompt needs a terminal to exercise, and the alternative is a pty harness that proves `dialoguer` works rather than that we do; keeping the exception to one named file is what makes it reviewable |

## Format term

### Idempotency

If the value already references `@agent_status`, leave it exactly as it is. A reference is a token
match (`@agent_status` not followed by `[A-Za-z0-9_]`), not a substring match: a user's unrelated
`#{@agent_status_colour}` must not read as ours. Matching the full literal term instead would miss
anyone who dropped the space or wrapped the term in styling, and then install a second copy.

### Which line to edit

tmux executes config commands strictly in the order it encounters them, including inside
`source-file`; the last assignment wins. So the search reconstructs tmux's own command order: walk
the config from the top, descend into each `source-file` at the point it appears, depth-limited with
a visited set against a cycle, and collect every matching line in that order. The last one is the
one to edit. Editing an earlier one produces a line tmux discards (see [findings.md](./findings.md)).

### Quoting

The term is inserted before the first `#{?window_flags` or at the end of the value. A bare value must
be requoted, always: the term contains a space, and a bare value ends at the first space. Single
quotes are always safe because the term contains none; a value that already contains a single quote
is re-emitted double-quoted, and a value that defeats both falls to the manual path.

### No existing line

When there is no existing format line, read tmux's compiled-in default by starting a throwaway
server with `-f /dev/null` and asking `show-options -gwv`. That default goes through the same splice
and the step writes a new pair of lines - both options, because a term in only one of them makes the
glyph vanish when the window becomes current.

### Manual fallback

If the parser cannot requote safely or the user declines the edit, the step does not fail and nothing
is written. The tool prints the term, the file and line, and the proposed line where it got far enough
to build one. The summary marks the format step as not installed, with instructions, and the run
exits 0.

### Reloading

Never `set-option`. After a successful format or hook step, offer to run `tmux source-file <config>`
if a server is running, confirmed like everything else. Agents still need their own restart.

## Verification by tmux probe

After the byte-level verify, start a throwaway tmux on the edited file and ask it what the format
actually is. Compare the full option dump against a baseline taken before the edit. The only allowed
differences are the intended format values and, for the hook step, the two hooks. This catches the
abandoned-config case, where tmux still answers `show-options -gwv` with the default. The write lands
first, then the probe runs; a failure rolls back through the backup.

The probe runs with `$TMUX` cleared, a unique socket name, cwd `$HOME`, and is killed on every exit
path. It is bounded by a timeout and can be skipped with `--no-tmux-probe`.


## The coverage gate reads the merged region view

The bar stays "every region of `src/` is reached". What changed is where the bar is read from:
`cargo llvm-cov --fail-under-regions 100` disagrees with `llvm-cov show` about this crate, and the
disagreement is an artefact of compiling `src/` twice (see [findings.md](./findings.md)). Chasing it
with tests would be chasing an aggregation bug, and lowering the bar would stop catching real region
gaps, so the recipe gates on `llvm-cov`'s exported segments instead - the same view `show` renders,
which answers the question consistently.

A line that genuinely cannot be reached excuses itself with a trailing `// coverage: off` and the
reason. The marker is ours because nothing else is available: `llvm-cov` has no line-level exclusion
at all, and `#[coverage(off)]` is unstable on the toolchain this crate builds with.

Five regions carry the marker: the four `?` arms in `probe.rs` that need a tmux which answers one
call and not the next (or a process with no `$HOME`), and the `matches!` arm in one `agents.rs`
assertion that only a failing run reaches. The alternative for the `probe.rs` four was to widen the
test-fault switch in production code, which buys less than it costs.
