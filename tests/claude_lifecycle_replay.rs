//! Replay the Claude Code lifecycle fixtures through the shipped drop-in.
//!
//! `tests/fixtures/claude-code/lifecycle/<scenario>/` holds one probe record
//! per hook event Claude fired during a real run, reduced to adapter-readable
//! fields. This test plays each record's event back through
//! `share/agents/claude-code/hooks.json` - the event's own matchers decide
//! which command runs - executed inside a pane of a disposable tmux server,
//! so the bell reaches a real tty and `window_bell_flag` can see it.
//!
//! Every record yields one row: `NNN event command pane window bell`, held in
//! the scenario's `expected.tsv`. With `TAS_REPLAY_WRITE=1` (only `1`) the rows are
//! written instead of asserted. A command taking `--stdin` is fed the record's
//! `payload` from a file, recorded in the command column as `< NNN-event`.

use std::fs;
use std::path::Path;
use std::time::Duration;

mod support;

use support::lifecycle::{self, Scenario, read_record};
use support::tempdir::TempDir;
use support::tmux::{Server, wait_for};

const DROP_IN: &str = "share/agents/claude-code/hooks.json";

/// The pane's shell. Without `--noprofile --norc` it would source the
/// developer's startup files, which can put an installed `tmux-agent-status`
/// ahead of the build under test on `PATH`; without `HISTFILE=/dev/null` the
/// SIGHUP from `kill-server` makes it save every replayed command into the
/// developer's `~/.bash_history`. `PATH` itself comes from the server, which
/// `support::tmux` starts with the binary under test in front.
const SHELL: &str = "env HISTFILE=/dev/null bash --noprofile --norc";

impl Server {
    /// Run a hook command the way the host would: inside the pane, so $TMUX
    /// and $TMUX_PANE are set and the bell lands on the pane's tty.
    fn run_hook(&self, pane: &str, command: &str, seq: usize) {
        let marker = format!("__tas_done_{seq}__");
        self.tmux(&[
            "send-keys",
            "-t",
            pane,
            "-l",
            &format!("{command}; echo {marker}"),
        ]);
        self.tmux(&["send-keys", "-t", pane, "Enter"]);
        // The marker shows up in the *input* line as soon as it is typed, so a
        // plain contains() can return before the command has run. Only a line
        // that is exactly the marker proves the echo executed.
        wait_for(
            || self.tmux(&["capture-pane", "-t", pane, "-p"]),
            |screen| screen.lines().any(|line| line.trim() == marker),
        );
    }

    /// The pane's state, its window's rollup and the window's bell flag, as
    /// `pane\twindow\tbell`, read in one round trip.
    fn observed(&self, pane: &str) -> String {
        self.tmux(&[
            "display-message",
            "-p",
            "-t",
            pane,
            "#{@agent_pane_status}\t#{@agent_status}\t#{window_bell_flag}",
        ])
        .trim_end()
        .to_owned()
    }
}

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
    let mut found = Vec::new();
    for entry in hooks["hooks"][event].as_array().into_iter().flatten() {
        let applies = match entry.get("matcher").and_then(|m| m.as_str()) {
            None => true,
            Some(matcher) => match match_subject(event) {
                MatchSubject::Ignored => true,
                // A fixture missing the field would replay as a silent
                // non-match; it means the sanitizer dropped it.
                MatchSubject::Field(field) => {
                    let subject =
                        payload
                            .get(field)
                            .and_then(|v| v.as_str())
                            .unwrap_or_else(|| {
                                panic!("{event} payload has no string `{field}`: {payload}")
                            });
                    matcher_applies(event, matcher, subject)
                }
            },
        };
        if !applies {
            continue;
        }
        for hook in entry["hooks"].as_array().into_iter().flatten() {
            if let Some(command) = hook.get("command").and_then(|c| c.as_str()) {
                found.push(command.to_owned());
            }
        }
    }
    found
}

