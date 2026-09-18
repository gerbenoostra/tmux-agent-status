# Step 3: `src/install/write.rs`

## Scope

The safe-write primitive used by every file this tool touches:
`install::write::safely(path, new_contents)`.

## Relevant design

From [design.md](../design.md), "The safe write":

1. **Resolve.** `canonicalize` the path. A symlink chain resolves to its target and the edit lands
   there; the link itself is never replaced, because the rename in step 9 targets the resolved
   path. A path that does not exist canonicalizes its nearest existing ancestor instead, and the
   run continues in **create mode** (below).
2. **Refuse what must not be edited.** An existing target that is not a regular file (directory,
   fifo, socket, device): refuse. **The permission that matters is the parent directory's, not the
   file's** (see [findings.md](../findings.md)), so testing the file's mode would refuse edits that work and permit
   edits that do not. So: parent directory not writable, refuse with a message that names the
   resolved path and explains why. Target file itself read-only but its directory writable: warn
   and ask, because the mode is the user saying "not this one". Outside `$HOME`: warn and ask, since
   a dotfiles checkout may legitimately live elsewhere. More than one hard link: warn and ask,
   because the rename breaks the link and a dotfiles setup that hardlinks would silently diverge.
   `-y` proceeds on every warning and says so.
3. **Lock.** `O_CREAT | O_EXCL` on `<target>.tmux-agent-status.lock`, holding it across 4-13,
   removed on every exit path. A `SIGKILL` or a power cut runs no cleanup, so a stale lock is
   expected, not exceptional: the file therefore records pid, process start time and hostname, and
   is broken only when that exact process is provably gone. A lock from another host, or one whose
   liveness cannot be established, is reported rather than broken.
4. **Read and fingerprint.** Read the whole file. Record `(len, mtime, hash)`.
5. **Merge, and decide "already installed" semantically.** Build the new contents from the bytes
   read in step 4 with a pure function. The no-op test is **whether the parsed document already
   carries exactly our entries**, not whether our serialiser reproduces the file byte for byte
   (see [findings.md](../findings.md)). So: already carries our entries, stop here, **nothing written, no backup**.
   Only a genuine semantic change earns a write.
6. **Back up.** Copy to `<target>.bak-<UTC RFC3339 basic>` beside the target, preserving mode,
   `fsync`ed. Never deleted, never reused, never overwritten.
7. **Write a sibling.** `<target>.tmp-<pid>-<nonce>` in the target's own directory, so the rename in
   step 9 is same-filesystem and therefore atomic. Write, `fsync`, copy the target's mode.
8. **Re-check the fingerprint.** Stat the target again. Changed since step 4 - the agent wrote it, a
   dotfiles sync ran, another editor saved - then abort: delete the temp, keep the file untouched,
   fail the step with "the file changed while we were preparing the edit; nothing was written".
9. **Rename.** `rename(2)` the temp over the resolved target. A crash at any instant leaves either
   the old file or the new one, never a truncated one.
10. **Fsync the parent directory**, so the rename itself survives a power cut.
11. **Verify, and decide what a mismatch means.** Re-read the target and compare byte for byte
    against what step 7 wrote. A match is success. A mismatch has two possible causes:

    | What is on disk | Cause | Response |
    | --- | --- | --- |
    | empty, truncated, or unparseable in its own language | our write did not land intact | **restore the backup**, fail the step |
    | a complete, parseable document that is neither ours nor the backup | a writer that raced past step 8 | **restore nothing**, fail the step, print both paths |

12. **Verify semantically, by asking tmux** - for the two tmux steps only; see [Step 7](./07-install-probe-rs.md).
13. **Release the lock**, and name the backup path in the summary.

**Create mode**, for a target that does not exist. Same primitive, three differences: step 4 reads an
empty document rather than a file, step 6 writes no backup because there is nothing to lose, and any
missing parent directories are created first (`0755`, and only under a path the user confirmed).
Everything else - lock, temp file, rename, verify - is identical. The summary distinguishes
"created" from "edited".

