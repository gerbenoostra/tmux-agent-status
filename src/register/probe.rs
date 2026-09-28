//! Letting tmux mark our homework.
//!
//! Our own parser agreeing with itself proves nothing about what tmux will do
//! with the line we wrote. Verified on 3.6a, and the reason this module exists
//! at all: an unknown command name, or a valid command with one stray extra
//! argument - exactly what a quoting bug produces - makes tmux **abandon the
//! entire config file**, silently, exit 0, and fall back to its compiled-in
//! defaults. One malformed line does not cost the user our glyph; it costs them
//! their whole tmux configuration, with a config file that still looks right.
//!
//! So after editing a tmux config, start a throwaway server on it and compare
//! its whole option dump against a baseline taken before the edit. Reading back
//! one option would not do: an abandoned config answers
//! `show-options -gwv window-status-format` perfectly happily. It just answers
//! with the default.

use std::cell::Cell;
use std::fs;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::format;

/// How long any one tmux invocation is given.
///
/// The probe loads the user's real config, so their `run-shell`, `if-shell` and
/// plugin-manager bootstrap actually run. That is a disclosed side effect, and
/// this is the bound on it.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The prefix of the private directories the probe creates under `private_root()`.
///
/// Exposed for tests that assert no dir with this prefix remains after the
/// probes finish.
pub const PRIVATE_PREFIX: &str = "tmux-agent-status-probe";

/// The directory under which probe private directories are created.
///
/// `/tmp` is used directly because `std::env::temp_dir()` on macOS resolves to
/// a long `/var/folders/.../T/` path; once tmux appends `tmux-<uid>/<socket>`
/// the result can exceed `sun_path`.
pub fn private_root() -> &'static Path {
    Path::new("/tmp")
}

/// What the running server says it would load: candidates, never a decision.
pub fn config_files() -> Option<String> {
    run_here(&["display-message", "-p", "#{config_files}"])
}

/// This tmux's compiled-in default format.
///
/// Asked rather than remembered: the default has changed between tmux versions
/// and the one in *this* tmux is the only one that is right. `-f /dev/null`
/// means the user's config is not loaded, so the probe server has no side
/// effects: it lives in a private directory removed with it.
pub fn compiled_in_default() -> Option<String> {
    let server = Server::start_on(Path::new("/dev/null"), TIMEOUT)?;
    server
        .ask(&["show-options", "-gwv", format::OPTIONS[0]])
        .map(|value| value.trim_end().to_owned())
}

/// Everything a throwaway server can tell us about what a config produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dump {
    lines: Vec<String>,
}

impl Dump {
    /// The whole option and hook state, as `name value` lines.
    fn of(server: &Server) -> Option<Dump> {
        let mut lines = Vec::new();
        for args in [
            ["show-options", "-g"],
            ["show-options", "-gw"],
            // `focus-events` is a server option, which neither of the above
            // lists (verified on 3.6a: only `-s` does, though `-gv` asks for
            // it by name).
            ["show-options", "-s"],
            ["show-hooks", "-g"],
            ["show-hooks", "-gw"],
        ] {
            // The `?` arm: a tmux that answers one of these and then stops
            // answering. The fault switch below cannot arrange it, because
            // `no-tmux` fails `Server::start_on` first and nothing downstream
            // is reached; a staged spelling of it could. The marker exempts
            // this whole line, success path included.
            lines.extend(server.ask(&args)?.lines().map(str::to_owned)); // coverage: off
        }
        lines.sort();
        Some(Dump { lines })
    }

    /// The name each line assigns, which is its first word.
    fn named(&self) -> Vec<(&str, &str)> {
        self.lines
            .iter()
            .map(|line| match line.split_once(' ') {
                Some((name, value)) => (name, value),
                // A flag option prints as a bare name with no value.
                None => (line.as_str(), ""),
            })
            .collect()
    }

    /// Whether anything tmux reads under `name` satisfies `holds`.
    ///
    /// Any, rather than the first: one name can appear more than once, because
    /// tmux reports an unset hook bare and a set one under `name[index]`, and
    /// the bare spelling sorts first. Asking "is there one that holds" is the
    /// question either way.
    pub fn any_value(&self, name: &str, holds: impl Fn(&str) -> bool) -> bool {
        self.named()
            .into_iter()
            .filter(|(found, _)| without_index(found) == name)
            .any(|(_, value)| holds(value))
    }
}

