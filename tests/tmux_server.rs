//! Everything that only a real tmux server can answer.
//!
//! Each test gets its own `tmux -L` socket, because `cargo test` is threaded
//! and two tests sharing a socket would interleave. Each server is killed by a
//! guard, so a panicking test cannot leak one.

use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

mod support;

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
            support::stderr_of(&out)
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
        Command::new(support::BIN)
            .args(args)
            .env("TMUX", format!("{},0,0", self.socket_path()))
            .env("TMUX_PANE", pane)
            .stdin(Stdio::null())
            .output()
            .expect("the binary runs")
    }

    /// Run the binary as a hook would, without waiting for it to finish.
    fn spawn_agent_status(&self, pane: &str, args: &[&str]) -> std::process::Child {
        Command::new(support::BIN)
            .args(args)
            .env("TMUX", format!("{},0,0", self.socket_path()))
            .env("TMUX_PANE", pane)
            .stdin(Stdio::null())
            .spawn()
            .expect("the binary runs")
    }

    /// Put a value in a pane's status behind the tool's back, so a test can
    /// arrange what a pane already holds without going through the policy.
    fn put_status(&self, pane: &str, value: &str) {
        match value {
            "" => self.tmux(&["set-option", "-p", "-u", "-t", pane, "@agent_pane_status"]),
            value => self.tmux(&["set-option", "-p", "-t", pane, "@agent_pane_status", value]),
        };
    }

    /// Every option set on the pane, to tell an unset one from an empty one.
    fn pane_options(&self, pane: &str) -> String {
        self.tmux(&["show-options", "-p", "-t", pane])
    }

    /// Every option set on the pane's window.
    fn window_options(&self, target: &str) -> String {
        self.tmux(&["show-options", "-w", "-t", target])
    }

    /// Run the binary with no $TMUX_PANE, using $TMUX_AGENT_STATUS_PANE instead.
    fn agent_status_pane_env(&self, pane: &str, args: &[&str]) -> Output {
        Command::new(support::BIN)
            .args(args)
            .env("TMUX", format!("{},0,0", self.socket_path()))
            .env("TMUX_AGENT_STATUS_PANE", pane)
            .env_remove("TMUX_PANE")
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

    /// A new window whose pane runs the command itself, the way an agent hook
    /// does: `$TMUX` and `$TMUX_PANE` come from tmux, and any bell goes to that
    /// pane's tty.
    fn new_window_running_command(&self, name: &str, arguments: &str) -> String {
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
            &format!("'{}' {arguments}; {IDLE}", support::BIN),
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

    /// The two facts that together mean "on screen": current window, and a
    /// client attached to look at it.
    fn window_active_and_attached(&self, target: &str) -> String {
        self.tmux(&[
            "display-message",
            "-p",
            "-t",
            target,
            "#{window_active} #{?session_attached,1,0}",
        ])
        .trim_end()
        .to_owned()
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
    let dir = std::path::Path::new(support::BIN)
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
        support::stderr_of(out)
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
fn notify_payload_sets_window_status_via_agent_mapping() {
    // Shape B end-to-end: a JSON payload from an agent hook maps to a state.
    let server = Server::start();
    let pane = server.first_pane();
    let payload = r#"{"hook_event_name":"pre_tool"}"#;

    assert_ok(&server.agent_status(&pane, &["notify", "--agent", "mistral-vibe", payload]));

    assert_eq!(server.pane_statuses(&pane), ["working"]);
    assert_eq!(server.window_status(&pane), "🤖");
}

#[test]
fn pane_flag_overrides_missing_tmux_pane() {
    let server = Server::start();
    let pane = server.first_pane();

    assert_ok(&server.agent_status_pane_env(&pane, &["set", "done"]));

    assert_eq!(server.pane_statuses(&pane), ["done"]);
    assert_eq!(server.window_status(&pane), "✅");
}

#[test]
fn pane_flag_overrides_tmux_pane() {
    let server = Server::start();
    let first = server.first_pane();
    let second = server.split(&first);

    assert_ok(&server.agent_status(&first, &["set", "done", "--pane", second.as_str()]));

    assert_eq!(server.pane_statuses(&first), ["", "done"]);
    assert_eq!(server.window_status(&first), "✅");
}

#[test]
fn a_state_on_the_current_window_of_an_attached_session_is_painted() {
    // tmux cannot tell a focused terminal from a background tab, so being the
    // current window of an attached session does not mean anyone is looking:
    // the glyph is written and only a focus event on the pane clears it.
    let server = Server::start();
    let pane = server.first_pane();
    let _client = server.attach();
    wait_for(
        || server.window_active_and_attached(&pane),
        |seen| seen == "1 1",
    );

    assert_ok(&server.agent_status(&pane, &["set", "done"]));

    assert_eq!(server.pane_statuses(&pane), ["done"]);
    assert_eq!(server.window_status(&pane), "\u{2705}");
}

#[test]
fn a_report_on_an_attached_window_touches_no_other_pane() {
    let server = Server::start();
    let reporter = server.first_pane();
    let finished = server.split(&reporter);
    let busy = server.split(&reporter);
    assert_ok(&server.agent_status(&reporter, &["set", "working"]));
    assert_ok(&server.agent_status(&finished, &["set", "done"]));
    assert_ok(&server.agent_status(&busy, &["set", "working"]));
    let _client = server.attach();
    wait_for(
        || server.window_active_and_attached(&reporter),
        |seen| seen == "1 1",
    );

    assert_ok(&server.agent_status(&reporter, &["set", "waiting"]));

    // The siblings keep what they held, on screen or not.
    let mut statuses = server.pane_statuses(&reporter);
    statuses.sort();
    assert_eq!(statuses, ["done", "waiting", "working"]);
    assert_eq!(server.window_status(&reporter), "\u{1f4ac}");
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
        // Reported onto an empty pane: a pane that already holds a state can
        // refuse a lower one, which `a_state_is_refused_if_it_ranks_lower` covers.
        assert_ok(&server.agent_status(&pane, &["reset"]));
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
fn reset_clears_only_its_pane_and_recomputes_the_rollup() {
    let server = Server::start();
    let reset = server.first_pane();
    let sibling = server.split(&reset);
    assert_ok(&server.agent_status(&reset, &["set", "waiting"]));
    assert_ok(&server.agent_status(&sibling, &["set", "working"]));

    assert_ok(&server.agent_status(&reset, &["reset"]));

    assert_eq!(server.pane_statuses(&reset), ["", "working"]);
    assert_eq!(server.window_status(&reset), "🤖");
    assert_ok(&server.agent_status(&reset, &["reset"]));
    assert_eq!(server.pane_statuses(&reset), ["", "working"]);
}

#[test]
fn finish_resolves_session_states_but_preserves_error() {
    let server = Server::start();
    let pane = server.first_pane();

    for initial in [Some("working"), Some("waiting"), None] {
        assert_ok(&server.agent_status(&pane, &["reset"]));
        if let Some(state) = initial {
            assert_ok(&server.agent_status(&pane, &["set", state]));
        }
        assert_ok(&server.agent_status(&pane, &["finish"]));
        assert_eq!(server.pane_statuses(&pane), ["done"], "initial {initial:?}");
        assert_eq!(server.window_status(&pane), "✅", "initial {initial:?}");
    }

    assert_ok(&server.agent_status(&pane, &["set", "error"]));
    assert_ok(&server.agent_status(&pane, &["finish"]));
    assert_eq!(server.pane_statuses(&pane), ["error"]);
    assert_eq!(server.window_status(&pane), "❗");
}

#[test]
fn finish_recomputes_a_window_with_a_higher_ranked_sibling() {
    let server = Server::start();
    let finishing = server.first_pane();
    let sibling = server.split(&finishing);
    assert_ok(&server.agent_status(&finishing, &["set", "working"]));
    assert_ok(&server.agent_status(&sibling, &["set", "waiting"]));

    assert_ok(&server.agent_status(&finishing, &["finish"]));

    assert_eq!(server.pane_statuses(&finishing), ["done", "waiting"]);
    assert_eq!(server.window_status(&finishing), "💬");
}

#[test]
fn finish_on_an_attached_window_still_paints_its_glyph() {
    // What `/clear` does to a stranded `working`: the session ends and the
    // pane is resolved to `done`, which stays until the pane is acknowledged.
    let server = Server::start();
    let pane = server.first_pane();
    assert_ok(&server.agent_status(&pane, &["set", "working"]));
    let _client = server.attach();
    wait_for(
        || server.window_active_and_attached(&pane),
        |seen| seen == "1 1",
    );

    assert_ok(&server.agent_status(&pane, &["finish"]));

    assert_eq!(server.pane_statuses(&pane), ["done"]);
    assert_eq!(server.window_status(&pane), "\u{2705}");

    // The `SessionStart` of the successor session then clears it.
    assert_ok(&server.agent_status(&pane, &["reset"]));
    assert_eq!(server.pane_statuses(&pane), [""]);
    assert_eq!(server.window_status(&pane), "");
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

    let window = server.new_window_running_command("ringer", "set done");

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

    let window = server.new_window_running_command("quiet", "set working");

    // The state landing is proof the setter ran, and `working` rings for no
    // pane state at all, so nothing is racing a later bell.
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
fn finish_does_not_ring() {
    let server = Server::start();
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "other"]);

    let commands = format!("set working; '{}' finish", support::BIN);
    let window = server.new_window_running_command("quiet-finish", &commands);

    wait_for(|| server.window_status(&window), |status| status == "✅");
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
        ["reset"].as_slice(),
        ["finish"].as_slice(),
        ["clear-window"].as_slice(),
        ["clear-window", "%0"].as_slice(),
    ] {
        let out = Command::new(support::BIN)
            .args(args)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .stdin(Stdio::null())
            .output()
            .expect("the binary runs");
        assert_ok(&out);
        assert!(
            support::stderr_of(&out).is_empty(),
            "{args:?} wrote to stderr"
        );
    }
}

#[test]
fn a_hook_with_only_tmux_pane_exits_zero_and_says_nothing() {
    let out = Command::new(support::BIN)
        .args(["set", "done"])
        .env_remove("TMUX")
        .env("TMUX_PANE", "%0")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    assert_ok(&out);
    assert!(support::stderr_of(&out).is_empty());
}

#[test]
fn an_unknown_state_is_loud() {
    let out = Command::new(support::BIN)
        .args(["set", "busy"])
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    assert_eq!(out.status.code(), Some(2));
    assert!(support::stderr_of(&out).contains("unknown state 'busy'"));
}

#[test]
fn a_state_is_refused_if_it_ranks_lower_than_the_one_the_pane_holds() {
    // The table 013 decides: within a pane `error` > `done` > `waiting` >
    // `working`, and a value this tool does not recognise is replaced by any of
    // them. The glyph follows the pane, since it is the only pane here.
    let server = Server::start();
    let pane = server.first_pane();

    for (held, reported, expected) in [
        ("", "working", "working"),
        ("", "waiting", "waiting"),
        ("", "done", "done"),
        ("", "error", "error"),
        ("working", "working", "working"),
        ("working", "waiting", "waiting"),
        ("working", "done", "done"),
        ("working", "error", "error"),
        ("waiting", "working", "waiting"),
        ("waiting", "waiting", "waiting"),
        ("waiting", "done", "done"),
        ("waiting", "error", "error"),
        ("done", "working", "done"),
        ("done", "waiting", "done"),
        ("done", "done", "done"),
        ("done", "error", "error"),
        ("error", "working", "error"),
        ("error", "waiting", "error"),
        ("error", "done", "error"),
        ("error", "error", "error"),
        ("busy", "working", "working"),
        ("busy", "waiting", "waiting"),
        ("busy", "done", "done"),
        ("busy", "error", "error"),
    ] {
        server.put_status(&pane, held);

        assert_ok(&server.agent_status(&pane, &["set", reported]));

        let case = format!("{held:?} then set {reported}");
        assert_eq!(server.pane_statuses(&pane), [expected], "{case}");
        assert_eq!(server.window_status(&pane), glyph_of(expected), "{case}");
    }
}

#[test]
fn a_sibling_report_cannot_lower_a_state_that_outranks_it() {
    // The defect 013 fixes, as a race: an agent runs the hooks of one turn
    // concurrently, so the `working` of a tool that finished arrives while the
    // prompt of the tool that is blocked is still open.
    let server = Server::start();
    let pane = server.first_pane();
    assert_ok(&server.agent_status(&pane, &["reset"]));

    let mut running: Vec<_> = (0..20)
        .map(|_| server.spawn_agent_status(&pane, &["set", "working"]))
        .collect();
    running.push(server.spawn_agent_status(&pane, &["set", "waiting"]));
    running.extend((0..20).map(|_| server.spawn_agent_status(&pane, &["set", "working"])));
    for mut child in running {
        assert!(child.wait().expect("the binary exits").success());
    }

    assert_eq!(server.pane_statuses(&pane), ["waiting"]);
    assert_eq!(server.window_status(&pane), "💬");
}

#[test]
fn the_window_shows_the_highest_ranked_state_of_its_panes() {
    // The rollup rank is unchanged by 013 and is not the pane precedence:
    // `waiting` > `error` > `done` > `working`, so a window holding one
    // finished and one blocked pane asks you to come to the blocked one.
    let server = Server::start();
    let first = server.first_pane();
    let second = server.split(&first);

    for held in ["", "working", "done", "error", "waiting"] {
        for reported in ["", "working", "done", "error", "waiting"] {
            server.put_status(&first, held);
            server.put_status(&second, "");
            let case = format!("{held:?} beside {reported:?}");

            // Reported through the tool, so the rollup is what the tool wrote.
            match reported {
                "" => assert_ok(&server.agent_status(&second, &["reset"])),
                state => assert_ok(&server.agent_status(&second, &["set", state])),
            }

            let expected = if rollup_rank(held) >= rollup_rank(reported) {
                held
            } else {
                reported
            };
            assert_eq!(server.window_status(&first), glyph_of(expected), "{case}");
        }
    }
}

#[test]
fn a_refused_state_still_rings() {
    // The glyph cannot say "blocked on you" while the pane holds a `done`
    // nobody has looked at, so the bell is the only channel that can. It rings
    // before tmux is touched at all.
    let server = Server::start();
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "other"]);
    let agent = server.first_pane();
    assert_ok(&server.agent_status(&agent, &["set", "done"]));

    // From another window, so the bell would land somewhere this can read, and
    // saying so through tmux is how the test knows the command ran at all.
    // `-t "$TMUX_PANE"` because a bare `set-option -w` writes to the session's
    // active window, and this one is created detached.
    let ringer = server.new_window_running_command(
        "refused",
        &format!(r#"set waiting --pane {agent}; tmux set-option -w -t "$TMUX_PANE" @probe_ran 1"#),
    );
    wait_for(
        || server.tmux(&["display-message", "-p", "-t", &ringer, "#{@probe_ran}"]),
        |ran| ran.trim() == "1",
    );

    assert_eq!(server.pane_statuses(&agent), ["done"]);
    assert_eq!(server.window_status(&agent), "✅");
    let flag = server.tmux(&[
        "display-message",
        "-p",
        "-t",
        &ringer,
        "#{window_bell_flag}",
    ]);
    assert_eq!(flag.trim(), "1", "a refused state must still ring");
}

#[test]
fn start_replaces_whatever_the_last_turn_left() {
    // The hole precedence opens: a `done` written while you were away is only
    // cleared by a window or pane change, so reattaching onto the window it is
    // already on leaves it there, and it outranks every state of the next turn.
    // Typing a prompt is seeing the pane, and `start` says so.
    let server = Server::start();
    let pane = server.first_pane();

    for held in ["", "working", "waiting", "done", "error", "busy"] {
        server.put_status(&pane, held);

        assert_ok(&server.agent_status(&pane, &["start"]));

        assert_eq!(server.pane_statuses(&pane), ["working"], "held {held:?}");
        assert_eq!(server.window_status(&pane), "🤖", "held {held:?}");
    }
}

#[test]
fn a_cleared_option_is_unset_rather_than_empty() {
    // Every write is a format, and a format can only produce a value, so a
    // clear leaves an empty string behind unless it is normalised away. An
    // empty option would read the same through a format but show up in
    // `show-options`, and 001 promises a window with no agent carries none.
    let server = Server::start();
    let pane = server.first_pane();
    let bare = server.new_window("no-agent-here");
    assert_ok(&server.agent_status(&pane, &["set", "error"]));

    assert_ok(&server.agent_status(&pane, &["clear-window"]));

    assert!(
        !server.pane_options(&pane).contains("@agent_pane_status"),
        "pane options: {}",
        server.pane_options(&pane)
    );
    assert!(
        !server.window_options(&pane).contains("@agent_status"),
        "window options: {}",
        server.window_options(&pane)
    );

    // And a window this tool has never had anything to say about stays clean.
    assert_ok(&server.agent_status(&bare, &["clear-window"]));
    assert!(
        !server.window_options(&bare).contains("@agent_status"),
        "window options: {}",
        server.window_options(&bare)
    );
    assert!(
        !server.pane_options(&bare).contains("@agent_pane_status"),
        "pane options: {}",
        server.pane_options(&bare)
    );
}

/// The glyph of a state name, empty for no state and for a value this tool does
/// not recognise. Spelled out rather than read from the crate: these tests are
/// the outside view.
fn glyph_of(state: &str) -> &'static str {
    match state {
        "working" => "🤖",
        "done" => "✅",
        "error" => "❗",
        "waiting" => "💬",
        _ => "",
    }
}

/// The rank the window rollup reduces by, 0 for no state.
fn rollup_rank(state: &str) -> u8 {
    match state {
        "waiting" => 4,
        "error" => 3,
        "done" => 2,
        "working" => 1,
        _ => 0,
    }
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
