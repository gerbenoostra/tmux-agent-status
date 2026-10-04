//! The `claude-code` adapter, driven by the captured lifecycle fixtures.
//!
//! Every payload is the `payload` field of a record under
//! `tests/fixtures/claude-code/lifecycle/`, captured from real Claude Code runs.
//! Malformed, mismatched and duplicate inputs are labeled mutations of those
//! captures, never hand-written Claude payloads.
//!
//! The CLI half of this file runs the real binary against a disposable tmux
//! server: `reset --agent claude-code --stdin` and `finish --agent
//! claude-code --stdin` must map a readable session payload to `ResetSession` /
//! `EndSession`, and anything else to the generic command.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use tmux_agent_status::notify::{HostSession, NotifyAction, WorkKey, dispatch};

mod support;

use support::lifecycle::read_record;
use support::tmux::{Server, TMUX_TIMEOUT, output_within, output_within_feeding};

const AGENT: &str = "claude-code";

/// The `payload` of one fixture record, as the hook would receive it on stdin.
fn payload(scenario: &str, stem: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-code/lifecycle")
        .join(scenario)
        .join(format!("{stem}.json"));
    read_record(&path)["payload"].clone()
}

fn dispatch_payload(payload: &serde_json::Value) -> Option<NotifyAction> {
    dispatch(AGENT, &serde_json::to_string(payload).unwrap())
}

fn field<'a>(payload: &'a serde_json::Value, name: &str) -> &'a str {
    payload[name]
        .as_str()
        .unwrap_or_else(|| panic!("{name}: {payload}"))
}

#[test]
fn session_start_is_a_reset_session() {
    let start = payload("s1-permission-clean-finish", "001-sessionstart");
    assert_eq!(
        dispatch_payload(&start),
        Some(NotifyAction::ResetSession {
            session: HostSession::new(field(&start, "session_id")).unwrap()
        })
    );
}

#[test]
fn session_end_is_an_end_session() {
    let end = payload("s8-host-exit", "019-sessionend");
    assert_eq!(
        dispatch_payload(&end),
        Some(NotifyAction::EndSession {
            session: HostSession::new(field(&end, "session_id")).unwrap()
        })
    );
}

#[test]
fn subagent_start_and_stop_are_work_lifecycle() {
    let start = payload("s7-child-tool-events", "004-subagentstart");
    let key = WorkKey::new(field(&start, "session_id"), field(&start, "agent_id")).unwrap();
    assert_eq!(
        dispatch_payload(&start),
        Some(NotifyAction::WorkStarted { key: key.clone() })
    );

    let stop = payload("s7-child-tool-events", "012-subagentstop");
    assert_eq!(
        dispatch_payload(&stop),
        Some(NotifyAction::WorkStopped { key })
    );
}

#[test]
fn a_successful_task_stop_stops_the_started_work() {
    // S5's identity proof: the TaskStop task_id is the cancelled agent's
    // SubagentStart agent_id, so the cancellation removes the item the start
    // added rather than an anonymous decrement.
    let start = payload("s5-model-cancellation", "009-subagentstart");
    let stop = payload("s5-model-cancellation", "014-posttooluse");
    assert_eq!(field(&stop, "tool_name"), "TaskStop");

    let started = match dispatch_payload(&start) {
        Some(NotifyAction::WorkStarted { key }) => key,
        other => panic!("SubagentStart did not start work: {other:?}"),
    };
    assert_eq!(
        dispatch_payload(&stop),
        Some(NotifyAction::WorkStopped { key: started })
    );
}

#[test]
fn unmatched_helper_stops_still_map() {
    // `SubagentStop` also fires for internal helpers that never emitted a
    // start. The adapter maps it like any other stop; the ledger's
    // exact-removal makes it a no-op.
    let stop = payload("s5-model-cancellation", "019-subagentstop");
    assert_eq!(field(&stop, "agent_type"), "");
    assert!(matches!(
        dispatch_payload(&stop),
        Some(NotifyAction::WorkStopped { .. })
    ));
}

