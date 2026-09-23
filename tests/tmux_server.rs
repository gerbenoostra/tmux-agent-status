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

    /// Whether the window is its session's current one and a client is
    /// attached: the precondition the tests wait for. tmux cannot tell that
    /// from anyone looking, so it says nothing about attention.
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

    /// One pane's own state, empty when it holds none.
    fn pane_status(&self, pane: &str) -> String {
        self.tmux(&["display-message", "-p", "-t", pane, "#{@agent_pane_status}"])
            .trim_end()
            .to_owned()
    }

    /// Source the shipped snippet, the way a user's `tmux.conf` does. The path is
    /// relative to the crate root, which is where `cargo test` runs.
    fn source_snippet(&self) {
        self.tmux(&["source-file", "share/tmux/tmux-agent-status.conf"]);
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

    /// A three-servers-deep sandwich, so a pane on `self` sees real terminal
    /// focus rather than just an active-pane change inside one server: a
    /// level-0 detached server (`outer`) supplies the pty for a level-1
    /// host's client, the host has a second window to switch to, and the
    /// host's first window is the client attached to `self`. Switching the
    /// host's window away and back drops and restores that client's
    /// `focused` flag, forwarding `pane-focus-out`/`pane-focus-in` down onto
    /// `self`'s attached pane - probed on tmux 3.6a by nesting exactly this
    /// way. Both the host and `self` need `focus-events on` for the
    /// forwarding to happen at all (probed on tmux 3.6a: with it off on
    /// either server, the inner client's `focused` flag never moves); the
    /// caller is responsible for `self`'s side (typically `source_snippet`).
    /// `outer` is never attached to, which is what keeps its own client
    /// permanently focused and makes it stand in for level 0.
    fn nested_client(&self) -> (Server, Server) {
        let host = self.attach();
        host.new_window("elsewhere");
        host.tmux(&["set-option", "-g", "focus-events", "on"]);
        let outer = host.attach();
        (host, outer)
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
fn clear_pane_clears_only_its_pane_and_leaves_a_sibling_alone() {
    let server = Server::start();
    let seen = server.first_pane();
    let sibling = server.split(&seen);
    assert_ok(&server.agent_status(&seen, &["set", "done"]));
    assert_ok(&server.agent_status(&sibling, &["set", "waiting"]));

    assert_ok(&server.agent_status(&seen, &["clear-pane"]));

    assert_eq!(server.pane_statuses(&seen), ["", "waiting"]);
    // The window falls back to what is left, and a sibling on screen stays.
    assert_eq!(server.window_status(&seen), "💬");
}

#[test]
fn clear_pane_keeps_a_working_pane_working() {
    let server = Server::start();
    let pane = server.first_pane();
    assert_ok(&server.agent_status(&pane, &["set", "working"]));

    assert_ok(&server.agent_status(&pane, &["clear-pane"]));

    assert_eq!(server.pane_statuses(&pane), ["working"]);
    assert_eq!(server.window_status(&pane), "🤖");
}

#[test]
fn clear_pane_clears_waiting_and_error_and_the_window_option_with_them() {
    let server = Server::start();
    let pane = server.first_pane();
    for state in ["waiting", "error"] {
        assert_ok(&server.agent_status(&pane, &["set", state]));

        assert_ok(&server.agent_status(&pane, &["clear-pane"]));

        assert_eq!(server.pane_statuses(&pane), [""], "{state}");
        assert!(
            !server.pane_options(&pane).contains("@agent_pane_status"),
            "{state}"
        );
        assert!(
            !server.window_options(&pane).contains("@agent_status"),
            "{state}"
        );
    }
}

#[test]
fn clear_pane_on_a_pane_without_a_state_changes_nothing() {
    let server = Server::start();
    let bare = server.first_pane();
    let sibling = server.split(&bare);
    assert_ok(&server.agent_status(&sibling, &["set", "done"]));

    assert_ok(&server.agent_status(&bare, &["clear-pane"]));

    assert_eq!(server.pane_statuses(&bare), ["", "done"]);
    assert_eq!(server.window_status(&bare), "✅");
}

#[test]
fn clear_pane_heals_a_window_glyph_left_stale() {
    let server = Server::start();
    let pane = server.first_pane();
    // A glyph no pane backs, as a failed write between the two would leave.
    server.tmux(&["set-option", "-w", "-t", &pane, "@agent_status", "💬"]);

    assert_ok(&server.agent_status(&pane, &["clear-pane"]));

    assert_eq!(server.window_status(&pane), "");
}

#[test]
fn clear_pane_takes_the_pane_as_an_argument() {
    let server = Server::start();
    let pane = server.first_pane();
    let elsewhere = server.new_window("elsewhere");
    assert_ok(&server.agent_status(&pane, &["set", "done"]));
    assert_ok(&server.agent_status(&elsewhere, &["set", "waiting"]));

    // Addressed from a different pane entirely, the way a hook does it.
    assert_ok(&server.agent_status(&elsewhere, &["clear-pane", &pane]));

    assert_eq!(server.window_status(&pane), "");
    assert_eq!(server.pane_statuses(&elsewhere), ["waiting"]);
    assert_eq!(server.window_status(&elsewhere), "💬");
}

#[test]
fn clear_pane_of_a_pane_that_has_closed_writes_nothing() {
    // A focus hook can name a pane that is gone by the time the binary runs.
    let server = Server::start();
    let survivor = server.first_pane();
    let closing = server.split(&survivor);
    let elsewhere = server.new_window("elsewhere");
    assert_ok(&server.agent_status(&survivor, &["set", "waiting"]));
    assert_ok(&server.agent_status(&elsewhere, &["set", "done"]));
    server.tmux(&["kill-pane", "-t", &closing]);

    let out = server.agent_status(&elsewhere, &["clear-pane", &closing]);

    assert_ok(&out);
    assert!(support::stderr_of(&out).is_empty());
    assert_eq!(server.pane_statuses(&survivor), ["waiting"]);
    assert_eq!(server.pane_statuses(&elsewhere), ["done"]);
    assert_eq!(server.window_status(&survivor), "💬");
    assert_eq!(server.window_status(&elsewhere), "✅");
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

    assert_ok(&server.agent_status(&pane, &["clear-pane"]));

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
fn the_shipped_snippet_registers_the_option_and_all_three_hooks() {
    let server = Server::start();

    server.source_snippet();

    assert_eq!(
        server.tmux(&["show-options", "-gv", "focus-events"]).trim(),
        "on"
    );
    // Each hook in the scope tmux keeps it in.
    let global = server.tmux(&["show-hooks", "-g"]);
    assert!(global.contains("session-window-changed[50]"), "{global}");
    let window = server.tmux(&["show-hooks", "-gw"]);
    for hook in ["pane-focus-in[50]", "window-pane-changed[50]"] {
        assert!(window.contains(hook), "{hook}: {window}");
    }
}

#[test]
fn a_pane_focus_in_clears_the_pane_it_names_and_no_other() {
    // The hook that sees the terminal, on its own: the two fallbacks are
    // removed, so only `pane-focus-in` can be what clears anything here.
    let server = Server::start();
    let here = server.first_pane();
    let target = server.new_window("target");
    let sibling = server.split(&target);
    let _client = server.attach();
    server.source_snippet();
    for hook in ["session-window-changed[50]", "window-pane-changed[50]"] {
        server.tmux(&["set-hook", "-gu", hook]);
    }
    assert_ok(&server.agent_status(&here, &["set", "done"]));
    assert_ok(&server.agent_status(&target, &["set", "done"]));
    assert_ok(&server.agent_status(&sibling, &["set", "waiting"]));
    assert_eq!(server.window_status(&target), "\u{1f4ac}");

    // The client moves onto the window, and tmux focuses the pane it lands on.
    server.tmux(&["select-window", "-t", "t:1"]);

    wait_for(|| server.pane_status(&target), |status| status.is_empty());
    // The visible sibling was not read, the window glyph is recomputed from it,
    // and the window left behind keeps its own.
    assert_eq!(server.pane_status(&sibling), "waiting");
    assert_eq!(server.window_status(&target), "\u{1f4ac}");
    assert_eq!(server.pane_status(&here), "done");
    assert_eq!(server.window_status(&here), "\u{2705}");
}

#[test]
fn without_focus_events_a_switch_still_clears_the_pane_it_lands_on() {
    // No client and the option off: `pane-focus-in` cannot fire, so this is the
    // fallback pair on its own.
    let server = Server::start();
    let here = server.first_pane();
    let target = server.new_window("target");
    let sibling = server.split(&target);
    server.source_snippet();
    server.tmux(&["set-option", "-g", "focus-events", "off"]);
    assert_ok(&server.agent_status(&here, &["set", "done"]));
    assert_ok(&server.agent_status(&target, &["set", "waiting"]));
    assert_ok(&server.agent_status(&sibling, &["set", "error"]));

    // Switching windows: session-window-changed, carrying the new pane.
    server.tmux(&["select-window", "-t", "t:1"]);

    wait_for(|| server.pane_status(&target), |status| status.is_empty());
    // The pane left behind and the sibling on the same window keep theirs.
    assert_eq!(server.pane_status(&here), "done");
    assert_eq!(server.pane_status(&sibling), "error");
    assert_eq!(server.window_status(&target), "\u{2757}");

    // Switching panes inside the window: window-pane-changed.
    server.tmux(&["select-pane", "-t", &sibling]);

    wait_for(|| server.pane_status(&sibling), |status| status.is_empty());
    assert_eq!(server.pane_status(&here), "done");
    wait_for(|| server.window_status(&target), |status| status.is_empty());
}

/// Attach a client with the snippet sourced and `focus-events` set to `mode`,
/// then detach it and attach another: the pane the first attach lands on is
/// acknowledged, every other pane and window keeps its state through both.
///
/// Only the first attach fires `pane-focus-in` (probed on tmux 3.6a, in both
/// modes and for a dying as well as a detached client): tmux keeps the pane
/// flagged as focused across a detach, so a later attach finds nothing to
/// announce. The second half therefore pins only that nothing else moves, and
/// not that the landing pane clears again.
fn attach_cycle_with_focus_events(mode: &str) {
    let server = Server::start();
    let landing = server.first_pane();
    let beside = server.split(&landing);
    let elsewhere = server.new_window("elsewhere");
    server.source_snippet();
    server.tmux(&["set-option", "-g", "focus-events", mode]);
    assert_ok(&server.agent_status(&landing, &["set", "done"]));
    assert_ok(&server.agent_status(&beside, &["set", "error"]));
    assert_ok(&server.agent_status(&elsewhere, &["set", "waiting"]));

    let first = server.attach();

    // Attaching fires `pane-focus-in` for the pane it lands on, whatever
    // `focus-events` says.
    wait_for(|| server.pane_status(&landing), |status| status.is_empty());
    wait_for(
        || server.window_status(&landing),
        |status| status == "\u{2757}",
    );
    assert_eq!(server.pane_status(&beside), "error");
    assert_eq!(server.pane_status(&elsewhere), "waiting");

    drop(first);
    wait_for(
        || server.tmux(&["list-clients", "-F", "#{client_name}"]),
        |clients| clients.trim().is_empty(),
    );
    // Detaching acknowledges nothing.
    assert_eq!(server.pane_status(&beside), "error");
    assert_eq!(server.pane_status(&elsewhere), "waiting");

    let _second = server.attach();

    assert_eq!(server.pane_status(&beside), "error");
    assert_eq!(server.pane_status(&elsewhere), "waiting");
    assert_eq!(server.window_status(&elsewhere), "\u{1f4ac}");
}

#[test]
fn an_attach_clears_only_the_pane_it_lands_on() {
    attach_cycle_with_focus_events("on");
}

#[test]
fn an_attach_clears_only_the_pane_it_lands_on_with_focus_events_off() {
    attach_cycle_with_focus_events("off");
}

#[test]
fn losing_and_regaining_terminal_focus_clears_the_pane_that_regained_it() {
    // The actual reported defect: a background terminal tab, not just an
    // active-pane change inside one server. The nested sandwich is what makes
    // "the terminal loses focus" real rather than simulated.
    let server = Server::start();
    let inner = server.first_pane();
    server.source_snippet();
    // The client the sandwich attaches lands on `inner`, which fires its own
    // `pane-focus-in` and spawns a hook run in the background; wait for that
    // one to land before arranging the state under test, or it could race the
    // assertions below.
    server.put_status(&inner, "waiting");
    let (host, _outer) = server.nested_client();
    wait_for(|| server.pane_status(&inner), |status| status.is_empty());

    // The host's window moves off the client attached to `server`: that
    // client's terminal loses focus, and `pane-focus-out` reaches `inner`.
    // Nothing acknowledges a pane on focus-out, so a state reported while the
    // terminal is elsewhere is left for the user to find.
    host.tmux(&["select-window", "-t", "t:1"]);
    assert_ok(&server.agent_status(&inner, &["set", "done"]));
    assert_eq!(server.pane_status(&inner), "done");

    // The host's window returns: the client's terminal is focused again,
    // `pane-focus-in` reaches `inner`, and only now is it acknowledged.
    host.tmux(&["select-window", "-t", "t:0"]);
    wait_for(|| server.pane_status(&inner), |status| status.is_empty());
    wait_for(|| server.window_status(&inner), |status| status.is_empty());
}

#[test]
fn regaining_terminal_focus_clears_only_the_pane_the_client_landed_on() {
    // The pane that regains focus and its on-screen sibling, together: only
    // the one the terminal focus actually returns to is acknowledged, and the
    // window glyph is left to recompute from what the sibling still holds.
    let server = Server::start();
    let active = server.first_pane();
    let sibling = server.split(&active);
    server.source_snippet();
    // As above: the sandwich's own attach lands on `active` and fires a
    // background hook run first.
    server.put_status(&active, "waiting");
    let (host, _outer) = server.nested_client();
    wait_for(|| server.pane_status(&active), |status| status.is_empty());

    // `active` outranks `sibling` here, so the window glyph reflects `active`
    // until it clears - the assertions below can only tell the recompute
    // happened if the glyph actually changes when that clear lands.
    assert_ok(&server.agent_status(&active, &["set", "waiting"]));
    assert_ok(&server.agent_status(&sibling, &["set", "done"]));
    assert_eq!(server.window_status(&active), "\u{1f4ac}");

    host.tmux(&["select-window", "-t", "t:1"]);
    host.tmux(&["select-window", "-t", "t:0"]);

    wait_for(|| server.pane_status(&active), |status| status.is_empty());
    // The sibling was never focused and keeps its state; the window glyph
    // recomputes from it rather than vanishing with the active pane's.
    assert_eq!(server.pane_status(&sibling), "done");
    assert_eq!(server.window_status(&active), "\u{2705}");
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
        ["clear-pane"].as_slice(),
        ["clear-pane", "%0"].as_slice(),
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
    // The pane precedence: within a pane `error` > `done` > `waiting` >
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
    // The race the pane precedence exists for: an agent runs the hooks of one
    // turn concurrently, so the `working` of a tool that finished arrives while
    // the prompt of the tool that is blocked is still open.
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
    // The rollup rank is not the pane precedence:
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
    // `show-options`, and a window with no agent must carry none.
    let server = Server::start();
    let pane = server.first_pane();
    let bare = server.new_window("no-agent-here");
    assert_ok(&server.agent_status(&pane, &["set", "error"]));

    assert_ok(&server.agent_status(&pane, &["clear-pane"]));

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
    assert_ok(&server.agent_status(&bare, &["clear-pane"]));
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
