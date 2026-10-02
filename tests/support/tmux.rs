//! A throwaway tmux server for tests that need a real one.
//!
//! Each server gets its own `tmux -L` socket, because `cargo test` is threaded
//! and two tests sharing a socket would interleave. The guard kills the server
//! and removes its socket file, so a panicking test cannot leak one. Test
//! files add their own helpers in further `impl Server` blocks.

use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

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
        Command::new("tmux")
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
            .stdin(Stdio::null())
            .output()
            .expect("tmux is on PATH")
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
        let _ = self.try_tmux(&["kill-server"]);
        let _ = std::fs::remove_file(&self.path);
    }
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