## Relevant decisions

From [decisions.md](../decisions.md):

- **write mechanism**: temp file plus `rename(2)`, never truncate - the only way a crash cannot lose
  the file.
- **backup location**: beside the target, `.bak-<timestamp>`, never pruned - discoverable without
  knowing a state directory exists.
- **concurrency**: `O_EXCL` lock plus a fingerprint re-checked before the rename - the lock stops
  *our* races; the fingerprint is the only defence against a writer that takes no lock.
- **restore**: automatic on a failed verify, never a prompt - a broken config is the worst moment to
  block, and `-y` has nobody to ask.
- **generated configs**: refuse on writability of the resolved target, never on a path prefix.

## Relevant findings

From [findings.md](../findings.md):

- `rename(2)` over an `r--r--r--` file succeeds when its parent directory is writable. So the
  permission that matters for replacing a file is the directory's, not the file's mode.
- Reserialising an already-correct JSON file with `serde_json` (without `preserve_order`) changed
  2906 bytes into 3284 with no semantic change at all. This makes a byte-level "already installed"
  test rewrite a hand-maintained config that is already correct.
- Generated config detection via nix `mkOutOfStoreSymlink` vs store build product case study: the
  filesystem writability of the final target, not a path prefix, is the signal.

## Implementation

- Implement `install::write::safely(path, new_contents)` following the 13 steps above.
- Implement create mode for missing targets.
- Implement automatic restore from backup on truncated/unparseable post-write content; do not
  restore over a complete parseable document written by someone else.
- If restore itself fails, print the backup path and the literal `cp` command, exit 1.
- On every exit path remove the lock file and kill any probe server.
- Expose a test-only fault switch (`TMUX_AGENT_STATUS_TEST_FAULT=<stage>`) documented as unstable and
  unsupported so the verification suite can exercise every fallible syscall branch.

## Verification

8. **Symlink**: a chain two deep; the edit lands on the final target and the links are still links.
9. **Hard link**: detected, and the warning fires.
10. **Read-only parent**: clean failure, original intact, exit 1.
11. **Writability, both directions.** A chain ending in a genuine read-only store file: refused,
    with the generator fragment printed. A chain that *passes through* a read-only store and ends in
    a writable file: **edited normally** - the `mkOutOfStoreSymlink` case. Plus a read-only file in
    a writable directory, which `rename(2)` can replace and which must therefore warn and ask rather
    than refuse.
12. **Backup**: created, matches the pre-state byte for byte, named in the output.
13. **Fault injection** - the single most important test here. A test-only switch makes the write
    produce truncated, empty or scrambled bytes. The same switch names every other fallible syscall
    in the write - the backup copy, each `fsync`, the rename, the restore. Assert verify catches every
    one, the restore runs, and the file afterwards is **byte-identical to before the run**. Repeat
    with a fault in the restore itself: the exit is 1 and the message names the backup.
14. **The concurrent writer is not clobbered.** The counterpart to 13: a fault that replaces the
    target, after our rename, with a *complete and parseable* document that is neither ours nor the
    backup. Assert nothing is restored, the step fails, and the message names the file, the backup
    and the temp file. Then assert the damaged variants of the same test still *do* restore, so the
    two rows of step 11 are both pinned.
15. **Create mode**: a target that does not exist, with a missing parent directory. Assert the
    directory and file are created, no backup is written, the summary says "created", and a second
    run reports already installed.
16. **Concurrency**: N processes installing into the same file at once. Afterwards the file is valid
    in its own language and carries exactly one copy of our entries; every loser reported a clean
    precondition or lock failure and wrote nothing.
17. **Idempotency**: two runs. The second writes no file, creates no backup, and reports every step
    as already installed. Asserted on file mtimes, not just on output.