/// A hook name with its `[index]` removed.
///
/// tmux reports an unset hook under its bare name and a set one under
/// `name[index]`, so one edit shows up as two changes under two spellings of
/// the same thing.
pub fn without_index(name: &str) -> &str {
    name.split_once('[').map_or(name, |(head, _)| head)
}

/// One option or hook whose value the edit moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// What changed between the config as it was and the config as we wrote it.
///
/// The assertion the caller makes on this is that the *only* differences are
/// the ones it intended. Anything else moving - or everything reverting to
/// tmux's defaults, which is what an abandoned config looks like - fails the
/// step.
pub fn changes(baseline: &Dump, candidate: &Dump) -> Vec<Change> {
    let before = baseline.named();
    let after = candidate.named();
    let mut changes = Vec::new();

    for (name, value) in &after {
        match before.iter().find(|(other, _)| other == name) {
            Some((_, was)) if was == value => {}
            was => changes.push(Change {
                name: (*name).to_owned(),
                before: was.map(|(_, was)| (*was).to_owned()),
                after: Some((*value).to_owned()),
            }),
        }
    }
    for (name, value) in &before {
        if !after.iter().any(|(other, _)| other == name) {
            changes.push(Change {
                name: (*name).to_owned(),
                before: Some((*value).to_owned()),
                after: None,
            });
        }
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));
    changes
}

/// Take the dump a config produces, or `None` when there is no tmux to ask.
///
/// The whole config is loaded in a server of its own, which is the cost this
/// module pays for being able to answer the question at all.
pub fn dump(config: &Path) -> Option<Dump> {
    dump_within(config, TIMEOUT)
}

/// The same, with the bound named.
///
/// A config whose `run-shell` blocks - verified on 3.6a to hold up
/// `new-session -d` for as long as the command takes - must not hold up a
/// `register` run. This is how long it gets.
pub fn dump_within(config: &Path, timeout: Duration) -> Option<Dump> {
    let server = Server::start_on(config, timeout)?;
    Dump::of(&server)
}

/// Ask tmux whether it can read a config at all.
///
/// `source-file` on a running server reports the file, the line and the reason
/// and exits non-zero - verified on 3.6a - and it is the only channel tmux
/// offers: fed the same config at server start, it abandons the whole file in
/// silence and exits 0. So the question is asked of a throwaway server that has
/// loaded nothing, which is also what makes the answer about *this* config
/// rather than about the user's running one.
///
/// `None` when there is no tmux to ask.
pub fn check(config: &Path) -> Option<Result<(), String>> {
    let server = Server::start_on(Path::new("/dev/null"), TIMEOUT)?;
    // Same again: the server started, so the only way past this `?` is a tmux
    // that stops answering between two calls, which the all-or-nothing fault
    // switch cannot stage. The marker exempts this whole line.
    let config = config.to_string_lossy();
    let (ok, complaint, also) = server.attempt(&["source-file", &config])?; // coverage: off
    Some(match ok {
        true => Ok(()),
        // Verified on 3.6a: the complaint comes back on *stdout*, not stderr.
        // Both are taken, because which one a given tmux uses is not something
        // to find out from a user's bug report.
        false => Err(format!("{}{}", complaint.trim(), also.trim())),
    })
}

/// Ask the running server to reload a config, the same command a user types.
///
/// Never `set-option`. By this point the probe has already loaded that exact
/// file in a throwaway server and found it sound, so the reload is offered on a
/// file that is known to parse rather than hoped to.
pub fn reload(config: &Path) -> io::Result<()> {
    match run_here(&["source-file", &config.to_string_lossy()]) {
        Some(_) => Ok(()),
        None => Err(io::Error::other(format!(
            "tmux source-file {} failed",
            config.display()
        ))),
    }
}

/// Maximum number of attempts to create a private directory.
const PRIVATE_DIR_ATTEMPTS: usize = 16;

/// A throwaway tmux server in a private directory, cleaned up on every exit
/// path.
///
/// Each server creates a `<PRIVATE_PREFIX>-<pid>-<n>` directory under `private_root()`
/// and runs as `-L probe` inside it. tmux never unlinks its own socket, so whoever
/// names a server must remove it. The private directory keeps the socket out
/// of the user's tmux socket dir entirely, and `Drop` or the background reaper
/// removes the directory with the server.
struct Server {
    /// The private directory holding this server's socket.
    private_dir: PathBuf,
    timeout: Duration,
    /// Set by `run_probe` on timeout: the background reaper owns the kill and
    /// the directory removal, so `Drop` must not attempt either.
    reaped: Cell<bool>,
}

