//! The one primitive that touches a user's file.
//!
//! 001 rejected a `setup` command on four specific objections, and each one is
//! a step here rather than a refutation: resolve the symlink chain and edit the
//! target, re-check the file's fingerprint immediately before the rename, hold
//! an exclusive lock against our own concurrent runs, and never truncate - write
//! a sibling temp file and `rename(2)` over it, so a crash at any instant leaves
//! either the old file or the new one.
//!
//! The contract is the deliverable; every other module in `install` is a caller.
//! `tasks/plans/011-install-command.md` is normative for it.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// What the caller decided to do with the bytes that are on disk.
///
/// The no-op test is whether the parsed document already carries our entries,
/// not whether our serialiser reproduces the file byte for byte: reserialising
/// an already-correct hand-maintained config changes nothing semantically and
/// everything textually, and rewriting it would take a pointless backup and put
/// a large no-op diff in someone's dotfiles repo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The document already carries our entries. Nothing is written, and no
    /// backup is taken.
    AlreadyInstalled,
    /// A genuine semantic change, and so a write.
    Write(String),
}

impl Plan {
    /// The bytes this plan would put on disk, when there are any.
    pub fn written(self) -> Option<String> {
        match self {
            Plan::Write(contents) => Some(contents),
            Plan::AlreadyInstalled => None,
        }
    }
}

/// What a successful write did, which is what the summary reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    AlreadyInstalled,
    /// The file did not exist and now does. Different from `Edited` because it
    /// is a different thing to want to undo.
    Created,
    Edited,
}

/// A completed write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// Where the edit actually landed, which is not the path the user typed
    /// when a symlink chain is in the way.
    pub resolved: PathBuf,
    pub outcome: Outcome,
    /// `None` in create mode, where there was nothing to lose.
    pub backup: Option<PathBuf>,
}

/// A condition that is the user saying "not this one", rather than the
/// filesystem saying "you cannot".
///
/// Each is asked rather than refused, because each has a legitimate answer:
/// a dotfiles checkout may live outside `$HOME`, and a mode may be stale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// The file's own mode says read-only, but its directory is writable, so
    /// `rename(2)` can replace it anyway.
    ReadOnlyFile(PathBuf),
    /// A dotfiles checkout may legitimately live elsewhere.
    OutsideHome(PathBuf),
    /// The rename breaks the link, and a setup that hardlinks would silently
    /// diverge from then on.
    HardLinked { path: PathBuf, links: u64 },
}

impl Warning {
    pub fn message(&self) -> String {
        match self {
            Warning::ReadOnlyFile(path) => format!(
                "{} is read-only, but its directory is writable, so the edit would still land.",
                path.display()
            ),
            Warning::OutsideHome(path) => format!(
                "{} is outside $HOME. That is normal for a dotfiles checkout, and worth a look otherwise.",
                path.display()
            ),
            Warning::HardLinked { path, links } => format!(
                "{} has {links} hard links. Replacing it breaks the link, and the other names keep the old contents.",
                path.display()
            ),
        }
    }
}

/// How a warning gets answered, and the only thing here that can block.
///
/// A trait rather than a flag because `write` must not know whether there is a
/// terminal: `-y` and `--dry-run` are answered in `prompt`, and the tests
/// answer with a stub.
pub trait Ask {
    /// Whether to go ahead despite `warning`.
    fn warn(&self, warning: &Warning) -> bool;
}

/// Checked after the bytes are on disk and before the lock is released.
///
/// The tmux steps use this to let tmux mark our homework. A rejection rolls the
/// file back, and that rollback is unconditionally safe because the byte-level
/// verify has just proved the bytes on disk are ours and nobody else's.
pub trait Verify {
    fn verify(&self, path: &Path) -> Result<(), String>;
}

/// Everything a write needs that is not the bytes themselves.
pub struct SafeWrite<'a> {
    /// The path as the user or the discovery named it.
    pub path: &'a Path,
    pub ask: &'a dyn Ask,
    /// Whether some bytes are a complete document in this file's own language.
    ///
    /// This is what tells a write that did not land intact - which must be
    /// restored - from a writer that raced past the fingerprint check - which
    /// must not be, because overwriting a legitimate concurrent write with a
    /// backup taken before it is exactly the data loss this contract exists to
    /// prevent.
    pub parses: fn(&str) -> bool,
    /// The semantic check, for the steps that have one.
    pub verify: Option<&'a dyn Verify>,
    /// Test-only; `Faults::default()` in every real run.
    pub faults: Faults,
}

