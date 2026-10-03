//! A throwaway tmux server for tests that need a real one.
//!
//! Each server gets its own `tmux -L` socket, because `cargo test` is threaded
//! and two tests sharing a socket would interleave. The guard kills the server
//! and removes its socket file, so a panicking test cannot leak one. Test
//! files add their own helpers in further `impl Server` blocks.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
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
    try_output_within(command, limit).unwrap_or_else(|message| panic!("{message}"))
}

/// Like [`output_within`], returning the failure instead of panicking.
///
/// A command that cannot be started is also an `Err`. On a timeout the child
/// is killed, but its output readers are left behind: a grandchild that kept
/// the pipes open (a tmux server starting up) must not block the report.
pub fn try_output_within(mut command: Command, limit: Duration) -> Result<Output, String> {
    let line = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot start `{line}`: {err}"))?;
    let stdout = drain(child.stdout.take().expect("stdout is piped"));
    let stderr = drain(child.stderr.take().expect("stderr is piped"));

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
                return Err(format!(
                    "`{line}` was still running after {limit:?} and was killed"
                ));
            }
            Err(err) => return Err(format!("cannot wait for `{line}`: {err}")),
        }
    };
    let collect = |reader: std::thread::JoinHandle<Vec<u8>>| reader.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout: collect(stdout),
        stderr: collect(stderr),
    })
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
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
