//! A throwaway tmux server for tests that need a real one.
//!
//! Each server gets its own `tmux -L` socket, because `cargo test` is threaded
//! and two tests sharing a socket would interleave. The guard kills the server
//! and removes its socket file, so a panicking test cannot leak one. Test
//! files add their own helpers in further `impl Server` blocks.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long one tmux call may take before the test fails naming it.
///
/// A call takes milliseconds, and the whole suite about three seconds on a
/// laptop. A CI runner can be a lot slower and has been seen to stall for
/// minutes, so this sits far above any plausible latency while still turning
/// a hang into a failure that names the call that blocked.
pub const TMUX_TIMEOUT: Duration = Duration::from_secs(60);

/// What an idle pane runs: until its server is killed.
///
/// Never a time-limited command such as `sleep 300`. `Server` users can be
/// held up for a long time (a process-wide lock, a loaded CI runner), and a
/// pane that exits takes the whole server with it, failing every test that is
/// still waiting on one.
pub const IDLE: &str = "tail -f /dev/null";

/// A throwaway tmux server holding one session `t`.
pub struct Server {
    socket: String,
    path: String,
}

impl Server {
    /// A new server whose first pane runs `command`.
    pub fn start_running(command: &str) -> Server {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let socket = format!(
            "tmux-agent-status-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let mut server = Server {
            socket,
            path: String::new(),
        };
        // -f /dev/null: the user's own tmux.conf must not decide what a test sees.
        server.tmux(&[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "t",
            "-x",
            "100",
            "-y",
            "20",
            command,
        ]);
        server.path = server.socket_path();
        server
    }

    /// The `-L` socket name, for a client of this server.
    pub fn socket(&self) -> &str {
        &self.socket
    }

    /// Run a tmux command on this server and return its stdout, asserting success.
    pub fn tmux(&self, args: &[&str]) -> String {
        let out = self.try_tmux(args);
        assert!(
            out.status.success(),
            "tmux {args:?} failed: {}",
            super::stderr_of(&out)
        );
        String::from_utf8(out.stdout).expect("tmux printed valid utf-8")
    }

    pub fn try_tmux(&self, args: &[&str]) -> Output {
        output_within(self.tmux_command(args), TMUX_TIMEOUT)
    }

    /// The tmux invocation for `args` on this server, not yet run.
    fn tmux_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("tmux");
        command
            // -u: a client with no UTF-8 locale renders the glyphs as
            // underscores, and a build sandbox has no locale at all. The stored
            // option is unaffected; this is only about what a client sees.
            .arg("-u")
            .arg("-L")
            .arg(&self.socket)
            .args(args)
            // The server inherits this, so the shipped hook finds the binary
            // under test rather than an installed one, or nothing at all.
            .env("PATH", bin_dir_first_on_path())
            // tmux runs a pane's command as `$SHELL -c`, and a shell such as
            // zsh reads startup files (~/.zshenv) even then, which can rewrite
            // the PATH above. `/bin/sh -c` reads none.
            .env("SHELL", "/bin/sh")
            .stdin(Stdio::null());
        command
    }

    pub fn socket_path(&self) -> String {
        self.tmux(&["display-message", "-p", "#{socket_path}"])
            .trim_end()
            .to_owned()
    }

    /// One pane's own state, empty when it holds none.
    pub fn pane_status(&self, pane: &str) -> String {
        self.tmux(&["display-message", "-p", "-t", pane, "#{@agent_pane_status}"])
            .trim_end()
            .to_owned()
    }

    /// The window rollup, empty when the option is unset.
    pub fn window_status(&self, target: &str) -> String {
        self.tmux(&["display-message", "-p", "-t", target, "#{@agent_status}"])
            .trim_end()
            .to_owned()
    }

    /// Run `f` with the process environment aimed at this server, for calls
    /// into the library (`command::apply` and friends) the CLI does not cover.
    ///
    /// `$TMUX` is process-global while `cargo test` threads share it, so a
    /// lock serializes every such call; `f` may itself spawn threads inside,
    /// which then all see this server. The previous values are restored
    /// afterwards - a panicking `f` leaves them dirty, but the next holder
    /// of the lock rewrites them before any call anyway.
    pub fn in_process<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = env_lock();
        let saved: Vec<(&'static str, Option<OsString>)> = ENV_VARS
            .iter()
            .map(|var| (*var, std::env::var_os(var)))
            .collect();
        // SAFETY: every edit of these variables in this test process happens
        // under `env_lock()`, which `f` cannot outlast.
        unsafe {
            std::env::set_var("TMUX", format!("{},0,0", self.socket_path()));
            for var in &ENV_VARS[1..] {
                std::env::remove_var(var);
            }
        }
        let result = f();
        for (var, value) in saved {
            // SAFETY: the lock is still held.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(var, value),
                    None => std::env::remove_var(var),
                }
            }
        }
        result
    }
}

