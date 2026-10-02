//! The aggregate pane lifecycle against a real disposable tmux server.
//!
//! The lifecycle actions (`ResetSession`, `WorkStarted`, `WorkStopped`,
//! `EndSession`) have no CLI spelling - an adapter maps payloads to them inside
//! the binary - so these tests call `command::apply` directly, with `$TMUX`
//! aimed at the test's own server through `Server::in_process`. Every
//! `command::apply` spawns its own tmux client, the same way concurrent hook
//! processes do.
//!
//! Bell assertions need a real pane tty, so the `set` that rings is driven
//! inside the pane's shell; `window_bell_flag` sees what that tty received.

use std::time::Duration;

mod support;

use support::tmux::{Server, wait_for};
use tmux_agent_status::command;
use tmux_agent_status::notify::{HostSession, NotifyAction, WorkKey};
use tmux_agent_status::state::State;

/// What an idle pane runs. The tool resolves panes from `$TMUX_PANE` and never
/// inspects processes, so a pane does not have to look like an agent.
const IDLE: &str = "sleep 300";

/// The pane's shell for the bell tests; see `claude_lifecycle_replay.rs` for
/// why the startup files are skipped.
const SHELL: &str = "env HISTFILE=/dev/null bash --noprofile --norc";

/// A host session the server accepts.
fn session(id: &str) -> HostSession {
    HostSession::new(id).expect("a valid session id")
}

fn key(session: &str, work: &str) -> WorkKey {
    WorkKey::new(session, work).expect("a valid work key")
}

impl Server {
    fn start() -> Server {
        Self::start_running(IDLE)
    }

    /// Apply an action to `pane` in-process, the way `notify`'s dispatch does.
    fn apply(&self, pane: &str, action: &NotifyAction) {
        self.in_process(|| command::apply(action, Some(pane)).expect("apply succeeds"));
    }

    /// `command::start`/`finish`/`clear_pane` are not `NotifyAction`s; run them
    /// the same way.
    fn run_command(&self, pane: &str, f: fn(Option<&str>) -> std::io::Result<()>) {
        self.in_process(|| f(Some(pane)).expect("command succeeds"));
    }

    /// Run `command` inside the pane's shell, the way an agent hook does:
    /// `$TMUX`/`$TMUX_PANE` come from tmux and a bell lands on the pane's tty.
    fn run_in_pane(&self, pane: &str, command: &str, seq: usize) {
        let marker = format!("__tas_done_{seq}__");
        self.tmux(&[
            "send-keys",
            "-t",
            pane,
            "-l",
            &format!("{command}; echo {marker}"),
        ]);
        self.tmux(&["send-keys", "-t", pane, "Enter"]);
        wait_for(
            || self.tmux(&["capture-pane", "-t", pane, "-p"]),
            |screen| screen.lines().any(|line| line.trim() == marker),
        );
    }

    /// The pane's shell is answering; keys sent before it is ready can drop.
    fn handshake(&self, pane: &str) {
        wait_for(
            || {
                self.tmux(&["send-keys", "-t", pane, "-l", "echo __tas_ready__"]);
                self.tmux(&["send-keys", "-t", pane, "Enter"]);
                std::thread::sleep(Duration::from_millis(50));
                self.tmux(&["capture-pane", "-t", pane, "-p"])
            },
            |screen| screen.lines().any(|line| line.trim() == "__tas_ready__"),
        );
    }

