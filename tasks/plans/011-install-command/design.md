# 011 - design

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
   file's** (see [findings.md](./findings.md)), so testing the file's mode would refuse edits that work and permit
   edits that do not. So: parent directory not writable, refuse with a message that names the
   resolved path and explains why. Target file itself read-only but its directory writable: warn and ask,
   because the mode is the user saying "not this one". Outside `$HOME`: warn and ask, since a dotfiles
   checkout may legitimately live elsewhere. More than one hard link: warn and ask, because the rename
   breaks the link and a dotfiles setup that hardlinks would silently diverge. `-y` proceeds on
   every warning and says so.
3. **Lock.** `O_CREAT | O_EXCL` on `<target>.tmux-agent-status.lock`, holding it across 4-13,
   removed on every exit path. A `SIGKILL` or a power cut runs no cleanup, so a stale lock is
   expected, not exceptional: the file therefore records pid, process start time and hostname, and
   is broken only when that exact process is provably gone - a pid alone is reused, and "older than
   60 seconds" alone breaks a live lock held by a slow filesystem. A lock from another host, or one
   whose liveness cannot be established, is reported rather than broken. This serialises *our*
   concurrent runs. It does **not** serialise the agent that also writes the file - nothing can,
   since the agent takes no lock - which is what steps 8 and 11 are for.
4. **Read and fingerprint.** Read the whole file. Record `(len, mtime, hash)`.
5. **Merge, and decide "already installed" semantically.** Build the new contents from the bytes
   read in step 4 with a pure function. The no-op test is **whether the parsed document already
   carries exactly our entries**, not whether our serialiser reproduces the file byte for byte (see
   [findings.md](./findings.md)). A byte-level test would rewrite that file, take a pointless backup and put a
   large no-op diff in the user's dotfiles repo. So: already carries our entries, stop here,
   **nothing written, no backup**. Only a genuine semantic change earns a write, and when one is
   earned the reformatting it brings is disclosed in the confirmation.
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
    our homework". Two questions: nothing we are not responsible for moved, *and* what the edit was
    for arrived - the option carries the term, the hooks are registered. A failure here rolls the
    file back, and that rollback is unconditionally safe because step 11 just proved the bytes on
    disk are ours and nobody else's.
13. **Release the lock**, and name the backup path in the summary.

**Create mode**, for a target that does not exist - the new `~/.config/tmux/tmux.conf`, the written
snippet, an agent's first drop-in. Same primitive, three differences: step 4 reads an empty document
rather than a file, step 6 writes no backup because there is nothing to lose, and any missing parent
directories are created first (`0755`, and only under a path the user confirmed). Everything else -
lock, temp file, rename, verify - is identical, so a create still cannot half-land, and a target
that appears between step 1 and step 9 is caught by step 8 like any other concurrent write. The
summary distinguishes "created" from "edited", because they are different things to want to undo.

### Restore on failed verify

A verify that finds an empty, truncated or unparseable file restores automatically from the backup,
then reports failure while naming the backup. A verify that finds a complete document written by
someone else does not restore; it fails and names the file, the backup and the temp so a human can
reconcile. See [decisions.md](./decisions.md) for why a prompt was rejected.

### Generated configs

A target whose resolved path is not writable, or whose parent is not writable enough for a temp
file and atomic rename, is refused. The refusal message names the resolved path and explains why.
See [findings.md](./findings.md) for the nix-store case study and [decisions.md](./decisions.md) for why writability is the test.

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
