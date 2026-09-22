//! The safe write, against a real filesystem.
//!
//! This is the module `register` exists to get right, so these are the tests
//! that matter most: every one of them is a way a config file could be lost,
//! and the assertion is always the same - afterwards the user's file is either
//! the one they had or the one they asked for, and never anything in between.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use tmux_agent_status::register::write::{
    self, Ask, Error, Faults, Outcome, Plan, SafeWrite, Verify, Warning, Written,
};

mod support;

use support::tempdir::TempDir;

/// A caller that answers every warning the same way, and records what it saw.
struct Answer {
    yes: bool,
    seen: std::sync::Mutex<Vec<Warning>>,
}

impl Answer {
    fn yes() -> Answer {
        Answer {
            yes: true,
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn no() -> Answer {
        Answer {
            yes: false,
            ..Answer::yes()
        }
    }

    fn saw(&self) -> Vec<Warning> {
        self.seen.lock().expect("the lock is not poisoned").clone()
    }
}

impl Ask for Answer {
    fn warn(&self, warning: &Warning) -> bool {
        self.seen
            .lock()
            .expect("the lock is not poisoned")
            .push(warning.clone());
        self.yes
    }
}

/// Every temp directory is outside `$HOME`, so that warning is always asked and
/// always answered; the tests are about the other ones.
fn outside_home_only(seen: &[Warning]) -> bool {
    seen.iter()
        .all(|warning| matches!(warning, Warning::OutsideHome(_)))
}

/// A stand-in for a real language check, with the properties the real ones
/// have: a truncation leaves a document that does not end where a document
/// ends, and scrambled bytes are not a document at all.
fn parses(text: &str) -> bool {
    text.ends_with('\n') && !text.contains('\0')
}

/// Nothing is ever a complete document, so every mismatch reads as damage.
fn never_parses(_: &str) -> bool {
    false
}

fn writer<'a>(path: &'a Path, ask: &'a dyn Ask) -> SafeWrite<'a> {
    SafeWrite {
        path,
        ask,
        parses,
        verify: None,
        faults: Faults::default(),
    }
}

fn put(path: &Path, contents: &str) -> Result<Written, Error> {
    let ask = Answer::yes();
    writer(path, &ask).apply(|_| Plan::Write(contents.to_owned()))
}

fn backups(dir: &TempDir) -> Vec<String> {
    dir.entries()
        .into_iter()
        .filter(|name| name.contains(".bak-"))
        .collect()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("the file can be read")
}

#[test]
fn an_edit_replaces_the_contents_and_leaves_a_backup() {
    let dir = TempDir::new("edit");
    let target = dir.write("tmux.conf", "before\n");

    let written = put(&target, "after\n").expect("the write lands");

    assert_eq!(written.outcome, Outcome::Edited);
    assert_eq!(written.resolved, target);
    assert_eq!(read(&target), "after\n");

    // 12. The backup matches the pre-state byte for byte and is named in the
    // result, because a backup nobody can find is not a backup.
    let backup = written.backup.expect("an edit takes a backup");
    assert_eq!(read(&backup), "before\n");
    assert_eq!(backups(&dir).len(), 1);
}

#[test]
fn the_lock_and_temp_files_are_gone_afterwards() {
    let dir = TempDir::new("residue");
    let target = dir.write("tmux.conf", "before\n");
    put(&target, "after\n").expect("the write lands");

    let residue: Vec<String> = dir
        .entries()
        .into_iter()
        .filter(|name| name.contains(".tmp-") || name.contains(".lock"))
        .collect();
    assert!(residue.is_empty(), "left behind: {residue:?}");
}

// 8. A symlink chain two deep: the edit lands on the final target and the links
// are still links.
#[test]
fn a_symlink_chain_is_followed_and_survives() {
    let dir = TempDir::new("symlink");
    let real = dir.write("dotfiles/tmux.conf", "before\n");
    let middle = dir.join("middle.conf");
    let front = dir.join("front.conf");
    std::os::unix::fs::symlink(&real, &middle).expect("the first link");
    std::os::unix::fs::symlink(&middle, &front).expect("the second link");

    let written = put(&front, "after\n").expect("the write lands");

    assert_eq!(written.resolved, real, "the edit must land on the target");
    assert_eq!(read(&real), "after\n");
    for link in [&middle, &front] {
        assert!(
            fs::symlink_metadata(link)
                .expect("the link is there")
                .is_symlink(),
            "{} stopped being a symlink",
            link.display()
        );
    }
    // The backup belongs beside what was edited, not beside what was named.
    let backup = written.backup.expect("a backup");
    assert_eq!(backup.parent(), real.parent());
}

// 9. A hard link is detected and the warning fires.
#[test]
fn a_hard_link_is_warned_about() {
    let dir = TempDir::new("hardlink");
    let target = dir.write("tmux.conf", "before\n");
    let other = dir.join("other.conf");
    fs::hard_link(&target, &other).expect("the hard link");

    let ask = Answer::yes();
    writer(&target, &ask)
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect("the write lands once the warning is answered");

    assert!(
        ask.saw()
            .iter()
            .any(|warning| matches!(warning, Warning::HardLinked { links, .. } if *links == 2)),
        "the hard link was not warned about: {:?}",
        ask.saw()
    );
    // The warning is true: the other name keeps the old contents.
    assert_eq!(read(&target), "after\n");
    assert_eq!(read(&other), "before\n");
}

#[test]
fn declining_a_warning_writes_nothing() {
    let dir = TempDir::new("declined");
    let target = dir.write("tmux.conf", "before\n");

    let ask = Answer::no();
    let error = writer(&target, &ask)
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("a declined warning refuses the write");

    assert!(matches!(error, Error::Declined(_)), "{error}");
    assert_eq!(read(&target), "before\n");
    assert!(backups(&dir).is_empty(), "a refusal takes no backup");
}

// 10. A read-only parent directory fails cleanly with the original intact.
#[test]
fn a_read_only_directory_is_refused() {
    let dir = TempDir::new("readonly-dir");
    let inner = dir.join("locked");
    fs::create_dir(&inner).expect("the directory");
    let target = inner.join("tmux.conf");
    fs::write(&target, "before\n").expect("the file");
    fs::set_permissions(&inner, fs::Permissions::from_mode(0o555)).expect("chmod");

    let ask = Answer::yes();
    let error = writer(&target, &ask)
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("an unwritable directory is refused");

    assert!(matches!(error, Error::Unwritable { .. }), "{error}");
    assert!(
        error.to_string().contains(&target.display().to_string()),
        "the message must name the resolved path: {error}"
    );
    assert_eq!(read(&target), "before\n");
}

// 11. Writability, both directions. The chain that passes through a read-only
// directory and ends in a writable file is the `mkOutOfStoreSymlink` case, and
// a draft of the plan refused it on a path prefix.
#[test]
fn a_chain_through_a_read_only_directory_ending_in_a_writable_file_is_edited() {
    let dir = TempDir::new("through-store");
    let store = dir.join("store");
    let checkout = dir.write("dotfiles/tmux.conf", "before\n");
    fs::create_dir(&store).expect("the store directory");
    let through = store.join("hm_tmux.conf");
    std::os::unix::fs::symlink(&checkout, &through).expect("the link into the checkout");
    let front = dir.join("tmux.conf");
    std::os::unix::fs::symlink(&through, &front).expect("the link into the store");
    fs::set_permissions(&store, fs::Permissions::from_mode(0o555)).expect("chmod the store");

    let written = put(&front, "after\n").expect("the chain ends somewhere writable");

    assert_eq!(written.resolved, checkout);
    assert_eq!(read(&checkout), "after\n");
}

// 11, the other half: a read-only file in a writable directory. `rename(2)` can
// replace it, so it must warn and ask rather than refuse.
#[test]
fn a_read_only_file_in_a_writable_directory_warns_and_asks() {
    let dir = TempDir::new("readonly-file");
    let target = dir.write("tmux.conf", "before\n");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o444)).expect("chmod");