impl Server {
    fn start_on(config: &Path, timeout: Duration) -> Option<Server> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let pid = std::process::id();

        // Create a private dir under /tmp. On AlreadyExists, take the next
        // slot, up to PRIVATE_DIR_ATTEMPTS attempts.
        let private_dir = {
            let mut attempts = 0;
            loop {
                let n = NEXT.fetch_add(1, Ordering::Relaxed);
                let dir = private_root().join(format!("{PRIVATE_PREFIX}-{pid}-{n}"));
                match fs::DirBuilder::new().mode(0o700).create(&dir) {
                    Ok(()) => break dir,
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        attempts += 1;
                        if attempts >= PRIVATE_DIR_ATTEMPTS {
                            return None;
                        }
                        continue;
                    }
                    Err(_) => return None, // coverage: off - cannot be staged without breaking /tmp
                }
            }
        };

        let server = Server {
            private_dir,
            timeout,
            reaped: Cell::new(false),
        };
        server.ask(&[
            "-f",
            &config.to_string_lossy(),
            "new-session",
            "-d",
            "-x",
            "80",
            "-y",
            "24",
        ])?;
        Some(server)
    }

    fn ask(&self, args: &[&str]) -> Option<String> {
        let (ok, out, _) = self.attempt(args)?;
        ok.then_some(out)
    }

    /// The same, but keeping what tmux had to say when it refused.
    fn attempt(&self, args: &[&str]) -> Option<(bool, String, String)> {
        let mut all = vec!["-L", "probe"];
        all.extend_from_slice(args);
        run_probe(&all, &self.private_dir, self.timeout, &self.reaped)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Once the reaped flag is set, a background reaper owns both the kill
        // and the directory removal: a second kill would wait out the timeout
        // again on a server still wedged in its config.
        if self.reaped.get() {
            return;
        }
        // Kill the server, then remove the private directory.
        let _ = run_probe(
            &["-L", "probe", "kill-server"],
            &self.private_dir,
            self.timeout,
            &self.reaped,
        );
        // If the kill itself timed out and set the reaped flag, the reaper now
        // owns the directory.
        if !self.reaped.get() {
            let _ = fs::remove_dir_all(&self.private_dir);
        } // coverage: off - reached by integration tests; the lib-test compilation never creates a Server
    }
}