    /// `@agent_pane_status` for every pane of `target`'s window.
    fn pane_statuses(&self, target: &str) -> Vec<String> {
        self.tmux(&["list-panes", "-t", target, "-F", "#{@agent_pane_status}"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// One internal layer option of the pane, empty when unset. Assertions
    /// only; nothing the tool writes reads these back.
    fn layer(&self, pane: &str, option: &str) -> String {
        self.tmux(&[
            "display-message",
            "-p",
            "-t",
            pane,
            &format!("#{{{option}}}"),
        ])
        .trim_end()
        .to_owned()
    }

    /// `1` while a bell is pending on the pane's window.
    fn bell_flag(&self, pane: &str) -> String {
        self.tmux(&["display-message", "-p", "-t", pane, "#{window_bell_flag}"])
            .trim_end()
            .to_owned()
    }

    /// Selecting a window acknowledges its bell; away and back clears the flag
    /// so the next assertion sees only bells that came after.
    fn clear_bell(&self, pane: &str) {
        self.tmux(&["select-window", "-t", "t:dummy"]);
        self.tmux(&[
            "select-window",
            "-t",
            &format!("t:{}", self.window_id(pane)),
        ]);
    }

    fn window_id(&self, pane: &str) -> String {
        self.tmux(&["display-message", "-p", "-t", pane, "#{window_id}"])
            .trim_end()
            .to_owned()
    }

    /// A second idle window used to clear the bell flag between assertions.
    fn add_dummy_window(&self) {
        self.tmux(&[
            "new-window",
            "-d",
            "-a",
            "-t",
            "t:{end}",
            "-n",
            "dummy",
            IDLE,
        ]);
    }

    fn first_pane(&self) -> String {
        self.tmux(&["list-panes", "-t", "t", "-F", "#{pane_id}"])
            .lines()
            .next()
            .expect("the session has a pane")
            .to_owned()
    }

    /// A new pane in the same window.
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

    /// A legacy scalar status, the way an older version left it: only
    /// `@agent_pane_status` set, no layer options, no migration marker.
    fn legacy(&self, pane: &str, value: &str) {
        self.tmux(&["set-option", "-p", "-t", pane, "@agent_pane_status", value]);
    }
}

/// A clean `done` report through the pane lifecycle entry point.
fn done() -> NotifyAction {
    NotifyAction::Report(State::Done)
}

fn reset_session(id: &str) -> NotifyAction {
    NotifyAction::ResetSession {
        session: session(id),
    }
}

fn work_started(session: &str, work: &str) -> NotifyAction {
    NotifyAction::WorkStarted {
        key: key(session, work),
    }
}

fn work_stopped(session: &str, work: &str) -> NotifyAction {
    NotifyAction::WorkStopped {
        key: key(session, work),
    }
}

fn end_session(id: &str) -> NotifyAction {
    NotifyAction::EndSession {
        session: session(id),
    }
}

#[test]
fn a_clean_stop_waits_for_tracked_work_and_rings_at_the_final_root_stop() {
    // The canonical timeline: work outlives the parent turn's stop, the last
    // stop settles the pane, and the following clean root stop is the one that
    // shows done.
    let server = Server::start();
    let pane = server.first_pane();

    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &NotifyAction::Report(State::Working));
    server.apply(&pane, &work_started("s1", "a"));

    server.apply(&pane, &done());
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(
        server.layer(&pane, "@agent_pane_work"),
        format!(",{},", key("s1", "a").encoded())
    );
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "stopped");
    assert_eq!(server.layer(&pane, "@agent_pane_completion"), "pending");

    // The child stopping settles the root; the pane still shows activity.
    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "settling");
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
    assert_eq!(server.pane_status(&pane), "working");

    server.apply(&pane, &done());
    assert_eq!(server.pane_status(&pane), "done");
    assert_eq!(server.window_status(&pane), "✅");
}

#[test]
fn work_events_without_an_accepted_session_are_ignored() {
    // A pane no session was accepted on - scalar or un-migrated - takes no
    // work events at all.
    let server = Server::start();
    let pane = server.first_pane();

    server.apply(&pane, &work_started("s1", "a"));
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
    server.apply(&pane, &work_stopped("s1", "a"));
    server.apply(&pane, &end_session("s1"));
    assert_eq!(server.pane_status(&pane), "");
    // The migration still ran: the marker is the only trace.
    assert_eq!(server.layer(&pane, "@agent_pane_model"), "1");
}