    let ask = Answer::yes();
    writer(&target, &ask)
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect("rename can replace a read-only file in a writable directory");

    assert!(
        ask.saw()
            .iter()
            .any(|warning| matches!(warning, Warning::ReadOnlyFile(_))),
        "the mode was not warned about: {:?}",
        ask.saw()
    );
    assert_eq!(read(&target), "after\n");
    // The mode was the user saying something, so the new file keeps it.
    let mode = fs::metadata(&target)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o444, "the mode was not carried over");
}

#[test]
fn a_target_that_is_not_a_regular_file_is_refused() {
    let dir = TempDir::new("not-a-file");
    let target = dir.join("a-directory");
    fs::create_dir(&target).expect("the directory");

    let ask = Answer::yes();
    let error = writer(&target, &ask)
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("a directory is not a file to edit");

    assert!(matches!(error, Error::NotARegularFile(_)), "{error}");
}

// 15. Create mode, with a missing parent directory.
#[test]
fn create_mode_makes_the_directory_and_takes_no_backup() {
    let dir = TempDir::new("create");
    let target = dir.join("config/tmux/tmux.conf");

    let written = put(&target, "fresh\n").expect("the file is created");

    assert_eq!(written.outcome, Outcome::Created);
    assert_eq!(written.backup, None, "there was nothing to lose");
    assert_eq!(read(&target), "fresh\n");
    assert_eq!(written.resolved, target);
}