#[test]
fn a_duplicate_delivery_maps_to_the_same_action() {
    // Idempotency lives in the ledger; the adapter's part is returning the
    // identical key for the identical event.
    let start = payload("s7-child-tool-events", "004-subagentstart");
    assert_eq!(dispatch_payload(&start), dispatch_payload(&start));
}

/// Captured events that carry no lifecycle signal drop to `None`.
#[test]
fn events_without_a_lifecycle_mapping_are_dropped() {
    for (scenario, stem) in [
        ("s1-permission-clean-finish", "002-userpromptsubmit"),
        ("s1-permission-clean-finish", "008-stop"),
        ("s1-permission-clean-finish", "005-notification"),
        ("s1-permission-clean-finish", "003-pretooluse"),
        ("s1-permission-clean-finish", "006-posttooluse"),
        ("s2-background-outlives-turn", "010-messagedisplay"),
        ("s10-aborted-turn", "043-stopfailure"),
        ("s5-model-cancellation", "012-pretooluse"), // TaskStop's PreToolUse
    ] {
        assert_eq!(
            dispatch_payload(&payload(scenario, stem)),
            None,
            "{scenario}/{stem}"
        );
    }
}

/// Mutations of the captured `PostToolUse(TaskStop)` payload: only the equal,
/// non-empty `task_id` pair may stop work.
#[test]
fn task_stop_mutations_are_dropped() {
    let stop = payload("s5-model-cancellation", "014-posttooluse");
    let cases: Vec<(&str, serde_json::Value)> = vec![
        {
            // Failed/partial stop: the tool ran but reported another task.
            let mut p = stop.clone();
            p["tool_response"]["task_id"] = serde_json::json!("some-other-task");
            ("response names another task", p)
        },
        {
            let mut p = stop.clone();
            p["tool_input"]["task_id"] = serde_json::json!("");
            ("empty input task_id", p)
        },
        {
            let mut p = stop.clone();
            p["tool_response"]["task_id"] = serde_json::json!("");
            ("empty response task_id", p)
        },
        {
            let mut p = stop.clone();
            p["tool_response"]
                .as_object_mut()
                .unwrap()
                .remove("task_id");
            ("response task_id missing", p)
        },
        {
            let mut p = stop.clone();
            p["tool_response"] = serde_json::json!("Task stopped");
            ("response is a string", p)
        },
        {
            // A failed dispatch arrives as PostToolUseFailure, not PostToolUse.
            let mut p = stop.clone();
            p["hook_event_name"] = serde_json::json!("PostToolUseFailure");
            ("the failure event", p)
        },
        {
            let mut p = stop.clone();
            p.as_object_mut().unwrap().remove("tool_name");
            ("tool_name missing", p)
        },
        {
            let mut p = stop.clone();
            p["tool_name"] = serde_json::json!(42);
            ("tool_name not a string", p)
        },
        {
            let mut p = stop.clone();
            p["tool_name"] = serde_json::json!("Bash");
            ("another tool", p)
        },
        {
            let mut p = stop.clone();
            p.as_object_mut().unwrap().remove("tool_input");
            ("tool_input missing", p)
        },
        {
            let mut p = stop.clone();
            p["tool_input"].as_object_mut().unwrap().remove("task_id");
            ("input task_id missing", p)
        },
        {
            let mut p = stop.clone();
            p["tool_input"]["task_id"] = serde_json::json!(42);
            ("input task_id not a string", p)
        },
        {
            let mut p = stop.clone();
            p.as_object_mut().unwrap().remove("tool_response");
            ("tool_response missing", p)
        },
        {
            let mut p = stop.clone();
            p["tool_response"]["task_id"] = serde_json::json!(42);
            ("response task_id not a string", p)
        },
        {
            // Both IDs agree, but an empty ID is not a usable work key.
            let mut p = stop.clone();
            p["tool_input"]["task_id"] = serde_json::json!("");
            p["tool_response"]["task_id"] = serde_json::json!("");
            ("equal but empty task_ids", p)
        },
        {
            let mut p = stop.clone();
            p.as_object_mut().unwrap().remove("session_id");
            ("session_id missing", p)
        },
    ];
    for (label, mutated) in cases {
        assert_eq!(dispatch_payload(&mutated), None, "{label}");
    }
}