#[test]
fn two_work_items_complete_in_either_order() {
    for (first, second) in [("a", "b"), ("b", "a")] {
        let server = Server::start();
        let pane = server.first_pane();
        server.apply(&pane, &reset_session("s1"));
        server.apply(&pane, &work_started("s1", "a"));
        server.apply(&pane, &work_started("s1", "b"));
        server.apply(&pane, &done());
        assert_eq!(server.pane_status(&pane), "working", "{first} first");

        server.apply(&pane, &work_stopped("s1", first));
        assert_eq!(server.pane_status(&pane), "working", "{first} first");
        assert_eq!(
            server.layer(&pane, "@agent_pane_work"),
            format!(",{},", key("s1", second).encoded()),
            "{first} first"
        );

        server.apply(&pane, &work_stopped("s1", second));
        assert_eq!(server.layer(&pane, "@agent_pane_work"), "", "{first} first");
        assert_eq!(server.layer(&pane, "@agent_pane_root"), "settling");
    }
}

#[test]
fn duplicate_and_unmatched_work_events_are_no_ops() {
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));

    server.apply(&pane, &work_started("s1", "a"));
    server.apply(&pane, &work_started("s1", "a"));
    assert_eq!(
        server.layer(&pane, "@agent_pane_work"),
        format!(",{},", key("s1", "a").encoded())
    );

    // A stop for a token that was never added cannot eat another item.
    server.apply(&pane, &work_stopped("s1", "b"));
    assert_eq!(
        server.layer(&pane, "@agent_pane_work"),
        format!(",{},", key("s1", "a").encoded())
    );
    assert_eq!(server.pane_status(&pane), "working");

    // A stopped root does not settle on an unmatched stop.
    server.apply(&pane, &done());
    server.apply(&pane, &work_stopped("s1", "b"));
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "stopped");

    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "settling");
}

#[test]
fn attention_shows_over_work_and_acknowledgement_reveals_it() {
    // 💬 and ❗ are immediately visible over tracked work; focusing the pane
    // acknowledges the attention and shows the work again.
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));

    server.apply(&pane, &NotifyAction::Report(State::Waiting));
    assert_eq!(server.pane_status(&pane), "waiting");

    server.run_command(&pane, command::clear_pane);
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "stopped");

    // The last item stopping still settles, rather than jumping to done.
    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.pane_status(&pane), "working");
    server.apply(&pane, &done());
    assert_eq!(server.pane_status(&pane), "done");
}

#[test]
fn an_error_survives_a_clean_stop_until_acknowledged() {
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));
    server.apply(&pane, &NotifyAction::Report(State::Error));
    assert_eq!(server.pane_status(&pane), "error");

    // ❗ survives the clean stop, exactly as today.
    server.apply(&pane, &done());
    assert_eq!(server.pane_status(&pane), "error");

    // Focus acknowledges the error; the work is still tracked.
    server.run_command(&pane, command::clear_pane);
    assert_eq!(server.pane_status(&pane), "working");

    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.pane_status(&pane), "working");
    server.apply(&pane, &done());
    assert_eq!(server.pane_status(&pane), "done");
}

#[test]
fn start_preserves_work_and_clears_what_the_last_turn_left() {
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));
    server.apply(&pane, &done());
    assert_eq!(server.pane_status(&pane), "working");

    server.run_command(&pane, command::start);
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_completion"), "");
    assert_ne!(server.layer(&pane, "@agent_pane_work"), "");

    // The work finishing under a running root does not settle; the turn's own
    // stop is what resolves it.
    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "working");
    server.apply(&pane, &done());
    assert_eq!(server.pane_status(&pane), "done");
}