// 17. Idempotency, asserted on mtimes rather than only on output.
#[test]
fn a_second_run_writes_nothing_and_takes_no_backup() {
    let dir = TempDir::new("idempotent");
    let target = dir.write("tmux.conf", "before\n");
    put(&target, "after\n").expect("the first run writes");
    let after_first = fs::metadata(&target).expect("metadata").mtime_nsec();
    let backups_after_first = backups(&dir).len();

    let ask = Answer::yes();
    let written = writer(&target, &ask)
        .apply(|current| match current == "after\n" {
            true => Plan::AlreadyRegistered,
            false => Plan::Write("after\n".to_owned()),
        })
        .expect("the second run succeeds");

    assert_eq!(written.outcome, Outcome::AlreadyRegistered);
    assert_eq!(written.backup, None);
    assert_eq!(
        fs::metadata(&target).expect("metadata").mtime_nsec(),
        after_first,
        "an already-registered file must not be rewritten"
    );
    assert_eq!(backups(&dir).len(), backups_after_first, "no new backup");
}

// 13. Fault injection, the single most important test here.
#[test]
fn a_write_that_does_not_land_is_restored_byte_for_byte() {
    for stage in ["truncate", "empty", "scramble"] {
        let dir = TempDir::new("damaged");
        let target = dir.write("tmux.conf", "a config the user wrote\nand a second line\n");
        let before = read(&target);
        let ask = Answer::yes();

        let error = SafeWrite {
            faults: Faults::parse(stage),
            ..writer(&target, &ask)
        }
        .apply(|_| Plan::Write("what we meant to write\n".to_owned()))
        .expect_err("a damaged write must fail");

        match &error {
            Error::Damaged {
                backup, restore, ..
            } => {
                assert!(restore.is_none(), "the restore itself failed: {error}");
                let backup = backup.as_ref().expect("the backup is named");
                assert!(
                    error.to_string().contains(&backup.display().to_string()),
                    "the message must name the backup: {error}"
                );
            }
            other => panic!("{stage}: expected damage, got {other}"),
        }
        assert_eq!(read(&target), before, "{stage}: the file was not restored");
    }
}