/// Mutations of the captured `SubagentStart`/`SubagentStop` payloads.
#[test]
fn work_event_mutations_are_dropped() {
    let start = payload("s7-child-tool-events", "004-subagentstart");
    let stop = payload("s7-child-tool-events", "012-subagentstop");
    let cases: Vec<(&str, serde_json::Value)> = vec![
        {
            let mut p = stop.clone();
            p.as_object_mut().unwrap().remove("agent_id");
            ("stop: agent_id missing", p)
        },
        {
            let mut p = stop.clone();
            p["agent_id"] = serde_json::json!(42);
            ("stop: agent_id not a string", p)
        },
        {
            let mut p = stop.clone();
            p["agent_id"] = serde_json::json!("");
            ("stop: agent_id empty", p)
        },
        {
            let mut p = start.clone();
            p.as_object_mut().unwrap().remove("agent_id");
            ("agent_id missing", p)
        },
        {
            let mut p = start.clone();
            p["agent_id"] = serde_json::json!("");
            ("agent_id empty", p)
        },
        {
            let mut p = start.clone();
            p["agent_id"] = serde_json::json!(42);
            ("agent_id not a string", p)
        },
        {
            let mut p = start.clone();
            p["session_id"] = serde_json::json!("");
            ("session_id empty", p)
        },
        {
            let mut p = start.clone();
            p["session_id"] = serde_json::json!("s".repeat(257));
            ("session_id over the byte cap", p)
        },
        {
            let mut p = start.clone();
            p["hook_event_name"] = serde_json::json!("MadeUpEvent");
            ("unknown event", p)
        },
        {
            let mut p = start.clone();
            p["hook_event_name"] = serde_json::json!(7);
            ("non-string event", p)
        },
        {
            let mut p = start.clone();
            p.as_object_mut().unwrap().remove("hook_event_name");
            ("event missing", p)
        },
    ];
    for (label, mutated) in cases {
        assert_eq!(dispatch_payload(&mutated), None, "{label}");
    }
}

/// Mutations of the captured `SessionStart`/`SessionEnd` payloads.
#[test]
fn session_event_mutations_are_dropped() {
    let start = payload("s1-permission-clean-finish", "001-sessionstart");
    let end = payload("s8-host-exit", "019-sessionend");
    let cases: Vec<(&str, serde_json::Value)> = vec![
        {
            let mut p = start.clone();
            p.as_object_mut().unwrap().remove("session_id");
            ("session_id missing", p)
        },
        {
            let mut p = start.clone();
            p["session_id"] = serde_json::json!("");
            ("session_id empty", p)
        },
        {
            let mut p = start.clone();
            p["session_id"] = serde_json::json!(42);
            ("session_id not a string", p)
        },
        {
            let mut p = end.clone();
            p["session_id"] = serde_json::json!("");
            ("end: session_id empty", p)
        },
    ];
    for (label, mutated) in cases {
        assert_eq!(dispatch_payload(&mutated), None, "{label}");
    }
}

#[test]
fn an_unreadable_payload_is_dropped() {
    assert_eq!(dispatch(AGENT, "not-json"), None);
    assert_eq!(dispatch(AGENT, "[1,2,3]"), None);
    // `SubagentStop` under a foreign agent name is still nothing: dispatch
    // namespaces payloads by the agent they arrived for.
    let stop = payload("s7-child-tool-events", "012-subagentstop");
    assert_eq!(
        dispatch("mistral-vibe", &serde_json::to_string(&stop).unwrap()),
        None
    );
}