impl SafeWrite<'_> {
    /// Run the whole contract. Steps are numbered as the plan numbers them.
    pub fn apply(&self, build: impl FnOnce(&str) -> Plan) -> Result<Written, Error> {
        let target = Target::resolve(self.path);
        target.permit(self.ask, &self.faults)?;

        // Create mode makes its directories here rather than at step 6,
        // because the lock file is a sibling of the target and there is
        // nowhere to put it otherwise. Nothing is lost by the earlier move: a
        // document that does not exist is empty, an empty document cannot
        // already carry our entries, and so create mode always goes on to
        // write. `--dry-run` never reaches this line at all.
        if !target.exists {
            create_parents(&target.resolved, &self.faults)?;
        }

        let _lock = Lock::take(&target.resolved, &self.faults)?;

        // 4. Read and fingerprint.
        let before = target.read(&self.faults)?;
        let fingerprint = Fingerprint::of(&target.resolved, &self.faults, "stat")?;

        // 5. Merge, and decide "already installed" semantically.
        let Plan::Write(after) = build(&before) else {
            return Ok(Written {
                resolved: target.resolved,
                outcome: Outcome::AlreadyInstalled,
                backup: None,
            });
        };

        // 6. Back up. Create mode has nothing to lose and so takes none.
        let backup = match target.exists {
            true => Some(back_up(&target.resolved, &self.faults)?),
            false => None,
        };

        // 7. Write a sibling, in the target's own directory so the rename is
        // same-filesystem and therefore atomic.
        let mut temp = write_sibling(&target.resolved, &after, &self.faults)?;

        // 8. Re-check the fingerprint. The window between here and the rename
        // is small and not zero, and step 11 is what covers the difference.
        if self.faults.hits("changed") {
            let _ = fs::write(&target.resolved, "written by somebody else\n");
        }
        if Fingerprint::of(&target.resolved, &self.faults, "stat-again")? != fingerprint {
            return Err(Error::Changed(target.resolved));
        }

        // 9 and 10. Rename, then fsync the directory so it survives a power cut.
        rename(temp.path(), &target.resolved, &self.faults)?;
        temp.taken();
        fsync_parent(&target.resolved, &self.faults)?;

        // 11. Verify byte for byte, and tell the two kinds of mismatch apart.
        self.verify_bytes(&target, &after, &before, backup.as_deref())?;

        // 12. Verify semantically, by asking tmux.
        if let Some(verify) = self.verify {
            if let Err(reason) = verify.verify(&target.resolved) {
                let restored = roll_back(&target.resolved, backup.as_deref(), &self.faults);
                return Err(Error::Rejected {
                    path: target.resolved,
                    backup,
                    reason,
                    restored: restored.is_ok(),
                });
            }
        }

        Ok(Written {
            resolved: target.resolved,
            outcome: match target.exists {
                true => Outcome::Edited,
                false => Outcome::Created,
            },
            backup,
        })
    }

    /// Step 11, whose two rows want opposite responses.
    fn verify_bytes(
        &self,
        target: &Target,
        written: &str,
        before: &str,
        backup: Option<&Path>,
    ) -> Result<(), Error> {
        if self.faults.hits("verify-race") {
            let _ = fs::write(&target.resolved, "{\"raced\": true}\n");
        }
        let found = read_to_string(&target.resolved, &self.faults)?;
        if found == written {
            return Ok(());
        }
        // A truncation is a prefix of what we meant to write, and nothing
        // else is: that is exact rather than a judgement, and it catches the
        // case a language check cannot - a tmux config has no notion of being
        // truncated, because every prefix of a valid one is also valid.
        let ours_cut_short = written.starts_with(&found);
        if !ours_cut_short && (self.parses)(&found) && found != before {
            // A complete document that is neither ours nor the backup: somebody
            // wrote it after step 8. We cannot know whose write is worth more,
            // so we keep all three and hand the reconciliation to a human. It
            // is the only outcome in this contract that asks for one, and the
            // only one where a human genuinely has to.
            let kept = keep(&target.resolved, written, &self.faults);
            return Err(Error::Raced {
                path: target.resolved.clone(),
                backup: backup.map(Path::to_path_buf),
                temp: kept,
            });
        }
        // Empty, truncated, or unparseable in its own language: our write did
        // not land intact. Restore, without asking - in `-y` there is nobody to
        // ask, and interactively the honest answer is always yes.
        let restore = roll_back(&target.resolved, backup, &self.faults).err();
        Err(Error::Damaged {
            path: target.resolved.clone(),
            backup: backup.map(Path::to_path_buf),
            restore,
        })
    }
}

/// A path that has been resolved, and what is at the end of it.
struct Target {
    resolved: PathBuf,
    exists: bool,
}

impl Target {
    /// Step 1. A symlink chain resolves to its target and the edit lands there;
    /// the link itself is never replaced, because every later step addresses
    /// the resolved path.
    fn resolve(path: &Path) -> Target {
        match path.canonicalize() {
            Ok(resolved) => Target {
                resolved,
                exists: true,
            },
            // Create mode: canonicalize the nearest existing ancestor so the
            // new file lands in the same place a resolved one would.
            Err(_) => Target {
                resolved: resolve_missing(path),
                exists: false,
            },
        }
    }