/// Ask the tmux the *user* is running.
///
/// The environment is inherited, `$TMUX` included, because that is how tmux
/// finds the server the caller is actually in. Everything asked this way is a
/// question, never a probe.
fn run_here(args: &[&str]) -> Option<String> {
    let out = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run a tmux that belongs to us, bounded, in the environment a probe needs.
///
/// `TMUX_TMPDIR` is set to the private directory so the server's socket lands
/// there rather than in the user's socket dir. `$TMUX` is cleared so a probe
/// started from inside tmux cannot reach the server it is running in, and the
/// working directory is `$HOME` because tmux resolves a relative `source-file`
/// against the process's cwd rather than the config's - verified. The walk in
/// `tmux_conf` resolves one against the `Home` it is given, which in
/// production is the one `Home::from_env()` read of this same `$HOME`, so the
/// two read the same files, and reports that it had to guess.
fn run_probe(
    args: &[&str],
    private_dir: &Path,
    timeout: Duration,
    reaped: &Cell<bool>,
) -> Option<(bool, String, String)> {
    // The test-only switch, shared with the rest of `register`: a tmux that
    // stops answering partway through a sequence is a thing that happens and
    // nothing a test can arrange.
    if std::env::var("TMUX_AGENT_STATUS_TEST_FAULT")
        .is_ok_and(|value| value.split(',').any(|stage| stage == "no-tmux"))
    {
        return None;
    }
    let mut command = Command::new("tmux");
    command
        .args(args)
        .env("TMUX_TMPDIR", private_dir)
        .env_remove("TMUX")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A process with no `HOME` - a daemon, a `systemd` unit, a `su -c` - runs
    // the probe from wherever it already is rather than refusing to start.
    // `tests/register_no_home.rs` is the binary that reaches this.
    if let Some(home) = std::env::var_os("HOME") {
        command.current_dir(home);
    }
    let mut child = command.spawn().ok()?;

    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(delay);
                delay = delay.min(Duration::from_millis(500)).mul_f32(2.0);
            }
            // Out of time, or a wait that itself failed. A config whose
            // `run-shell` blocks holds up `new-session -d` for as long as the
            // command takes - verified on 3.6a - and must not hold up a
            // `register` run. Kill the client we started, then hand the server
            // and its private directory to a background reaper.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                reap(private_dir);
                reaped.set(true);
                return None;
            }
        }
    };
    // The child has already exited by here, so collecting what it wrote fails
    // only if the pipes themselves do. The marker exempts this whole line.
    let output = child.wait_with_output().ok()?; // coverage: off
    Some((
        status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
    // `wait_with_output` failing is the same "no answer" as everything else
    // above, which is what `None` means throughout this module.
}

/// Clear up a probe server we could not wait for, and remove its private
/// directory.
///
/// Not waited on, deliberately. The server is wedged executing its own config
/// and will not answer `kill-server` until it finishes, which is precisely the
/// wait we just refused to sit through. Spawning the kill-then-remove as a
/// single shell command means the `register` run is not held up and the server
/// and its directory still go, a moment later.
fn reap(private_dir: &Path) {
    let dir = private_dir.to_string_lossy();
    let _ = Command::new("sh")
        .args(["-c", "tmux -L probe kill-server; rm -rf \"$1\"", "sh", &dir])
        .env("TMUX_TMPDIR", private_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump_of(lines: &[&str]) -> Dump {
        let mut lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
        lines.sort();
        Dump { lines }
    }

    #[test]
    fn an_unchanged_dump_has_no_changes() {
        let dump = dump_of(&["status on", "window-status-format \"#I:#W\""]);
        assert!(changes(&dump, &dump).is_empty());
    }

    #[test]
    fn a_changed_value_is_reported_with_both_sides() {
        let before = dump_of(&["status on", "window-status-format \"#I:#W\""]);
        let after = dump_of(&["status on", "window-status-format \"#I:#W GLYPH\""]);

        assert_eq!(
            changes(&before, &after),
            vec![Change {
                name: "window-status-format".to_owned(),
                before: Some("\"#I:#W\"".to_owned()),
                after: Some("\"#I:#W GLYPH\"".to_owned()),
            }]
        );
    }

    #[test]
    fn an_appearing_and_a_vanishing_option_are_both_reported() {
        let before = dump_of(&["status on"]);
        let after = dump_of(&["session-window-changed[50] run-shell"]);

        let found = changes(&before, &after);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "session-window-changed[50]");
        assert_eq!(found[0].before, None);
        assert_eq!(found[1].name, "status");
        assert_eq!(found[1].after, None);
    }

    #[test]
    fn an_abandoned_config_shows_as_many_changes_rather_than_none() {
        // Which is the whole reason the comparison is over the dump and not
        // over the one option we care about.
        let before = dump_of(&[
            "status-left \"LEFT\"",
            "window-status-format \"#I:#W GOOD\"",
            "status on",
        ]);
        let defaults = dump_of(&[
            "status-left \"[#{session_name}] \"",
            "window-status-format \"#I:#W#{?window_flags,#{window_flags}, }\"",
            "status on",
        ]);

        assert_eq!(changes(&before, &defaults).len(), 2);
    }

    #[test]
    fn a_hooks_index_is_not_part_of_its_name() {
        // tmux reports an unset hook bare and a set one indexed, so one edit
        // shows up as two changes under two spellings of the same thing.
        assert_eq!(
            without_index("session-window-changed[50]"),
            "session-window-changed"
        );
        assert_eq!(
            without_index("session-window-changed"),
            "session-window-changed"
        );
        assert_eq!(
            without_index("window-status-format"),
            "window-status-format"
        );
    }

    #[test]
    fn a_name_is_looked_up_at_whatever_index_tmux_gave_it() {
        // The bare spelling sorts first, so taking the first match would read
        // an unset hook's empty value as the answer for the set one below it.
        let dump = dump_of(&[
            "session-window-changed",
            "session-window-changed[50] run-shell -b \"tmux-agent-status clear-pane\"",
            "window-status-format \"#I:#W\"",
        ]);
        assert!(dump.any_value("session-window-changed", |set| {
            set.contains("tmux-agent-status")
        }));
        assert!(dump.any_value("window-status-format", |value| value.contains("#I:#W")));
        assert!(!dump.any_value("window-status-format", |value| {
            value.contains("@agent_status")
        }));
        assert!(!dump.any_value("status-left", |_| true));
    }

    #[test]
    fn a_flag_option_with_no_value_still_has_a_name() {
        let dump = dump_of(&["some-flag"]);
        assert_eq!(dump.named(), vec![("some-flag", "")]);
    }
}