/// The environment the hook commands resolve a pane and a server from.
const ENV_VARS: [&str; 4] = [
    "TMUX",
    "TMUX_PANE",
    "TMUX_AGENT_STATUS_PANE",
    "TMUX_AGENT_STATUS_DISABLED",
];

fn env_lock() -> MutexGuard<'static, ()> {
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    // A panicking test must not poison the lock for the others.
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

impl Drop for Server {
    fn drop(&mut self) {
        // Best effort: a server that already exited is not a failure. tmux
        // leaves the socket file behind, so the guard takes that too.
        // Never panics: a panic in `drop` while a test is already unwinding
        // aborts the whole test binary.
        if let Err(stalled) = try_output_within(self.tmux_command(&["kill-server"]), TMUX_TIMEOUT) {
            eprintln!("{stalled}");
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Run `command` to completion and return its output, panicking with the
/// command line if it is still running after `limit`.
pub fn output_within(command: Command, limit: Duration) -> Output {
    try_output_within(command, limit).unwrap_or_else(|failure| panic!("{failure}"))
}

/// Why a call run under [`try_output_within`] failed.
///
/// The split is between "the program is not there" and "the program is
/// wedged": a probe such as [`super::tmux_or_skip`] may skip on the first but
/// must not on the second - a stalled call is the failure the bound exists to
/// name, and skipping on it would hide it behind a green suite.
#[derive(Debug)]
pub enum Failure {
    /// The program could not be spawned at all.
    NotStarted(String),
    /// It ran, but the call did not complete: it outlived its limit, exited
    /// while a spawned child kept its output open, or waiting on it errored.
    NotFinished(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::NotStarted(message) | Self::NotFinished(message)) = self;
        f.write_str(message)
    }
}

/// Like [`output_within`], with `input` written to the command's stdin, which
/// is then closed.
pub fn output_within_feeding(command: Command, input: &[u8], limit: Duration) -> Output {
    run_within(command, Some(input.to_vec()), limit).unwrap_or_else(|failure| panic!("{failure}"))
}

/// Like [`output_within`], returning the failure instead of panicking.
///
/// A command that cannot be started is an [`Failure::NotStarted`]. The limit
/// covers the whole call, output collection included: a grandchild that kept
/// a pipe open (a tmux server starting up) must not block the report, whether
/// its parent is still running or already exited.
pub fn try_output_within(command: Command, limit: Duration) -> Result<Output, Failure> {
    run_within(command, None, limit)
}

/// The bounded call behind every `*_within` function. With `input`, stdin is
/// piped and fed from a thread of its own, so a command that never reads it
/// cannot block the call past `limit` either.
fn run_within(
    mut command: Command,
    input: Option<Vec<u8>>,
    limit: Duration,
) -> Result<Output, Failure> {
    let line = std::iter::once(command.get_program())
        .chain(command.get_args())
        // Quote an argument that would smear into its neighbours - one that
        // is empty, holds whitespace, or holds a quote - so the message shows
        // where one argument ends and the next begins. `{:?}` escapes rather
        // than wraps, so a quote inside stays unambiguous.
        .map(|part| match part.to_string_lossy() {
            part if part.is_empty()
                || part
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '\'' | '"')) =>
            {
                format!("{part:?}")
            }
            part => part.into_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| Failure::NotStarted(format!("cannot start `{line}`: {err}")))?;
    let stdout = drain(child.stdout.take().expect("stdout is piped"));
    let stderr = drain(child.stderr.take().expect("stderr is piped"));
    if let Some(input) = input {
        let mut stdin = child.stdin.take().expect("stdin is piped");
        // Dropping `stdin` at the end closes it, ending the command's input.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }

    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::NotFinished(format!(
                    "`{line}` was still running after {limit:?} and was killed"
                )));
            }
            Err(err) => {
                return Err(Failure::NotFinished(format!(
                    "cannot wait for `{line}`: {err}"
                )));
            }
        }
    };
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let (Ok(stdout), Ok(stderr)) = (
        stdout.recv_timeout(remaining()),
        stderr.recv_timeout(remaining()),
    ) else {
        return Err(Failure::NotFinished(format!(
            "`{line}` exited, but its output was still open after {limit:?}"
        )));
    };
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn drain(mut pipe: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    rx
}

/// `PATH` with the binary under test in front.
pub fn bin_dir_first_on_path() -> String {
    let dir = std::path::Path::new(super::BIN)
        .parent()
        .expect("the test binary has a directory");
    let inherited = std::env::var("PATH").unwrap_or_default();
    format!("{}:{inherited}", dir.display())
}

/// Poll `read` until `done` accepts its value, failing after ten seconds.
pub fn wait_for(read: impl Fn() -> String, done: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let value = read();
        if done(&value) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting; last read: {value:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