#[test]
fn reset_session_ignores_delayed_old_session_events() {
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));
    assert_eq!(
        server.layer(&pane, "@agent_pane_host_session"),
        session("s1").encoded()
    );

    // The next session's SessionStart clears the aggregate and accepts s2.
    server.apply(&pane, &reset_session("s2"));
    assert_eq!(server.pane_status(&pane), "");
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
    assert_eq!(
        server.layer(&pane, "@agent_pane_host_session"),
        session("s2").encoded()
    );

    // Everything the dead session can still emit is a no-op.
    server.apply(&pane, &work_started("s1", "b"));
    server.apply(&pane, &work_stopped("s1", "a"));
    server.apply(&pane, &end_session("s1"));
    assert_eq!(server.pane_status(&pane), "");
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
    assert_eq!(
        server.layer(&pane, "@agent_pane_host_session"),
        session("s2").encoded()
    );
}

#[test]
fn end_session_of_the_accepted_session_finishes_and_clears_the_ledger() {
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));

    server.apply(&pane, &end_session("s1"));

    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
    assert_eq!(server.pane_status(&pane), "done");

    // An old session's end cannot resolve a pane it does not own.
    server.apply(&pane, &reset_session("s2"));
    server.apply(&pane, &work_started("s2", "a"));
    server.apply(&pane, &end_session("s1"));
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(
        server.layer(&pane, "@agent_pane_work"),
        format!(",{},", key("s2", "a").encoded())
    );
}

#[test]
fn a_generic_reset_drops_the_aggregate_and_the_accepted_session() {
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));

    server.run_command(&pane, command::reset);

    assert_eq!(server.pane_status(&pane), "");
    for option in [
        "@agent_pane_root",
        "@agent_pane_attention",
        "@agent_pane_completion",
        "@agent_pane_work",
        "@agent_pane_host_session",
    ] {
        assert_eq!(server.layer(&pane, option), "", "{option}");
    }
    // A late event of the cleared session is refused.
    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
}

#[test]
fn finish_records_a_pending_outcome_and_waits_for_work() {
    // The payload-less session end cannot know whether work ended, so it
    // behaves like a silent clean stop.
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));

    server.run_command(&pane, command::finish);
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_completion"), "pending");

    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "settling");
    server.run_command(&pane, command::finish);
    assert_eq!(server.pane_status(&pane), "done");
}

#[test]
fn finish_with_no_work_is_done() {
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &NotifyAction::Report(State::Working));
    server.run_command(&pane, command::finish);
    assert_eq!(server.pane_status(&pane), "done");
}

#[test]
fn concurrent_starts_and_stops_keep_the_ledger_exact() {
    // Every thread spawns its own tmux client, the way hook processes do; the
    // server serializes their queues. Half the starts are duplicates on
    // purpose, and half the stops are unmatched.
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));

    server.in_process(|| {
        std::thread::scope(|scope| {
            for i in 0..20 {
                let pane = pane.clone();
                scope.spawn(move || {
                    let action = work_started("s1", &format!("w{}", i % 10));
                    command::apply(&action, Some(&pane)).expect("apply succeeds");
                });
            }
        });
    });
    let mut ledger = server.layer(&pane, "@agent_pane_work");
    for i in 0..10 {
        assert!(
            ledger.contains(&format!(",{},", key("s1", &format!("w{i}")).encoded())),
            "w{i} missing from {ledger}"
        );
    }

    server.in_process(|| {
        std::thread::scope(|scope| {
            for i in 0..20 {
                let pane = pane.clone();
                scope.spawn(move || {
                    // Half unmatched, half the real tokens, all racing.
                    let work = if i % 2 == 0 {
                        format!("w{}", i / 2)
                    } else {
                        format!("unknown-{i}")
                    };
                    let action = work_stopped("s1", &work);
                    command::apply(&action, Some(&pane)).expect("apply succeeds");
                });
            }
        });
    });
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");

    // The normalized empty form is genuinely unset, not a remnant.
    ledger = server.tmux(&["show-options", "-p", "-t", &pane]);
    assert!(!ledger.contains("@agent_pane_work"), "{ledger}");
}

