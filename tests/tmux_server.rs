//! Everything that only a real tmux server can answer.
//!
//! Each test gets its own `tmux -L` socket, because `cargo test` is threaded
//! and two tests sharing a socket would interleave. Each server is killed by a
//! guard, so a panicking test cannot leak one.

use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-agent-status");

/// What an idle pane runs. The tool resolves panes from `$TMUX_PANE` and never
/// inspects processes, so a pane does not have to look like an agent.
const IDLE: &str = "sleep 300";

/// A throwaway tmux server holding one session `t`.
struct Server {
    socket: String,
    path: String,
}

impl Server {
    fn start() -> Server {
        Self::start_running(IDLE)
    }

    fn start_running(command: &str) -> Server {
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

    /// Run a tmux command on this server and return its stdout, asserting success.
    fn tmux(&self, args: &[&str]) -> String {
        let out = self.try_tmux(args);
        assert!(
            out.status.success(),
            "tmux {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("tmux printed valid utf-8")
    }

    fn try_tmux(&self, args: &[&str]) -> Output {
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

    fn socket_path(&self) -> String {
        self.tmux(&["display-message", "-p", "#{socket_path}"])
            .trim_end()
            .to_owned()
    }

    /// Run the binary as a hook would: inside this server, from this pane.
    fn agent_status(&self, pane: &str, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env("TMUX", format!("{},0,0", self.socket_path()))
            .env("TMUX_PANE", pane)
            .stdin(Stdio::null())
            .output()
            .expect("the binary runs")
    }

    /// A new window with one idle pane, returning that pane's id.
    ///
    /// Appended after the last window: `-a` against an occupied index moves the
    /// windows up and would reorder the status bar under the test's feet.
    fn new_window(&self, name: &str) -> String {
        self.tmux(&[
            "new-window",
            "-d",
            "-a",
            "-t",
            "t:{end}",
            "-n",
            name,
            "-P",
            "-F",
            "#{pane_id}",
        ])
        .trim_end()
        .to_owned()
    }

    /// A new pane in the same window, returning its id.
    fn split(&self, pane: &str) -> String {
        self.tmux(&[
            "split-window",
            "-d",
            "-t",
            pane,
            "-P",
            "-F",
            "#{pane_id}",
            IDLE,
        ])
        .trim_end()
        .to_owned()
    }

    /// A new window whose pane runs the setter itself, the way an agent hook
    /// does: `$TMUX` and `$TMUX_PANE` come from tmux, and the bell goes to that
    /// pane's tty.
    fn new_window_running_setter(&self, name: &str, state: &str) -> String {
        self.tmux(&[
            "new-window",
            "-d",
            "-a",
            "-t",
            "t:{end}",
            "-n",
            name,
            "-P",
            "-F",
            "#{window_id}",
            &format!("'{BIN}' set {state}; {IDLE}"),
        ])
        .trim_end()
        .to_owned()
    }

    fn first_pane(&self) -> String {
        self.tmux(&["list-panes", "-t", "t", "-F", "#{pane_id}"])
            .lines()
            .next()
            .expect("the session has a pane")
            .to_owned()
    }

    /// `@agent_pane_status` for every pane of `target`'s window, read the way
    /// the setter reads it.
    fn pane_statuses(&self, target: &str) -> Vec<String> {
        self.tmux(&["list-panes", "-t", target, "-F", "#{@agent_pane_status}"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The window rollup, empty when the option is unset.
    fn window_status(&self, target: &str) -> String {
        self.tmux(&["display-message", "-p", "-t", target, "#{@agent_status}"])
            .trim_end()
            .to_owned()
    }

    /// A second server whose only pane is a client attached to this one.
    ///
    /// Nothing draws and no pane ever gains focus without an attached client,
    /// so the rendering and focus tests need this sandwich; option-value tests
    /// do not.
    fn attach(&self) -> Server {
        let host = Server::start_running(&format!("tmux -L {} attach -t t", self.socket));
        wait_for(
            || self.tmux(&["list-clients", "-F", "#{client_name}"]),
            |clients| !clients.trim().is_empty(),
        );
        host
    }

    /// The documented format term, expanded for `target`.
    fn format_term(&self, target: &str) -> String {
        let expanded = self.tmux(&[
            "display-message",
            "-p",
            "-t",
            target,
            "[#{?@agent_status, #{@agent_status},}]",
        ]);
        expanded.trim_end().to_owned()
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
fn bin_dir_first_on_path() -> String {
    let dir = std::path::Path::new(BIN)
        .parent()
        .expect("the test binary has a directory");
    let inherited = std::env::var("PATH").unwrap_or_default();
    format!("{}:{inherited}", dir.display())
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "tmux-agent-status exited with {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "a hook wrote to stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn set_writes_the_pane_state_and_the_window_glyph() {
    let server = Server::start();
    let pane = server.first_pane();

    assert_ok(&server.agent_status(&pane, &["set", "done"]));

    assert_eq!(server.pane_statuses(&pane), ["done"]);
    assert_eq!(server.window_status(&pane), "✅");
}

#[test]
fn every_state_reaches_the_window_as_its_own_glyph() {
    let server = Server::start();
    let pane = server.first_pane();

    for (state, glyph) in [
        ("working", "🤖"),
        ("done", "✅"),
        ("error", "❗"),
        ("waiting", "💬"),
    ] {
        assert_ok(&server.agent_status(&pane, &["set", state]));
        assert_eq!(server.window_status(&pane), glyph, "state {state}");
    }
}

#[test]
fn two_panes_in_one_window_roll_up_to_the_higher_rank() {
    let server = Server::start();
    let first = server.first_pane();
    let second = server.split(&first);

    assert_ok(&server.agent_status(&first, &["set", "working"]));
    assert_ok(&server.agent_status(&second, &["set", "done"]));

    assert_eq!(server.pane_statuses(&first), ["working", "done"]);
    assert_eq!(server.window_status(&first), "✅");
}

#[test]
fn a_pane_with_no_state_reads_back_empty_while_the_window_has_one() {
    // The trap that forced two option names: with one name, the second pane
    // would read back the window's rollup and "unset" would be unreadable.
    let server = Server::start();
    let first = server.first_pane();
    let second = server.split(&first);

    assert_ok(&server.agent_status(&first, &["set", "waiting"]));

    assert_eq!(server.window_status(&second), "💬");
    assert_eq!(server.pane_statuses(&first), ["waiting", ""]);
}

#[test]
fn clear_window_clears_every_pane_but_keeps_working() {
    let server = Server::start();
    let first = server.first_pane();
    let second = server.split(&first);
    let third = server.split(&first);
    assert_ok(&server.agent_status(&first, &["set", "working"]));
    assert_ok(&server.agent_status(&second, &["set", "done"]));
    assert_ok(&server.agent_status(&third, &["set", "waiting"]));
    assert_eq!(server.window_status(&first), "💬");

    // Focusing the window clears the siblings too, not just the focused pane.
    assert_ok(&server.agent_status(&first, &["clear-window"]));

    let mut statuses = server.pane_statuses(&first);
    statuses.sort();
    assert_eq!(statuses, ["", "", "working"]);
    assert_eq!(server.window_status(&first), "🤖");
}

#[test]
fn clearing_the_last_state_unsets_the_window_option() {
    let server = Server::start();
    let pane = server.first_pane();
    assert_ok(&server.agent_status(&pane, &["set", "error"]));

    assert_ok(&server.agent_status(&pane, &["clear-window"]));

    assert_eq!(server.window_status(&pane), "");
    assert_eq!(server.format_term(&pane), "[]");
}

#[test]
fn the_format_term_renders_the_glyph_and_nothing_without_one() {
    let server = Server::start();
    let agent = server.first_pane();
    let stranger = server.new_window("no-agent-here");

    assert_ok(&server.agent_status(&agent, &["set", "waiting"]));

    assert_eq!(server.format_term(&agent), "[ 💬]");
    assert_eq!(server.format_term(&stranger), "[]");
}

#[test]
fn a_sibling_window_is_untouched() {
    let server = Server::start();
    let here = server.first_pane();
    let elsewhere = server.new_window("elsewhere");
    assert_ok(&server.agent_status(&elsewhere, &["set", "working"]));

    assert_ok(&server.agent_status(&here, &["set", "done"]));

    assert_eq!(server.window_status(&here), "✅");
    assert_eq!(server.window_status(&elsewhere), "🤖");
}

#[test]
fn the_status_bar_renders_the_glyph_after_a_truncated_name() {
    // Rendering needs an attached client, so a second server runs one.
    let test = Server::start();
    let term = "#{?@agent_status, #{@agent_status},}";
    let format =
        format!("#I:#{{=/10/…:#{{window_name}}}}{term}#{{?window_flags,#{{window_flags}}, }}");
    for option in ["window-status-format", "window-status-current-format"] {
        test.tmux(&["set-option", "-g", option, &format]);
    }
    test.tmux(&["set-option", "-g", "status-left", ""]);
    test.tmux(&["set-option", "-g", "status-right", ""]);
    test.tmux(&["rename-window", "-t", "t:0", "an-overlong-window-name"]);
    let current = test.first_pane();
    test.new_window("quiet");
    let other = test.new_window("other");
    assert_ok(&test.agent_status(&current, &["set", "done"]));
    assert_ok(&test.agent_status(&other, &["set", "waiting"]));

    let host = test.attach();
    let status_bar = wait_for(
        || {
            host.tmux(&["capture-pane", "-p", "-t", "t"])
                .lines()
                .next_back()
                .unwrap_or_default()
                .trim_end()
                .to_owned()
        },
        |line| line.contains("quiet"),
    );

    // The glyph sits outside the truncation, in both the current and the plain
    // window format, and the agent-free window renders exactly as it would
    // without this tool installed: the double space after `quiet` is its empty
    // flag placeholder followed by the status separator.
    assert_eq!(status_bar, "0:an-overlon… ✅* 1:quiet  2:other 💬");
}

#[test]
fn the_shipped_hooks_clear_a_window_when_it_is_looked_at() {
    let server = Server::start();
    let agent = server.first_pane();
    let elsewhere = server.new_window("elsewhere");
    server.tmux(&["source-file", "share/tmux/tmux-agent-status.conf"]);
    assert_ok(&server.agent_status(&agent, &["set", "done"]));
    assert_ok(&server.agent_status(&elsewhere, &["set", "waiting"]));

    // Switching windows: session-window-changed, carrying the new pane.
    server.tmux(&["select-window", "-t", "t:1"]);

    // The hooks run in the background, so the effect arrives a moment later.
    wait_for(
        || server.window_status(&elsewhere),
        |status| status.is_empty(),
    );
    // Only the window looked at; the one left behind keeps its glyph.
    assert_eq!(server.window_status(&agent), "✅");

    server.tmux(&["select-window", "-t", "t:0"]);

    wait_for(|| server.window_status(&agent), |status| status.is_empty());
}

#[test]
fn the_shipped_hooks_clear_a_window_when_another_pane_of_it_is_selected() {
    let server = Server::start();
    let first = server.first_pane();
    let second = server.split(&first);
    server.tmux(&["source-file", "share/tmux/tmux-agent-status.conf"]);
    assert_ok(&server.agent_status(&first, &["set", "error"]));

    // Switching panes inside the window: window-pane-changed.
    server.tmux(&["select-pane", "-t", &second]);

    wait_for(|| server.window_status(&first), |status| status.is_empty());
}

#[test]
fn clear_window_takes_the_pane_as_an_argument() {
    let server = Server::start();
    let pane = server.first_pane();
    let elsewhere = server.new_window("elsewhere");
    assert_ok(&server.agent_status(&pane, &["set", "done"]));

    // Addressed from a different pane entirely, the way a hook does it.
    assert_ok(&server.agent_status(&elsewhere, &["clear-window", &pane]));

    assert_eq!(server.window_status(&pane), "");
}

#[test]
fn a_turn_ending_state_rings_the_bell_of_its_window() {
    let server = Server::start();
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "other"]);

    let window = server.new_window_running_setter("ringer", "done");

    wait_for(
        || {
            server.tmux(&[
                "display-message",
                "-p",
                "-t",
                &window,
                "#{window_bell_flag}",
            ])
        },
        |flag| flag.trim() == "1",
    );
}

#[test]
fn working_does_not_ring() {
    // A bell on every PostToolUse is not a signal.
    let server = Server::start();
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "other"]);

    let window = server.new_window_running_setter("quiet", "working");

    // The state landing is proof the setter ran, and proof enough that no bell
    // is coming: the setter rings before it touches tmux at all.
    wait_for(|| server.window_status(&window), |status| status == "🤖");
    let flag = server.tmux(&[
        "display-message",
        "-p",
        "-t",
        &window,
        "#{window_bell_flag}",
    ]);
    assert_eq!(flag.trim(), "0");
}

#[test]
fn a_hook_outside_tmux_exits_zero_and_says_nothing() {
    for args in [
        ["set", "done"].as_slice(),
        ["clear-window"].as_slice(),
        ["clear-window", "%0"].as_slice(),
    ] {
        let out = Command::new(BIN)
            .args(args)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .stdin(Stdio::null())
            .output()
            .expect("the binary runs");
        assert_ok(&out);
        assert!(out.stderr.is_empty(), "{args:?} wrote to stderr");
    }
}

#[test]
fn an_unknown_state_is_loud() {
    let out = Command::new(BIN)
        .args(["set", "busy"])
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown state 'busy'"));
}

fn wait_for(read: impl Fn() -> String, done: impl Fn(&str) -> bool) -> String {
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