    /// Step 2. Refuse what must not be edited; ask about what is merely odd.
    fn permit(&self, ask: &dyn Ask, faults: &Faults) -> Result<(), Error> {
        let metadata = match self.exists {
            true => Some(
                faults
                    .guard("lstat")
                    .and_then(|()| fs::symlink_metadata(&self.resolved))
                    .map_err(|source| Error::io("reading the file's metadata", source))?,
            ),
            false => None,
        };

        if let Some(metadata) = &metadata {
            if !metadata.is_file() {
                return Err(Error::NotARegularFile(self.resolved.clone()));
            }
        }

        // The permission that matters is the parent directory's, not the
        // file's: `rename(2)` over an `r--r--r--` file succeeds when its
        // directory is writable, so testing the file's mode would refuse edits
        // that work and permit edits that do not.
        let parent = self.parent();
        if unwritable(parent) {
            return Err(Error::Unwritable {
                path: self.resolved.clone(),
                directory: parent.to_path_buf(),
            });
        }

        for warning in self.warnings(metadata.as_ref()) {
            if !ask.warn(&warning) {
                return Err(Error::Declined(warning));
            }
        }
        Ok(())
    }

    /// The directory the temp file and the rename have to work in.
    ///
    /// A resolved path always has one; the root directory is the degenerate
    /// case and is its own parent.
    fn parent(&self) -> &Path {
        self.resolved.parent().unwrap_or(&self.resolved)
    }

    fn warnings(&self, metadata: Option<&fs::Metadata>) -> Vec<Warning> {
        let mut warnings = Vec::new();
        if let Some(metadata) = metadata {
            if metadata.permissions().readonly() {
                warnings.push(Warning::ReadOnlyFile(self.resolved.clone()));
            }
            if metadata.nlink() > 1 {
                warnings.push(Warning::HardLinked {
                    path: self.resolved.clone(),
                    links: metadata.nlink(),
                });
            }
        }
        if !in_home(&self.resolved) {
            warnings.push(Warning::OutsideHome(self.resolved.clone()));
        }
        warnings
    }

    /// Step 4. Create mode reads an empty document rather than a file.
    fn read(&self, faults: &Faults) -> Result<String, Error> {
        match self.exists {
            true => read_to_string(&self.resolved, faults),
            false => Ok(String::new()),
        }
    }
}

/// Read-only inspection, for the plan phase and for `--dry-run`.
///
/// Everything it reports is answered by `stat`, so it creates nothing, and the
/// dry run's promise that it changes nothing survives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    pub named: PathBuf,
    pub resolved: PathBuf,
    pub exists: bool,
    /// Empty when the file does not exist.
    pub contents: String,
    pub warnings: Vec<Warning>,
}

/// Look at a target without touching it.
pub fn inspect(path: &Path) -> Result<Inspection, Error> {
    // Read-only, so nothing here is ever asked to fail: the fault switch is
    // for the write, and a plan that could be made to fail on demand would
    // only be testing the switch.
    let faults = Faults::default();
    let target = Target::resolve(path);
    let metadata = match target.exists {
        true => Some(
            fs::symlink_metadata(&target.resolved)
                .map_err(|source| Error::io("reading the file's metadata", source))?,
        ),
        false => None,
    };
    if let Some(metadata) = &metadata {
        if !metadata.is_file() {
            return Err(Error::NotARegularFile(target.resolved.clone()));
        }
    }
    let parent = target.parent();
    if unwritable(parent) {
        return Err(Error::Unwritable {
            path: target.resolved.clone(),
            directory: parent.to_path_buf(),
        });
    }
    Ok(Inspection {
        named: path.to_path_buf(),
        warnings: target.warnings(metadata.as_ref()),
        contents: target.read(&faults)?,
        resolved: target.resolved,
        exists: target.exists,
    })
}

/// The nearest existing ancestor, resolved, with the rest of the path rejoined.
fn resolve_missing(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut head = path;
    loop {
        if let Ok(resolved) = head.canonicalize() {
            return tail.iter().rev().fold(resolved, |acc, part| acc.join(part));
        }
        // Nothing along the path exists, which means the path is relative to a
        // working directory that has itself been removed. There is nothing to
        // resolve against, so it stands as it was written.
        let (Some(name), Some(parent)) = (head.file_name(), head.parent()) else {
            return path.to_path_buf();
        };
        tail.push(name.to_owned());
        head = parent;
    }
}

/// Whether the directory's mode forbids writing.
///
/// Mode rather than `access(2)`, deliberately: this runs in the plan phase and
/// under `--dry-run`, where creating a probe file to find out would break the
/// promise that nothing is written. It gets the case the plan cares about right,
/// because a package store is mounted read-only and its directories are
/// `r-xr-xr-x`; where it is too optimistic, the real error at the temp-file
/// write is the backstop and names the same path.
fn unwritable(directory: &Path) -> bool {
    fs::metadata(directory).is_ok_and(|meta| meta.permissions().readonly())
}

fn in_home(path: &Path) -> bool {
    // `$HOME` from the environment, never `getpwuid`: the tests point a child
    // process at a temp home, and a tool that cannot be redirected cannot be
    // tested.
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| home.canonicalize().ok())
        .is_some_and(|home| path.starts_with(home))
}