#[test]
fn concurrent_stops_and_a_done_leave_a_consistent_end_state() {
    // The pair the serialize-everything rule exists for: a clean stop racing
    // the final work stops. The server picks an order; either way the ledger
    // is exact and the projection matches the layers it lands on. `done`
    // serialized last ends the pane; anything else leaves a settling root
    // waiting on the host's automatic turn.
    let server = Server::start();
    let pane = server.first_pane();
    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));
    server.apply(&pane, &work_started("s1", "b"));

    server.in_process(|| {
        std::thread::scope(|scope| {
            for action in [done(), work_stopped("s1", "a"), work_stopped("s1", "b")] {
                let pane = pane.clone();
                scope.spawn(move || {
                    command::apply(&action, Some(&pane)).expect("apply succeeds");
                });
            }
        });
    });
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
    let status = server.pane_status(&pane);
    let root = server.layer(&pane, "@agent_pane_root");
    assert!(
        (status == "done" && root == "stopped") || (status == "working" && root == "settling"),
        "inconsistent end state: status {status} root {root}"
    );
}

#[test]
fn concurrent_starts_and_stops_across_windows_roll_up_per_pane() {
    // Two panes of one window aggregate independently; the window glyph is the
    // highest rank of their projections.
    let server = Server::start();
    let first = server.first_pane();
    let second = server.split(&first);
    server.apply(&first, &reset_session("s1"));
    server.apply(&first, &work_started("s1", "a"));
    server.apply(&first, &done());
    assert_eq!(server.window_status(&first), "🤖");

    server.apply(&second, &NotifyAction::Report(State::Waiting));
    assert_eq!(server.pane_statuses(&first), ["working", "waiting"]);
    assert_eq!(server.window_status(&first), "💬");

    server.run_command(&second, command::clear_pane);
    assert_eq!(server.window_status(&first), "🤖");

    server.apply(&first, &work_stopped("s1", "a"));
    server.apply(&first, &done());
    assert_eq!(server.pane_statuses(&first), ["done", ""]);
    assert_eq!(server.window_status(&first), "✅");
}

#[test]
fn every_legacy_scalar_imports_before_the_transition_that_arrives() {
    // Cases written as an old version left them: only the scalar
    // `@agent_pane_status` set, no marker. The first transition imports it
    // inside the same queue.
    let cases: &[(&str, &[NotifyAction], &str)] = &[
        (
            "working",
            &[NotifyAction::Report(State::Working)],
            "working",
        ),
        ("done", &[NotifyAction::Report(State::Working)], "working"),
        (
            "waiting",
            &[NotifyAction::Report(State::Working)],
            "waiting",
        ),
        ("error", &[NotifyAction::Report(State::Working)], "error"),
        ("waiting", &[done()], "done"),
        ("error", &[done()], "error"),
        ("done", &[done()], "done"),
        ("busy", &[done()], "done"),
    ];
    for (i, (held, actions, expected)) in cases.iter().enumerate() {
        let server = Server::start();
        let pane = server.first_pane();
        server.legacy(&pane, held);
        for action in *actions {
            server.apply(&pane, action);
        }
        assert_eq!(
            server.pane_status(&pane),
            *expected,
            "case {i}: held {held}"
        );
        assert_eq!(server.layer(&pane, "@agent_pane_model"), "1");
    }
}

#[test]
fn a_migrated_working_survives_clear_pane() {
    // The sticky-state case the migration exists for: legacy `working`
    // followed by focus acknowledgement must stay working.
    let server = Server::start();
    let pane = server.first_pane();
    server.legacy(&pane, "working");

    server.run_command(&pane, command::clear_pane);

    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "working");
    assert_eq!(server.layer(&pane, "@agent_pane_model"), "1");
}

#[test]
fn clear_pane_after_migration_acknowledges_done_waiting_and_error() {
    for held in ["done", "waiting", "error"] {
        let server = Server::start();
        let pane = server.first_pane();
        server.legacy(&pane, held);

        server.run_command(&pane, command::clear_pane);

        assert_eq!(server.pane_status(&pane), "", "held {held}");
    }
}

