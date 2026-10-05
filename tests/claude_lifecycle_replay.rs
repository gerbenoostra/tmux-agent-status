//! Replay the Claude Code lifecycle fixtures through the shipped drop-in.
//!
//! `tests/fixtures/claude-code/lifecycle/<scenario>/` holds one probe record
//! per hook event Claude fired during a real run, reduced to adapter-readable
//! fields. Each scenario is played back through
//! `share/agents/claude-code/hooks.json` by `support::replay` - the event's
//! own matchers decide which command runs - and its rows are held in the
//! scenario's `expected.tsv`.

use std::fs;
use std::path::Path;

mod support;

use support::lifecycle;
use support::replay;

const DROP_IN: &str = "share/agents/claude-code/hooks.json";

/// What a hook entry's `matcher` is tested against for one event, per
/// Claude Code's hooks reference (https://code.claude.com/docs/en/hooks).
enum MatchSubject {
    /// The event has no matcher support: Claude ignores the field and always
    /// runs the entry.
    Ignored,
    /// The payload field whose value the matcher tests.
    Field(&'static str),
}

/// Events the reference lists with a matcher subject this replay does not
/// model fail the test rather than being guessed at.
fn match_subject(event: &str) -> MatchSubject {
    match event {
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure" | "PermissionRequest"
        | "PermissionDenied" => MatchSubject::Field("tool_name"),
        "SessionStart" | "ConfigChange" => MatchSubject::Field("source"),
        "SessionEnd" => MatchSubject::Field("reason"),
        "Notification" => MatchSubject::Field("notification_type"),
        "SubagentStart" | "SubagentStop" => MatchSubject::Field("agent_type"),
        "PreCompact" | "PostCompact" => MatchSubject::Field("trigger"),
        // The reference calls the value `error_type`; the captured payloads
        // carry it as `error`.
        "StopFailure" => MatchSubject::Field("error"),
        "UserPromptSubmit" | "PostToolBatch" | "Stop" | "TeammateIdle" | "TaskCreated"
        | "TaskCompleted" | "WorktreeCreate" | "WorktreeRemove" | "MessageDisplay"
        | "CwdChanged" => MatchSubject::Ignored,
        other => panic!("replay does not know how Claude matches {other}"),
    }
}

/// Claude's documented matcher rule (https://code.claude.com/docs/en/hooks):
/// `*` or empty matches everything; a matcher of only letters, digits, `_`,
/// `-`, spaces, `,` and `|` is a list of exact names split on `|` or `,`;
/// anything else is an unanchored regex. `FileChanged` and `StopFailure` are
/// narrower: only letters, digits, `_` and `|` are exact, and only `|`
/// separates names. The shipped matchers are exact lists, and this replay has
/// no regex engine, so a regex matcher fails the test instead of being guessed
/// at.
fn matcher_applies(event: &str, matcher: &str, subject: &str) -> bool {
    if matcher.is_empty() || matcher == "*" {
        return true;
    }
    let (exact_extra, separators): (&str, &[char]) = match event {
        "FileChanged" | "StopFailure" => ("_|", &['|']),
        _ => ("_- ,|", &['|', ',']),
    };
    let exact = matcher
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || exact_extra.contains(c));
    assert!(
        exact,
        "replay cannot evaluate regex matcher `{matcher}` on {event}"
    );
    matcher
        .split(separators)
        .map(str::trim)
        .any(|name| !name.is_empty() && name == subject)
}

#[test]
fn matchers_are_exact_names_not_substrings() {
    assert!(matcher_applies(
        "PreToolUse",
        "AskUserQuestion|ExitPlanMode",
        "ExitPlanMode"
    ));
    assert!(matcher_applies("PreToolUse", "Edit, Write", "Write"));
    assert!(matcher_applies("PreToolUse", "*", "anything"));
    assert!(!matcher_applies("PreToolUse", "Task", "TaskStop"));
    assert!(!matcher_applies(
        "SessionStart",
        "startup|resume|clear|fork",
        "compact"
    ));
    assert!(matcher_applies(
        "StopFailure",
        "server_error|rate_limit",
        "rate_limit"
    ));
}

#[test]
#[should_panic(expected = "regex matcher")]
fn stop_failure_comma_list_is_a_regex() {
    matcher_applies("StopFailure", "server_error, rate_limit", "rate_limit");
}

fn drop_in_with(event: &str, matcher: &str) -> serde_json::Value {
    serde_json::json!({ "hooks": { event: [{
        "matcher": matcher,
        "hooks": [{ "type": "command", "command": "tmux-agent-status set done" }]
    }] } })
}

fn record(event: &str, payload: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "event": event, "payload": payload })
}

#[test]
fn a_matcher_on_an_event_without_matcher_support_is_ignored() {
    let hooks = drop_in_with("Stop", "anything");
    assert_eq!(
        commands_for(&record("Stop", serde_json::json!({})), &hooks).len(),
        1
    );
}

#[test]
fn stop_failure_matches_on_the_error_code() {
    let hooks = drop_in_with("StopFailure", "rate_limit");
    let failure = |error| record("StopFailure", serde_json::json!({ "error": error }));
    assert_eq!(commands_for(&failure("rate_limit"), &hooks).len(), 1);
    assert!(commands_for(&failure("server_error"), &hooks).is_empty());
}

#[test]
#[should_panic(expected = "StopFailure payload has no string `error`")]
fn a_payload_missing_its_matcher_subject_fails_the_replay() {
    let hooks = drop_in_with("StopFailure", "rate_limit");
    commands_for(&record("StopFailure", serde_json::json!({})), &hooks);
}

#[test]
#[should_panic(expected = "does not know how Claude matches")]
fn a_matcher_on_an_unmodelled_event_fails_the_replay() {
    let hooks = drop_in_with("FileChanged", ".envrc");
    commands_for(&record("FileChanged", serde_json::json!({})), &hooks);
}

/// The commands the shipped drop-in would run for one fixture record.
fn commands_for(record: &serde_json::Value, hooks: &serde_json::Value) -> Vec<String> {
    let event = record["event"].as_str().unwrap();
    let payload = &record["payload"];
    replay::commands(&hooks["hooks"][event], |matcher| {
        match match_subject(event) {
            MatchSubject::Ignored => true,
            // A fixture missing the field would replay as a silent
            // non-match; it means the sanitizer dropped it.
            MatchSubject::Field(field) => {
                let subject = payload
                    .get(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        panic!("{event} payload has no string `{field}`: {payload}")
                    });
                matcher_applies(event, matcher, subject)
            }
        }
    })
}

#[test]
fn claude_lifecycle_fixtures_replay_through_the_shipped_drop_in() {
    if !support::tmux_or_skip() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hooks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(DROP_IN)).unwrap()).unwrap();
    // Each scenario has its own server, so they replay concurrently; a
    // failing scenario's panic names its thread.
    let scenarios = lifecycle::scenarios();
    let replays: Vec<Vec<String>> = std::thread::scope(|scope| {
        let replays: Vec<_> = scenarios
            .iter()
            .map(|scenario| {
                std::thread::Builder::new()
                    .name(scenario.name())
                    .spawn_scoped(scope, || {
                        replay::replay(scenario, |record| commands_for(record, &hooks))
                    })
                    .expect("a replay thread starts")
            })
            .collect();
        replays
            .into_iter()
            .map(|replay| replay.join().expect("the scenario replays"))
            .collect()
    });
    for (scenario, rows) in scenarios.iter().zip(replays) {
        replay::check_or_write(scenario, &rows);
    }
}