/// Whether a resolved path sits in a package store, which decides the wording
/// of a refusal and nothing else.
///
/// "The path is inside `/nix/store`" is emphatically not the test for whether
/// to edit: a `mkOutOfStoreSymlink` chain passes through the store and ends in
/// a writable dotfiles checkout, which the writability test gets right for
/// free. This only picks the advice.
fn in_a_package_store(path: &Path) -> bool {
    ["/nix/store/", "/gnu/store/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Step 6. Beside the target, never deleted, never reused, never overwritten.
fn back_up(target: &Path, faults: &Faults) -> Result<PathBuf, Error> {
    let backup = target.with_file_name(format!(
        "{}.bak-{}",
        file_name(target),
        timestamp(SystemTime::now())
    ));
    faults
        .guard("backup")
        .and_then(|()| fs::copy(target, &backup))
        .map_err(|source| Error::io("copying the file to its backup", source))?;
    fsync(&backup, faults)?;
    Ok(backup)
}

/// A temp file that removes itself unless the rename takes it.
///
/// Every way out of a failed write goes through this, so the invariant is one
/// thing in one place: a run that fails leaves nothing behind, and a
/// half-written sibling of somebody's tmux.conf is exactly the residue this
/// tool promises not to leave.
struct Temp {
    path: PathBuf,
    renamed: bool,
}

impl Temp {
    fn path(&self) -> &Path {
        &self.path
    }

    /// The rename took it, so there is nothing left to remove.
    fn taken(&mut self) {
        self.renamed = true;
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        if !self.renamed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Step 7. In the target's own directory, so step 9's rename is atomic.
fn write_sibling(target: &Path, contents: &str, faults: &Faults) -> Result<Temp, Error> {
    let temp = target.with_file_name(format!(
        "{}.tmp-{}-{}",
        file_name(target),
        std::process::id(),
        nonce()
    ));
    let bytes = faults.damage(contents);
    let mut file = faults
        .guard("create")
        .and_then(|()| File::create(&temp))
        .map_err(|source| Error::io("creating a temp file", source))?;
    // From here the file exists, so from here it is something that cleans up
    // after itself.
    let temp = Temp {
        path: temp,
        renamed: false,
    };
    fill(&mut file, &bytes, temp.path(), faults)?;
    // The new file inherits the old one's mode, so a config the user chmodded
    // keeps the mode they gave it.
    if let Ok(metadata) = fs::metadata(target) {
        let _ = fs::set_permissions(temp.path(), metadata.permissions());
    }
    Ok(temp)
}

/// Put the bytes in the temp file and make sure they are really there.
fn fill(file: &mut File, bytes: &str, temp: &Path, faults: &Faults) -> Result<(), Error> {
    faults
        .guard("write")
        .and_then(|()| file.write_all(bytes.as_bytes()))
        .map_err(|source| Error::io("writing the temp file", source))?;
    sync(temp, "the temp file", "fsync-temp", faults)
}

/// Step 9.
fn rename(temp: &Path, target: &Path, faults: &Faults) -> Result<(), Error> {
    faults
        .guard("rename")
        .and_then(|()| fs::rename(temp, target))
        .map_err(|source| Error::io("renaming the temp file over the target", source))
}

/// Step 10.
fn fsync_parent(target: &Path, faults: &Faults) -> Result<(), Error> {
    let parent = target.parent().unwrap_or(target);
    sync(parent, "the directory", "fsync-dir", faults)
}

fn fsync(path: &Path, faults: &Faults) -> Result<(), Error> {
    sync(path, "the backup", "fsync", faults)
}

fn sync(path: &Path, what: &str, stage: &str, faults: &Faults) -> Result<(), Error> {
    faults
        .guard(stage)
        .and_then(|()| File::open(path))
        .and_then(|file| file.sync_all())
        .map_err(|source| Error::io(format!("syncing {what}"), source))
}

/// Put the file back the way it was, or delete what create mode created.
fn roll_back(target: &Path, backup: Option<&Path>, faults: &Faults) -> io::Result<()> {
    if faults.hits("restore") {
        return Err(faults.error("restore"));
    }
    match backup {
        Some(backup) => fs::copy(backup, target).map(drop),
        None => fs::remove_file(target),
    }
}

/// Put what we meant to write somewhere a human can see it.
///
/// The temp file became the target at the rename, so this writes the bytes
/// afresh under a name of its own. It is a new file beside the target and never
/// an overwrite of anything, which is the whole point of the branch it serves.
fn keep(target: &Path, contents: &str, faults: &Faults) -> Option<PathBuf> {
    let kept = match faults.hits("keep") {
        // A directory, so the write below fails the way a full disk would.
        true => target.with_file_name("."),
        false => target.with_file_name(format!("{}.raced-{}", file_name(target), nonce())),
    };
    match fs::write(&kept, contents) {
        Ok(()) => Some(kept),
        // A worse report, but not a worse outcome: the file on disk and the
        // backup are still named, and neither has been touched.
        Err(_) => None,
    }
}

fn read_to_string(path: &Path, faults: &Faults) -> Result<String, Error> {
    faults
        .guard("read")
        .and_then(|()| fs::read_to_string(path))
        .map_err(|source| Error::io("reading the file", source))
}

fn create_parents(target: &Path, faults: &Faults) -> Result<(), Error> {
    let parent = target.parent().unwrap_or(target);
    faults
        .guard("mkdir")
        .and_then(|()| fs::create_dir_all(parent))
        .map_err(|source| Error::io("creating the directory", source))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// UTC, RFC 3339 basic, which sorts and carries no separator a filesystem minds.
fn timestamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (year, month, day, hour, minute, second) = civil(secs);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// Days-and-seconds to a civil date, by the usual era arithmetic.
///
/// Written out rather than pulled in: one date, formatted one way, in a crate
/// whose whole dependency list is three lines long.
fn civil(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86_400;
    let rest = secs % 86_400;
    // 719_468 is the days from 0000-03-01 to 1970-01-01.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + u64::from(month <= 2);
    (year, month, day, rest / 3_600, (rest / 60) % 60, rest % 60)
}

/// Enough uniqueness for a name in one directory, from what is already to hand.
fn nonce() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0u64, |d| u64::from(d.subsec_nanos()));
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{now:x}{count:x}")
}

/// `(len, mtime)`, which is what `stat` answers and what step 8 compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    mtime: i64,
    mtime_nanos: i64,
}

impl Fingerprint {
    fn of(path: &Path, faults: &Faults, stage: &str) -> Result<Option<Fingerprint>, Error> {
        // The fault stats a path that runs *through* a regular file, which
        // fails with `ENOTDIR` rather than `ENOENT`: a refusal that is not
        // absence, and so not create mode.
        let through_a_file;
        let path = match faults.hits(stage) {
            true => {
                through_a_file = path.join("not-a-directory");
                &through_a_file
            }
            false => path,
        };
        match fs::metadata(path) {
            Ok(meta) => Ok(Some(Fingerprint {
                len: meta.len(),
                mtime: meta.mtime(),
                mtime_nanos: meta.mtime_nsec(),
            })),
            // In create mode there is nothing to fingerprint, and a target that
            // appears between here and the rename is caught by the comparison
            // like any other concurrent write.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::io("stat-ing the file", source)),
        }
    }
}

/// The exclusive lock of step 3.
///
/// It serialises *our* concurrent runs. It does not serialise the agent that
/// also writes the file - nothing can, since the agent takes no lock - which is
/// what the fingerprint re-check and the post-rename verify are for.
struct Lock {
    path: PathBuf,
}

impl Lock {
    fn take(target: &Path, faults: &Faults) -> Result<Lock, Error> {
        let path = target.with_file_name(format!("{}.tmux-agent-status.lock", file_name(target)));
        match create_new(&path, faults.hits("lock")) {
            Ok(mut file) => {
                let _ = file.write_all(Holder::current().to_line().as_bytes());
                let _ = file.sync_all();
                Ok(Lock { path })
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Lock::break_or_report(&path, faults)
            }
            Err(source) => Err(Error::io("creating the lock file", source)),
        }
    }

    /// A `SIGKILL` or a power cut runs no cleanup, so a stale lock is expected
    /// rather than exceptional. It is broken only when that exact process is
    /// provably gone: a pid alone is reused, and "older than 60 seconds" alone
    /// breaks a live lock held by a slow filesystem.
    fn break_or_report(path: &Path, faults: &Faults) -> Result<Lock, Error> {
        let held = fs::read_to_string(path).unwrap_or_default();
        let holder = Holder::from_line(held.trim());
        match holder.as_ref().map(Holder::liveness) {
            Some(Liveness::Gone) => {
                faults
                    .guard("unlock")
                    .and_then(|()| fs::remove_file(path))
                    .map_err(|source| Error::io("removing a stale lock file", source))?;
                Lock::take_after_breaking(path, faults)
            }
            _ => Err(Error::Locked {
                lock: path.to_path_buf(),
                holder: held.trim().to_owned(),
            }),
        }
    }

    fn take_after_breaking(path: &Path, faults: &Faults) -> Result<Lock, Error> {
        match create_new(path, faults.hits("lock-race")) {
            Ok(mut file) => {
                let _ = file.write_all(Holder::current().to_line().as_bytes());
                Ok(Lock {
                    path: path.to_path_buf(),
                })
            }
            // Another run of ours broke the same stale lock first. It holds it
            // now, and this run is a loser like any other.
            Err(source) => Err(Error::io("creating the lock file", source)),
        }
    }
}

/// `O_CREAT | O_EXCL`, with the fault switch standing in for a filesystem that
/// refuses for a reason other than the lock already being held.
fn create_new(path: &Path, fail: bool) -> io::Result<File> {
    if fail {
        return Err(io::Error::other("TMUX_AGENT_STATUS_TEST_FAULT=lock"));
    }
    OpenOptions::new().write(true).create_new(true).open(path)
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Who holds a lock, in enough detail to prove they are gone.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Holder {
    pid: u32,
    host: String,
    /// The process's start time and command, as `ps` reports them. A pid on its
    /// own is reused; a pid whose start time still matches is the same process.
    identity: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Liveness {
    /// The pid is gone, or is a different process now. Safe to break.
    Gone,
    /// That exact process is still running.
    Alive,
    /// Another host, or a `ps` that could not answer. Reported, never broken.
    Unknown,
}

impl Holder {
    fn current() -> Holder {
        let pid = std::process::id();
        Holder {
            pid,
            host: hostname(),
            identity: identity(pid).unwrap_or_default(),
        }
    }

    fn to_line(&self) -> String {
        format!("{}\t{}\t{}\n", self.pid, self.host, self.identity)
    }

    fn from_line(line: &str) -> Option<Holder> {
        let mut fields = line.splitn(3, '\t');
        let pid = fields.next()?.parse().ok()?;
        let host = fields.next()?.to_owned();
        let identity = fields.next()?.to_owned();
        Some(Holder {
            pid,
            host,
            identity,
        })
    }

    fn liveness(&self) -> Liveness {
        if self.host != hostname() {
            return Liveness::Unknown;
        }
        match identity(self.pid) {
            // `ps` said nothing about that pid: it is gone.
            None => Liveness::Gone,
            Some(found) if found == self.identity => Liveness::Alive,
            // The pid has been reused by something else.
            Some(_) => Liveness::Gone,
        }
    }
}

/// A process's start time and command, or `None` when there is no such process.
///
/// `ps` rather than `/proc`, because macOS has no `/proc` and this tool is
/// tested on both.
fn identity(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", "lstart=,comm=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn hostname() -> String {
    Command::new("hostname")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The test-only fault switch: unstable, unsupported, and documented nowhere a
/// user looks.
///
/// It exists so that every failure branch in this module - a write that does not
/// land, a backup that cannot be taken, a writer that races us, a restore that
/// fails in its turn - is a branch a test can reach, because a branch no test
/// can reach is a branch nobody has read, and this is the module where that
/// matters most.
///
/// A value rather than a global read of the environment: `cargo test` runs its
/// tests in threads of one process, and a test that sets an environment
/// variable sets it for its neighbours too. The binary reads the environment
/// once, at the edge, and passes what it found down here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faults(Vec<String>);

impl Faults {
    /// What `TMUX_AGENT_STATUS_TEST_FAULT` names, which is how an integration
    /// test reaches the real binary.
    pub fn from_env() -> Faults {
        Faults::parse(&std::env::var("TMUX_AGENT_STATUS_TEST_FAULT").unwrap_or_default())
    }

    /// A comma-separated list of stages, which is how an in-process test names
    /// them without reaching for the environment at all.
    pub fn parse(stages: &str) -> Faults {
        Faults(
            stages
                .split(',')
                .filter(|stage| !stage.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    }

    fn hits(&self, stage: &str) -> bool {
        self.0.iter().any(|named| named == stage)
    }

    /// An `io::Error` naming the stage, so a fault reads as itself in a report.
    fn error(&self, stage: &str) -> io::Error {
        io::Error::other(format!("TMUX_AGENT_STATUS_TEST_FAULT={stage}"))
    }

    /// Fail before a syscall the run asked to see fail.
    ///
    /// Every fallible call below goes through this, so that "the disk refused"
    /// is a branch a test can reach at each of them rather than at none of
    /// them. Chained ahead of the real call, it also means each site has one
    /// error path instead of two.
    fn guard(&self, stage: &str) -> io::Result<()> {
        match self.hits(stage) {
            true => Err(self.error(stage)),
            false => Ok(()),
        }
    }

    /// Make the bytes that reach disk differ from the bytes we meant to write.
    fn damage(&self, contents: &str) -> String {
        if self.hits("truncate") {
            return contents[..contents.len() / 2].to_owned();
        }
        if self.hits("empty") {
            return String::new();
        }
        if self.hits("scramble") {
            return format!("\u{0}not what we meant\u{0}{contents}");
        }
        contents.to_owned()
    }
}

/// Every way the contract can refuse or fail, each naming what a human needs.
#[derive(Debug)]
pub enum Error {
    NotARegularFile(PathBuf),
    Unwritable {
        path: PathBuf,
        directory: PathBuf,
    },
    Declined(Warning),
    Locked {
        lock: PathBuf,
        holder: String,
    },
    Changed(PathBuf),
    /// Our write did not land intact, and the backup went back.
    Damaged {
        path: PathBuf,
        backup: Option<PathBuf>,
        /// Set when the restore itself failed, which needs two filesystem
        /// failures in a row and is the one path that leaves a user work to do.
        restore: Option<io::Error>,
    },
    /// Somebody else's complete document is on disk. Nothing was restored.
    Raced {
        path: PathBuf,
        backup: Option<PathBuf>,
        temp: Option<PathBuf>,
    },
    /// The semantic check said no.
    Rejected {
        path: PathBuf,
        backup: Option<PathBuf>,
        reason: String,
        restored: bool,
    },
    Io {
        what: String,
        source: io::Error,
    },
}

impl Error {
    fn io(what: impl Into<String>, source: io::Error) -> Error {
        Error::Io {
            what: what.into(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotARegularFile(path) => write!(
                f,
                "{} is not a regular file, so it is not one to edit",
                path.display()
            ),
            Error::Unwritable { path, directory } if in_a_package_store(path) => write!(
                f,
                "{} is generated by nix and its directory is read-only.\n\
                 Add the fragment above to the generator that produces it, not to the file.",
                path.display()
            ),
            Error::Unwritable { path, directory } => write!(
                f,
                "{} cannot be edited: its directory {} is not writable.\n\
                 That is the path the edit resolves to, which is not always the path you typed.",
                path.display(),
                directory.display()
            ),
            Error::Declined(warning) => write!(f, "declined: {}", warning.message()),
            Error::Locked { lock, holder } => write!(
                f,
                "another tmux-agent-status install holds {}\n  held by: {holder}\n\
                 Remove the lock file by hand if you are sure that process is gone.",
                lock.display()
            ),
            Error::Changed(path) => write!(
                f,
                "{} changed while we were preparing the edit; nothing was written",
                path.display()
            ),
            Error::Damaged {
                path,
                backup,
                restore,
            } => {
                let aftermath = match (backup, restore) {
                    (Some(backup), None) => {
                        format!("; the file was restored from {}", backup.display())
                    }
                    (Some(backup), Some(error)) => format!(
                        "; restoring it failed too ({error}).\nFinish it by hand:\n  cp {} {}",
                        backup.display(),
                        path.display()
                    ),
                    (None, None) => "; the file we created was removed".to_owned(),
                    (None, Some(error)) => format!(
                        "; removing the file we created failed too ({error}).\nRemove it by hand:\n  rm {}",
                        path.display()
                    ),
                };
                write!(
                    f,
                    "the edit to {} did not land intact{aftermath}",
                    path.display()
                )
            }
            Error::Raced { path, backup, temp } => write!(
                f,
                "{} was written by something else while we were writing it.\n\
                 Nothing was restored, because that write may be worth more than ours.\n\
                 Reconcile these by hand:\n  the file as it now stands: {}{}{}",
                path.display(),
                path.display(),
                named("\n  the backup taken first:    ", backup.as_deref()),
                named("\n  what we meant to write:    ", temp.as_deref()),
            ),
            Error::Rejected {
                path,
                backup,
                reason,
                restored,
            } => {
                let aftermath = match (restored, backup) {
                    (true, Some(backup)) => {
                        format!("\nThe file was restored from {}.", backup.display())
                    }
                    (true, None) => "\nThe file we created was removed.".to_owned(),
                    (false, Some(backup)) => format!(
                        "\nRestoring it failed. Finish it by hand:\n  cp {} {}",
                        backup.display(),
                        path.display()
                    ),
                    (false, None) => format!(
                        "\nRemoving the file we created failed. Remove it by hand:\n  rm {}",
                        path.display()
                    ),
                };
                write!(
                    f,
                    "{} was rejected after the write: {reason}{aftermath}",
                    path.display()
                )
            }
            Error::Io { what, source } => write!(f, "{what} failed: {source}"),
        }
    }
}

/// A labelled path, or nothing at all when there is no path to label.
fn named(label: &str, path: Option<&Path>) -> String {
    path.map(|path| format!("{label}{}", path.display()))
        .unwrap_or_default()
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_is_utc_rfc_3339_basic() {
        assert_eq!(
            timestamp(UNIX_EPOCH + std::time::Duration::from_secs(0)),
            "19700101T000000Z"
        );
        // 2026-09-12T11:35:07Z
        assert_eq!(
            timestamp(UNIX_EPOCH + std::time::Duration::from_secs(1_789_212_907)),
            "20260912T113507Z"
        );
        // A leap day, which the era arithmetic has to get right to sort.
        assert_eq!(
            timestamp(UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800)),
            "20240229T000000Z"
        );
        // Before the epoch there is no ordering to preserve and no file to
        // name; the clock is simply wrong, and zero is as good an answer.
        assert_eq!(
            timestamp(UNIX_EPOCH - std::time::Duration::from_secs(1)),
            "19700101T000000Z"
        );
    }

    #[test]
    fn a_plan_answers_only_when_it_would_write() {
        assert_eq!(
            Plan::Write("bytes".to_owned()).written(),
            Some("bytes".to_owned())
        );
        assert_eq!(Plan::AlreadyInstalled.written(), None);
    }

    #[test]
    fn a_nonce_does_not_repeat() {
        assert_ne!(nonce(), nonce());
    }

    #[test]
    fn a_holder_line_round_trips() {
        let holder = Holder::current();
        assert_eq!(Holder::from_line(holder.to_line().trim()), Some(holder));
    }

    #[test]
    fn a_lock_file_that_is_not_ours_is_reported_rather_than_broken() {
        assert_eq!(Holder::from_line(""), None);
        assert_eq!(Holder::from_line("nonsense"), None);
        assert_eq!(Holder::from_line("123"), None);
        assert_eq!(Holder::from_line("123\thost"), None);
    }

    #[test]
    fn our_own_process_is_alive() {
        assert_eq!(Holder::current().liveness(), Liveness::Alive);
    }

    #[test]
    fn a_holder_on_another_host_is_never_broken() {
        let holder = Holder {
            host: "somewhere-else".to_owned(),
            ..Holder::current()
        };
        assert_eq!(holder.liveness(), Liveness::Unknown);
    }

    #[test]
    fn a_pid_that_is_gone_or_reused_can_be_broken() {
        // pid 1 exists on every Unix, and is not this process's identity.
        let reused = Holder {
            pid: 1,
            identity: "not what pid 1 is".to_owned(),
            ..Holder::current()
        };
        assert_eq!(reused.liveness(), Liveness::Gone);

        // A pid nothing can be running under.
        let gone = Holder {
            pid: u32::MAX - 1,
            ..Holder::current()
        };
        assert_eq!(gone.liveness(), Liveness::Gone);
    }

    #[test]
    fn every_warning_says_what_it_is_about() {
        let path = PathBuf::from("/tmp/example");
        for warning in [
            Warning::ReadOnlyFile(path.clone()),
            Warning::OutsideHome(path.clone()),
            Warning::HardLinked { path, links: 2 },
        ] {
            assert!(warning.message().contains("/tmp/example"), "{warning:?}");
        }
    }

    #[test]
    fn a_path_under_home_is_not_warned_about() {
        let home = PathBuf::from(std::env::var_os("HOME").expect("a home directory"));
        assert!(in_home(&home.canonicalize().unwrap().join(".tmux.conf")));
        assert!(!in_home(Path::new("/")));
    }

    #[test]
    fn a_store_path_only_changes_the_wording() {
        assert!(in_a_package_store(Path::new(
            "/nix/store/abc-tmux/share/x.conf"
        )));
        assert!(in_a_package_store(Path::new("/gnu/store/abc/x.conf")));
        // The `mkOutOfStoreSymlink` case: the chain passes through the store,
        // but the resolved path does not end there.
        assert!(!in_a_package_store(Path::new("/home/u/dotfiles/tmux.conf")));
    }

    #[test]
    fn every_error_says_what_happened_and_names_the_files() {
        let path = PathBuf::from("/tmp/example.conf");
        let backup = PathBuf::from("/tmp/example.conf.bak-19700101T000000Z");
        let failed = || io::Error::other("the disk went away");
        let cases = [
            Error::NotARegularFile(path.clone()),
            Error::Unwritable {
                path: PathBuf::from("/nix/store/abc/share/x.conf"),
                directory: PathBuf::from("/nix/store/abc/share"),
            },
            Error::Unwritable {
                path: path.clone(),
                directory: PathBuf::from("/tmp"),
            },
            Error::Declined(Warning::OutsideHome(path.clone())),
            Error::Locked {
                lock: path.clone(),
                holder: "1\thost\tsomething".to_owned(),
            },
            Error::Changed(path.clone()),
            Error::Damaged {
                path: path.clone(),
                backup: Some(backup.clone()),
                restore: None,
            },
            Error::Damaged {
                path: path.clone(),
                backup: Some(backup.clone()),
                restore: Some(failed()),
            },
            Error::Damaged {
                path: path.clone(),
                backup: None,
                restore: None,
            },
            Error::Damaged {
                path: path.clone(),
                backup: None,
                restore: Some(failed()),
            },
            Error::Raced {
                path: path.clone(),
                backup: Some(backup.clone()),
                temp: Some(PathBuf::from("/tmp/example.conf.raced-1")),
            },
            Error::Raced {
                path: path.clone(),
                backup: None,
                temp: None,
            },
            Error::Rejected {
                path: path.clone(),
                backup: Some(backup.clone()),
                reason: "tmux said no".to_owned(),
                restored: true,
            },
            Error::Rejected {
                path: path.clone(),
                backup: None,
                reason: "tmux said no".to_owned(),
                restored: true,
            },
            Error::Rejected {
                path: path.clone(),
                backup: Some(backup),
                reason: "tmux said no".to_owned(),
                restored: false,
            },
            Error::Rejected {
                path: path.clone(),
                backup: None,
                reason: "tmux said no".to_owned(),
                restored: false,
            },
            Error::io("opening the door", failed()),
        ];
        for error in cases {
            let message = error.to_string();
            // Every refusal names the file it is about, so a user can go and
            // look at it. `Io` is the exception by contract: it names the
            // operation and what the operating system said about it.
            match &error {
                Error::Io { .. } => assert!(
                    message.contains("opening the door") && message.contains("disk"),
                    "{error:?} loses the operation or the cause: {message}"
                ),
                _ => assert!(
                    message.contains("example.conf") || message.contains("x.conf"),
                    "{error:?} names no path: {message}"
                ),
            }
        }
    }

    #[test]
    fn a_path_with_nothing_existing_along_it_stands_as_written() {
        // The working directory itself is gone, so there is nothing to resolve
        // against and no ancestor to fold the tail back onto.
        assert_eq!(resolve_missing(Path::new("")), PathBuf::from(""));
    }

    #[test]
    fn the_fault_switch_is_off_unless_a_stage_is_named() {
        assert!(!Faults::default().hits("truncate"));
        assert_eq!(Faults::default().damage("kept"), "kept");
        // The environment is read once, at the edge; an unset variable names
        // no stages rather than one empty one.
        assert_eq!(Faults::from_env(), Faults::default());
    }

    #[test]
    fn a_named_fault_damages_exactly_its_own_stage() {
        assert!(Faults::parse("empty,restore").hits("restore"));
        assert!(!Faults::parse("empty").hits("restore"));
        assert_eq!(Faults::parse("empty").damage("kept"), "");
        assert_eq!(Faults::parse("truncate").damage("kept"), "ke");
        assert!(
            Faults::parse("scramble")
                .damage("kept")
                .contains("not what we meant")
        );
        assert!(
            Faults::parse("rename")
                .error("rename")
                .to_string()
                .contains("rename")
        );
    }
}