#[test]
fn an_untouched_pane_migrates_clean() {
    let server = Server::start();
    let pane = server.first_pane();

    server.run_command(&pane, command::clear_pane);

    assert_eq!(server.pane_status(&pane), "");
    assert_eq!(server.layer(&pane, "@agent_pane_model"), "1");
    assert_eq!(server.layer(&pane, "@agent_pane_root"), "");
}

#[test]
fn a_turn_without_tracked_work_behaves_as_today() {
    // The scalar contract end to end: waiting rings, start acknowledges, the
    // clean stop lands ✅ and rings once.
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    server.add_dummy_window();
    let pane = server.first_pane();
    server.handshake(&pane);

    server.run_in_pane(&pane, "tmux-agent-status set waiting", 0);
    assert_eq!(server.pane_status(&pane), "waiting");
    assert_eq!(server.bell_flag(&pane), "1");

    server.clear_bell(&pane);
    server.run_in_pane(&pane, "tmux-agent-status start", 1);
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(server.bell_flag(&pane), "0", "start must not ring");

    server.run_in_pane(&pane, "tmux-agent-status set done", 2);
    assert_eq!(server.pane_status(&pane), "done");
    assert_eq!(server.bell_flag(&pane), "1");

    // A repeated done rings again, exactly as today.
    server.clear_bell(&pane);
    server.run_in_pane(&pane, "tmux-agent-status set done", 3);
    assert_eq!(server.bell_flag(&pane), "1", "a repeated done rings");
}

#[test]
fn a_clean_stop_is_silent_while_work_remains_and_rings_at_the_end() {
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    server.add_dummy_window();
    let pane = server.first_pane();
    server.handshake(&pane);

    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));

    // The premature clean stop is silent: nothing finished.
    server.run_in_pane(&pane, "tmux-agent-status set done", 0);
    assert_eq!(server.pane_status(&pane), "working");
    assert_eq!(
        server.bell_flag(&pane),
        "0",
        "a stop under work must not ring"
    );

    server.apply(&pane, &work_stopped("s1", "a"));
    assert_eq!(server.pane_status(&pane), "working");

    // The root stop after the last item is the one that rings.
    server.run_in_pane(&pane, "tmux-agent-status set done", 1);
    assert_eq!(server.pane_status(&pane), "done");
    assert_eq!(server.bell_flag(&pane), "1");
}

#[test]
fn a_failed_or_missing_pane_still_rings_done() {
    // The verdict cannot answer, so the bell falls back to ringing: a silently
    // dropped completion is worse than a bell.
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    server.add_dummy_window();
    let pane = server.first_pane();
    server.handshake(&pane);

    server.run_in_pane(&pane, "tmux-agent-status set done --pane %9999", 0);
    assert_eq!(server.bell_flag(&pane), "1", "a missing pane still rings");

    server.clear_bell(&pane);
    server.run_in_pane(&pane, "TMUX_PANE= tmux-agent-status set done", 1);
    assert_eq!(server.bell_flag(&pane), "1", "no resolved pane still rings");
}

#[test]
fn lifecycle_actions_never_ring() {
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    let pane = server.first_pane();
    server.handshake(&pane);

    server.apply(&pane, &reset_session("s1"));
    server.apply(&pane, &work_started("s1", "a"));
    server.apply(&pane, &work_stopped("s1", "a"));
    server.apply(&pane, &end_session("s1"));

    assert_eq!(server.bell_flag(&pane), "0");
    assert_eq!(server.pane_status(&pane), "done");
}

#[test]
fn apply_without_a_pane_is_a_no_op() {
    let server = Server::start();
    let pane = server.first_pane();
    server.in_process(|| {
        // No pane anywhere: not an argument, not the environment.
        command::apply(&reset_session("s1"), None).expect("apply succeeds");
        command::apply(&work_started("s1", "a"), None).expect("apply succeeds");
    });
    assert_eq!(server.pane_status(&pane), "");
    assert_eq!(server.layer(&pane, "@agent_pane_work"), "");
}