// 13, continued: a fault in the restore itself. Two filesystem failures in a
// row, and the only path that leaves a user work to do.
#[test]
fn a_restore_that_fails_names_the_backup_and_the_command() {
    let dir = TempDir::new("restore-fails");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();

    let error = SafeWrite {
        faults: Faults::parse("empty,restore"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("after\n".to_owned()))
    .expect_err("a damaged write must fail");

    let message = error.to_string();
    assert!(
        matches!(
            &error,
            Error::Damaged {
                restore: Some(_),
                ..
            }
        ),
        "{error}"
    );
    assert!(
        message.contains("cp "),
        "no command to finish it by hand: {message}"
    );
    assert!(
        message.contains(".bak-"),
        "the backup is not named: {message}"
    );
}

// 14. The counterpart, and the case a blanket restore gets wrong.
#[test]
fn a_write_cut_short_is_restored_even_when_the_remains_look_complete() {
    // A tmux config has no notion of being truncated - every prefix of a valid
    // one is also valid - so the language check cannot tell this from somebody
    // else's document. Being a prefix of what we meant to write can.
    let dir = TempDir::new("prefix");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();

    let error = SafeWrite {
        // Anything at all is a complete document, which is the worst case.
        parses: |_| true,
        faults: Faults::parse("truncate"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("a much longer line than the original\n".to_owned()))
    .expect_err("a truncated write must fail");

    assert!(matches!(error, Error::Damaged { .. }), "{error}");
    assert_eq!(read(&target), "before\n", "the file was not restored");
}

#[test]
fn a_concurrent_writers_document_is_never_clobbered() {
    let dir = TempDir::new("raced");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();

    let error = SafeWrite {
        faults: Faults::parse("verify-race"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("ours\n".to_owned()))
    .expect_err("a raced write must fail");

    let Error::Raced { backup, temp, .. } = &error else {
        panic!("expected a race, got {error}");
    };
    assert_ne!(
        read(&target),
        "before\n",
        "the other writer's document was overwritten with the backup"
    );
    let message = error.to_string();
    for named in [
        target.display().to_string(),
        backup.as_ref().expect("the backup").display().to_string(),
        temp.as_ref().expect("the temp file").display().to_string(),
    ] {
        assert!(
            message.contains(&named),
            "{named} is not named in: {message}"
        );
    }
    let temp = temp.as_ref().expect("the temp file");
    assert!(
        temp.exists(),
        "what we meant to write must still be on disk"
    );
    assert_eq!(read(temp), "ours\n");
}

#[test]
fn a_race_reports_the_two_files_it_has_when_it_cannot_keep_the_third() {
    let dir = TempDir::new("raced-unkept");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();

    let error = SafeWrite {
        faults: Faults::parse("verify-race,keep"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("ours\n".to_owned()))
    .expect_err("a raced write must fail");

    assert!(matches!(error, Error::Raced { temp: None, .. }), "{error}");
    // The two that matter are still named, and neither was touched.
    let message = error.to_string();
    assert!(message.contains(&target.display().to_string()), "{message}");
    assert!(message.contains(".bak-"), "{message}");
}

// 14, continued: the damaged variant of the same shape still restores, so both
// rows of the plan's step 11 table are pinned by a test.
#[test]
fn a_document_that_does_not_parse_is_restored_rather_than_kept() {
    let dir = TempDir::new("raced-damaged");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();

    let error = SafeWrite {
        parses: never_parses,
        faults: Faults::parse("verify-race"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("ours\n".to_owned()))
    .expect_err("the write must fail");

    assert!(matches!(error, Error::Damaged { .. }), "{error}");
    assert_eq!(read(&target), "before\n", "the backup did not go back");
}

#[test]
fn a_file_that_changes_before_the_rename_is_left_alone() {
    let dir = TempDir::new("changed");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();

    let error = SafeWrite {
        faults: Faults::parse("changed"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("ours\n".to_owned()))
    .expect_err("a file that moved under us must not be written");

    assert!(matches!(error, Error::Changed(_)), "{error}");
    assert_eq!(read(&target), "written by somebody else\n");
    let residue: Vec<String> = dir
        .entries()
        .into_iter()
        .filter(|name| name.contains(".tmp-"))
        .collect();
    assert!(
        residue.is_empty(),
        "the temp file was left behind: {residue:?}"
    );
}

#[test]
fn a_failure_before_the_rename_leaves_the_file_untouched() {
    // Every syscall the write makes before the rename, each one refusing in
    // turn. The assertion is always the same: the user's file is exactly the
    // one they had.
    for stage in [
        "lstat",
        "read",
        "stat",
        "backup",
        "fsync",
        "create",
        "write",
        "fsync-temp",
        "stat-again",
        "rename",
    ] {
        let dir = TempDir::new("early-failure");
        let target = dir.write("tmux.conf", "before\n");
        let ask = Answer::yes();

        let error = SafeWrite {
            faults: Faults::parse(stage),
            ..writer(&target, &ask)
        }
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("{stage} must fail the write");

        assert!(matches!(error, Error::Io { .. }), "{stage}: {error}");
        assert_eq!(read(&target), "before\n", "{stage}: the file changed");
        let residue: Vec<String> = dir
            .entries()
            .into_iter()
            .filter(|name| name.contains(".tmp-") || name.contains(".lock"))
            .collect();
        assert!(residue.is_empty(), "{stage}: left behind {residue:?}");
    }
}

#[test]
fn a_failure_after_the_rename_is_still_a_failure_of_the_step() {
    // The bytes are on disk by now, so these are the syscalls whose failure
    // the user hears about without losing anything.
    for stage in ["fsync-dir", "read"] {
        let dir = TempDir::new("late-failure");
        let target = dir.write("tmux.conf", "before\n");
        let ask = Answer::yes();

        let error = SafeWrite {
            // `read` fires first at step 4, so this pins the earlier site; the
            // later one is reached by the same stage in the loop above.
            faults: Faults::parse(stage),
            ..writer(&target, &ask)
        }
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("the write must fail");

        assert!(matches!(error, Error::Io { .. }), "{stage}: {error}");
    }
}

#[test]
fn create_mode_reports_a_directory_it_cannot_make() {
    let dir = TempDir::new("mkdir-fails");
    let target = dir.join("config/tmux/tmux.conf");
    let ask = Answer::yes();

    let error = SafeWrite {
        faults: Faults::parse("mkdir"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("fresh\n".to_owned()))
    .expect_err("the write must fail");

    assert!(matches!(error, Error::Io { .. }), "{error}");
    assert!(error.to_string().contains("mkdir"), "{error}");
    assert!(!target.exists());
}

#[test]
fn a_stale_lock_that_cannot_be_removed_is_reported() {
    let dir = TempDir::new("unlock-fails");
    let target = dir.write("tmux.conf", "before\n");
    fs::write(
        dir.join("tmux.conf.tmux-agent-status.lock"),
        format!("{}\t{}\tgone\n", u32::MAX - 1, hostname()),
    )
    .expect("the lock file");
    let ask = Answer::yes();

    let error = SafeWrite {
        faults: Faults::parse("unlock"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("after\n".to_owned()))
    .expect_err("the write must fail");

    assert!(matches!(error, Error::Io { .. }), "{error}");
    assert_eq!(read(&target), "before\n");
}

#[test]
fn create_mode_reports_a_directory_it_cannot_sync() {
    // The parent fsync is its own syscall, and in create mode nothing before
    // it can fail first.
    let dir = TempDir::new("fsync-dir-create");
    let target = dir.join("tmux.conf");
    let ask = Answer::yes();

    let error = SafeWrite {
        faults: Faults::parse("fsync-dir"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("fresh\n".to_owned()))
    .expect_err("the write must fail");

    assert!(error.to_string().contains("fsync-dir"), "{error}");
}

/// A semantic check that always says no, which is what a tmux probe does when
/// the edit produced a config tmux abandons.
struct Rejects;

impl Verify for Rejects {
    fn verify(&self, _: &Path) -> Result<(), String> {
        Err("tmux abandoned the config".to_owned())
    }
}

#[test]
fn a_rejected_edit_is_rolled_back() {
    let dir = TempDir::new("rejected");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();
    let rejects = Rejects;

    let error = SafeWrite {
        verify: Some(&rejects),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("after\n".to_owned()))
    .expect_err("a rejected edit must fail");

    let Error::Rejected { restored, .. } = &error else {
        panic!("expected a rejection, got {error}");
    };
    assert!(restored, "the rollback did not run: {error}");
    assert_eq!(read(&target), "before\n");
    assert!(
        error.to_string().contains("tmux abandoned the config"),
        "the reason is not reported: {error}"
    );
}

#[test]
fn a_rejected_creation_removes_the_file_it_created() {
    let dir = TempDir::new("rejected-create");
    let target = dir.join("tmux.conf");
    let ask = Answer::yes();
    let rejects = Rejects;

    let error = SafeWrite {
        verify: Some(&rejects),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("after\n".to_owned()))
    .expect_err("a rejected creation must fail");

    assert!(
        !target.exists(),
        "the created file was left behind: {error}"
    );
    assert!(error.to_string().contains("removed"), "{error}");
}

#[test]
fn a_rollback_that_fails_says_what_to_do_by_hand() {
    let dir = TempDir::new("rejected-restore-fails");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();
    let rejects = Rejects;

    let error = SafeWrite {
        verify: Some(&rejects),
        faults: Faults::parse("restore"),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("after\n".to_owned()))
    .expect_err("a rejected edit must fail");

    assert!(
        matches!(
            error,
            Error::Rejected {
                restored: false,
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("cp "), "{error}");

    // The create-mode half of the same failure: no backup, so the advice is a
    // removal rather than a copy.
    let fresh = dir.join("fresh.conf");
    let error = SafeWrite {
        verify: Some(&rejects),
        faults: Faults::parse("restore"),
        ..writer(&fresh, &ask)
    }
    .apply(|_| Plan::Write("after\n".to_owned()))
    .expect_err("a rejected creation must fail");
    assert!(error.to_string().contains("rm "), "{error}");
}

#[test]
fn a_lock_held_by_a_live_process_stops_a_second_run() {
    let dir = TempDir::new("locked");
    let target = dir.write("tmux.conf", "before\n");
    let lock = dir.join("tmux.conf.tmux-agent-status.lock");
    // This process's own pid, which `ps` will confirm is alive.
    let identity = live_identity();
    fs::write(&lock, &identity).expect("the lock file");

    let ask = Answer::yes();
    let error = writer(&target, &ask)
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("a held lock stops the write");

    assert!(matches!(error, Error::Locked { .. }), "{error}");
    assert_eq!(read(&target), "before\n");
    assert!(lock.exists(), "a live lock must not be removed");
}

#[test]
fn a_stale_lock_is_broken_and_a_lock_from_elsewhere_is_not() {
    let dir = TempDir::new("stale-lock");
    let target = dir.write("tmux.conf", "before\n");
    let lock = dir.join("tmux.conf.tmux-agent-status.lock");

    // A pid nothing can be running under, on this host: provably gone.
    fs::write(&lock, format!("{}\tthis-host\tgone\n", u32::MAX - 1)).expect("the lock file");
    let host = hostname();
    fs::write(&lock, format!("{}\t{host}\tgone\n", u32::MAX - 1)).expect("the lock file");
    put(&target, "after\n").expect("a stale lock is broken");
    assert_eq!(read(&target), "after\n");

    // A lock from another host cannot be proved dead from here.
    fs::write(&lock, format!("{}\tsome-other-host\tgone\n", u32::MAX - 1)).expect("the lock file");
    let ask = Answer::yes();
    let error = writer(&target, &ask)
        .apply(|_| Plan::Write("later\n".to_owned()))
        .expect_err("a lock from elsewhere is reported");
    assert!(matches!(error, Error::Locked { .. }), "{error}");
    assert!(error.to_string().contains("some-other-host"), "{error}");

    // So is one whose contents mean nothing.
    fs::write(&lock, "nonsense\n").expect("the lock file");
    let error = writer(&target, &ask)
        .apply(|_| Plan::Write("later\n".to_owned()))
        .expect_err("an unreadable lock is reported");
    assert!(matches!(error, Error::Locked { .. }), "{error}");
}

// 16. N processes registering into the same file at once.
#[test]
fn concurrent_runs_produce_exactly_one_copy_of_our_entries() {
    let dir = TempDir::new("concurrent");
    let target = dir.write("hooks.json", "entries: 0\n");
    let path = target.clone();

    let results: Vec<Result<Written, Error>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                scope.spawn(move || {
                    let ask = Answer::yes();
                    writer(&path, &ask).apply(|current| match current.contains("entries: 1") {
                        true => Plan::AlreadyRegistered,
                        false => Plan::Write("entries: 1\n".to_owned()),
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("no thread panicked"))
            .collect()
    });

    // Whatever the interleaving, the file is valid and carries our entries once.
    assert_eq!(read(&target), "entries: 1\n");
    for result in &results {
        match result {
            Ok(written) => assert!(
                matches!(
                    written.outcome,
                    Outcome::Edited | Outcome::AlreadyRegistered
                ),
                "{written:?}"
            ),
            // Every loser reported a clean precondition or lock failure, and
            // none of them wrote anything: the file above proves it.
            Err(error) => assert!(
                matches!(error, Error::Locked { .. } | Error::Changed(_)),
                "a loser failed for the wrong reason: {error}"
            ),
        }
    }
    assert!(
        results.iter().any(Result::is_ok),
        "every concurrent run failed"
    );
}

/// A semantic check that always says yes, which is a tmux probe finding the
/// edit sound.
struct Accepts;

impl Verify for Accepts {
    fn verify(&self, _: &Path) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn an_accepted_edit_keeps_its_backup_and_stands() {
    let dir = TempDir::new("accepted");
    let target = dir.write("tmux.conf", "before\n");
    let ask = Answer::yes();
    let accepts = Accepts;

    let written = SafeWrite {
        verify: Some(&accepts),
        ..writer(&target, &ask)
    }
    .apply(|_| Plan::Write("after\n".to_owned()))
    .expect("an accepted edit stands");

    assert_eq!(written.outcome, Outcome::Edited);
    assert_eq!(read(&target), "after\n");
    assert_eq!(read(&written.backup.expect("a backup")), "before\n");
}

#[test]
fn a_filesystem_that_refuses_for_its_own_reasons_fails_cleanly() {
    // Each of these is a syscall failing for a reason that is not "the file is
    // busy" or "the file is gone": the branches a real disk takes and no test
    // could otherwise reach.
    for stage in ["stat", "lock", "lock-race"] {
        let dir = TempDir::new("syscall-failure");
        let target = dir.write("tmux.conf", "before\n");
        if stage == "lock-race" {
            // A stale lock to break, so the second attempt is reached at all.
            fs::write(
                dir.join("tmux.conf.tmux-agent-status.lock"),
                format!("{}\t{}\tgone\n", u32::MAX - 1, hostname()),
            )
            .expect("the lock file");
        }
        let ask = Answer::yes();

        let error = SafeWrite {
            faults: Faults::parse(stage),
            ..writer(&target, &ask)
        }
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("the write must fail");

        assert!(matches!(error, Error::Io { .. }), "{stage}: {error}");
        assert_eq!(read(&target), "before\n", "{stage}: the file changed");
    }
}

#[test]
fn an_inspection_that_cannot_stat_the_target_says_so() {
    let dir = TempDir::new("inspect-lstat");
    let target = dir.write("tmux.conf", "before\n");

    let error = write::inspect(&target, &Faults::parse("lstat"))
        .expect_err("a stat that refuses is not an empty document");

    assert!(matches!(error, Error::Io { .. }), "{error}");
}

#[test]
fn a_lock_line_with_a_pid_that_is_not_a_number_is_not_ours() {
    let dir = TempDir::new("lock-nonsense-pid");
    let target = dir.write("tmux.conf", "before\n");
    let lock = dir.join("tmux.conf.tmux-agent-status.lock");
    // Three fields, so the shape is right and only the pid is nonsense.
    fs::write(&lock, "not-a-pid\tsome-host\tsomething\n").expect("the lock file");

    let ask = Answer::yes();
    let error = writer(&target, &ask)
        .apply(|_| Plan::Write("after\n".to_owned()))
        .expect_err("a lock we cannot read is reported");

    assert!(matches!(error, Error::Locked { .. }), "{error}");
    assert_eq!(read(&target), "before\n");
}

#[test]
fn inspect_reports_without_touching_anything() {
    let dir = TempDir::new("inspect");
    let real = dir.write("dotfiles/tmux.conf", "before\n");
    let front = dir.join("tmux.conf");
    std::os::unix::fs::symlink(&real, &front).expect("the link");

    let seen = write::inspect(&front, &Faults::default()).expect("the target can be inspected");

    assert_eq!(seen.named, front);
    assert_eq!(seen.resolved, real);
    assert!(seen.exists);
    assert_eq!(seen.contents, "before\n");
    assert!(
        outside_home_only(&seen.warnings),
        "unexpected warnings: {:?}",
        seen.warnings
    );
    assert_eq!(
        dir.entries(),
        vec!["dotfiles".to_owned(), "tmux.conf".to_owned()]
    );
}

#[test]
fn inspect_reads_a_missing_file_as_an_empty_document() {
    let dir = TempDir::new("inspect-missing");
    let target = dir.join("config/tmux/tmux.conf");

    let seen =
        write::inspect(&target, &Faults::default()).expect("a missing target can be inspected");

    assert!(!seen.exists);
    assert_eq!(seen.contents, "");
    assert_eq!(
        seen.resolved, target,
        "the path resolves through what exists"
    );
    assert!(
        !target.parent().expect("a parent").exists(),
        "inspect must not create anything"
    );
}

#[test]
fn inspect_refuses_what_apply_would_refuse() {
    let dir = TempDir::new("inspect-refuses");
    let a_directory = dir.join("a-directory");
    fs::create_dir(&a_directory).expect("the directory");
    assert!(matches!(
        write::inspect(&a_directory, &Faults::default()),
        Err(Error::NotARegularFile(_))
    ));

    let inner = dir.join("locked");
    fs::create_dir(&inner).expect("the directory");
    let target = inner.join("tmux.conf");
    fs::write(&target, "before\n").expect("the file");
    fs::set_permissions(&inner, fs::Permissions::from_mode(0o555)).expect("chmod");
    assert!(matches!(
        write::inspect(&target, &Faults::default()),
        Err(Error::Unwritable { .. })
    ));
}

fn hostname() -> String {
    String::from_utf8_lossy(
        &std::process::Command::new("hostname")
            .output()
            .expect("hostname runs")
            .stdout,
    )
    .trim()
    .to_owned()
}

/// A lock line naming this very process, which `ps` reports as alive.
fn live_identity() -> String {
    let pid = std::process::id();
    let out = std::process::Command::new("ps")
        .args(["-o", "lstart=,comm=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    let identity = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    format!("{pid}\t{}\t{identity}\n", hostname())
}