/// The binary, as the hook would invoke it: inside the pane's tmux
/// environment, with the payload piped to stdin.
fn binary(
    server: &Server,
    pane: &str,
    args: &[&str],
    payload: Option<&serde_json::Value>,
) -> Output {
    let mut command = Command::new(support::BIN);
    command
        .args(args)
        .env("TMUX", format!("{},0,0", server.socket_path()))
        .env("TMUX_PANE", pane)
        .env_remove("TMUX_AGENT_STATUS_PANE")
        .env_remove("TMUX_AGENT_STATUS_DISABLED")
        .env_remove("TMUX_AGENT_STATUS_DEBUG");
    match payload {
        Some(payload) => output_within_feeding(
            command,
            serde_json::to_string(payload).unwrap().as_bytes(),
            TMUX_TIMEOUT,
        ),
        None => {
            command.stdin(Stdio::null());
            output_within(command, TMUX_TIMEOUT)
        }
    }
}

/// One layer option of the pane, empty when unset.
fn layer(server: &Server, pane: &str, option: &str) -> String {
    server
        .tmux(&[
            "display-message",
            "-p",
            "-t",
            pane,
            &format!("#{{{option}}}"),
        ])
        .trim_end()
        .to_owned()
}

#[test]
fn session_commands_dispatch_the_payload_or_run_generic() {
    if !support::tmux_or_skip() {
        return;
    }
    let server = Server::start_running(support::tmux::IDLE);
    let pane = server
        .tmux(&["list-panes", "-t", "t", "-F", "#{pane_id}"])
        .lines()
        .next()
        .expect("the session has a pane")
        .to_owned();

    let start = payload("s1-permission-clean-finish", "001-sessionstart");
    let session = HostSession::new(field(&start, "session_id")).unwrap();
    let work = payload("s7-child-tool-events", "004-subagentstart");
    // The work fixture is from another capture: give it this session's id so
    // the ledger accepts it.
    let mut work = work;
    work["session_id"] = start["session_id"].clone();
    let end = payload("s8-host-exit", "019-sessionend");
    let mut end_here = end.clone();
    end_here["session_id"] = start["session_id"].clone();

    let run = |args: &[&str], payload: Option<&serde_json::Value>| {
        let out = binary(&server, &pane, args, payload);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            support::stderr_of(&out)
        );
    };

    // A readable SessionStart accepts the session.
    run(&["reset", "--agent", AGENT, "--stdin"], Some(&start));
    assert_eq!(
        layer(&server, &pane, "@agent_pane_host_session"),
        session.encoded(),
    );

    // A SubagentStart opens the ledger; the pane shows tracked work.
    run(&["notify", "--agent", AGENT, "--stdin"], Some(&work));
    assert!(!layer(&server, &pane, "@agent_pane_work").is_empty());
    assert_eq!(server.pane_status(&pane), "working");

    // A SessionEnd for another session is a no-op, ledger included.
    run(&["finish", "--agent", AGENT, "--stdin"], Some(&end));
    assert!(!layer(&server, &pane, "@agent_pane_work").is_empty());
    assert_eq!(server.pane_status(&pane), "working");

    // A wrong-event payload on finish runs the generic command: it resolves
    // the root but cannot touch the ledger, so work keeps the pane working.
    run(&["finish", "--agent", AGENT, "--stdin"], Some(&start));
    assert!(!layer(&server, &pane, "@agent_pane_work").is_empty());
    assert_eq!(server.pane_status(&pane), "working");

    // The matching SessionEnd ends the session and its tracked work.
    run(&["finish", "--agent", AGENT, "--stdin"], Some(&end_here));
    assert!(layer(&server, &pane, "@agent_pane_work").is_empty());
    assert_eq!(server.pane_status(&pane), "done");

    // An unreadable payload on reset runs the generic command: the pane and
    // the accepted session are gone.
    run(&["reset", "--agent", AGENT, "--stdin"], None);
    assert_eq!(server.pane_status(&pane), "");
    assert!(layer(&server, &pane, "@agent_pane_host_session").is_empty());
}