/// Replay one scenario directory and return its `NNN\tevent\tcommand\tpane\twindow\tbell` rows.
fn replay(scenario: &Scenario, hooks: &serde_json::Value) -> Vec<String> {
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    // A second window so selecting away and back clears the bell flag between
    // events without an attached client.
    server.tmux(&["new-window", "-d", "-n", "dummy", support::tmux::IDLE]);
    let pane = server
        .tmux(&["list-panes", "-t", "t:0", "-F", "#{pane_id}"])
        .trim_end()
        .to_owned();
    let window = server
        .tmux(&["display-message", "-p", "-t", &pane, "#{window_id}"])
        .trim_end()
        .to_owned();

    // Keys sent before the pane's shell is ready can be dropped, so handshake
    // first: resend until the pane echoes back.
    wait_for(
        || {
            server.tmux(&["send-keys", "-t", &pane, "-l", "echo __tas_ready__"]);
            server.tmux(&["send-keys", "-t", &pane, "Enter"]);
            std::thread::sleep(Duration::from_millis(50));
            server.tmux(&["capture-pane", "-t", &pane, "-p"])
        },
        |screen| screen.lines().any(|line| line.trim() == "__tas_ready__"),
    );

    let mut rows = Vec::new();
    let payloads = TempDir::new(&format!("replay-{}", scenario.name()));
    for (seq, path) in scenario.records.iter().enumerate() {
        let record = read_record(path);
        let event = record["event"].as_str().unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let commands = commands_for(&record, hooks);
        let command = if commands.is_empty() {
            "-".to_owned()
        } else {
            // Hook commands that take `--stdin` get the record's payload piped
            // in, the way the host delivers it. Written to a file rather than
            // inlined into a shell string, so payload quoting cannot decide
            // what the command does.
            let mut stdin_file = None;
            if commands.iter().any(|command| command.contains("--stdin")) {
                let file = payloads.write(
                    &format!("{stem}.payload.json"),
                    &serde_json::to_string(&record["payload"]).unwrap(),
                );
                stdin_file = Some(format!("'{}'", file.display()));
            }
            for (i, command) in commands.iter().enumerate() {
                let run = match &stdin_file {
                    Some(file) if command.contains("--stdin") => {
                        format!("{command} < {file}")
                    }
                    _ => command.clone(),
                };
                server.run_hook(&pane, &run, seq * 100 + i);
            }
            commands
                .iter()
                .map(|command| match &stdin_file {
                    Some(_) if command.contains("--stdin") => format!("{command} < {stem}"),
                    _ => command.clone(),
                })
                .collect::<Vec<_>>()
                .join(" && ")
        };
        let observed = server.observed(&pane);
        // Selecting the window marks it viewed and clears the bell flag.
        server.tmux(&["select-window", "-t", "t:dummy"]);
        server.tmux(&["select-window", "-t", &format!("t:{window}")]);
        rows.push(format!("{stem}\t{event}\t{command}\t{observed}"));
    }
    rows
}

const HEADER: &str = "# file\tevent\tcommand\tpane\twindow\tbell";

#[test]
fn claude_lifecycle_fixtures_replay_through_the_shipped_drop_in() {
    if !support::tmux_or_skip() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hooks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(DROP_IN)).unwrap()).unwrap();
    let write = std::env::var_os("TAS_REPLAY_WRITE").is_some_and(|v| v == "1");

    // Each scenario has its own server, so they replay concurrently; a
    // failing scenario's panic names its thread.
    let scenarios = lifecycle::scenarios();
    let replays: Vec<Vec<String>> = std::thread::scope(|scope| {
        let replays: Vec<_> = scenarios
            .iter()
            .map(|scenario| {
                std::thread::Builder::new()
                    .name(scenario.name())
                    .spawn_scoped(scope, || replay(scenario, &hooks))
                    .expect("a replay thread starts")
            })
            .collect();
        replays
            .into_iter()
            .map(|replay| replay.join().expect("the scenario replays"))
            .collect()
    });
    for (scenario, rows) in scenarios.iter().zip(replays) {
        let expected_path = scenario.expected_path();
        if write {
            let mut text = String::from(HEADER);
            text.push('\n');
            for row in &rows {
                text.push_str(row);
                text.push('\n');
            }
            fs::write(&expected_path, text).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap_or_else(|e| {
            panic!(
                "{} missing (run with TAS_REPLAY_WRITE=1 to record): {e}",
                expected_path.display()
            )
        });
        let expected_rows: Vec<&str> = expected.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(
            rows.len(),
            expected_rows.len(),
            "{}: fixture count changed - re-record with TAS_REPLAY_WRITE=1",
            scenario.name()
        );
        for (actual, wanted) in rows.iter().zip(&expected_rows) {
            assert_eq!(actual, wanted, "{}: replay drifted", scenario.name());
        }
    }
}
