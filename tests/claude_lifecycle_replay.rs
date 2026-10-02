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
//! the scenario's `expected.tsv`. With `TAS_REPLAY_WRITE=1` the rows are
//! written instead of asserted. The rows document what the *shipped* mapping
//! does - including the defect where a parent `Stop` shows ✅ and rings while
//! a tracked child is still running.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod support;

use support::tmux::{Server, wait_for};

const FIXTURES: &str = "tests/fixtures/claude-code/lifecycle";
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

    fn bell_flag(&self, target: &str) -> String {
        self.tmux(&["display-message", "-p", "-t", target, "#{window_bell_flag}"])
            .trim_end()
            .to_owned()
    }
}

/// The value a hook entry's `matcher` tests, per event.
///
/// Claude Code matches tool events on `tool_name`, `SessionStart` on `source`
/// and `Notification` on `notification_type`; events without a listed subject
/// here take no matcher in the shipped file, so None only matters if one is
/// added later and the fixture replay should flag it rather than guess.
fn match_subject<'a>(event: &str, payload: &'a serde_json::Value) -> Option<&'a str> {
    let field = match event {
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure" | "PermissionRequest" => "tool_name",
        "SessionStart" => "source",
        "Notification" => "notification_type",
        "SubagentStart" | "SubagentStop" => "agent_type",
        _ => return None,
    };
    payload.get(field)?.as_str()
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
    matcher.split(separators).any(|name| name.trim() == subject)
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

/// The commands the shipped drop-in would run for one fixture record.
fn commands_for(record: &serde_json::Value, hooks: &serde_json::Value) -> Vec<String> {
    let event = record["event"].as_str().unwrap();
    let payload = &record["payload"];
    let mut found = Vec::new();
    for entry in hooks["hooks"][event].as_array().into_iter().flatten() {
        let applies = match entry.get("matcher").and_then(|m| m.as_str()) {
            None => true,
            Some(matcher) => match_subject(event, payload)
                .map(|s| matcher_applies(event, matcher, s))
                .unwrap_or(false),
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

fn fixture_records(dir: &Path) -> Vec<(String, serde_json::Value)> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|f| f.unwrap().path())
        .filter(|f| f.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|f| {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            let record = serde_json::from_str(&fs::read_to_string(f).unwrap())
                .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", f.display()));
            (name, record)
        })
        .collect()
}

/// Replay one scenario directory and return its `NNN\tevent\tcommand\tpane\twindow\tbell` rows.
fn replay(dir: &Path, hooks: &serde_json::Value) -> Vec<String> {
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    // A second window so selecting away and back clears the bell flag between
    // events without an attached client.
    server.tmux(&["new-window", "-d", "-n", "dummy", "sleep 300"]);
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
    for (seq, (name, record)) in fixture_records(dir).iter().enumerate() {
        let event = record["event"].as_str().unwrap();
        let commands = commands_for(record, hooks);
        let command = if commands.is_empty() {
            "-".to_owned()
        } else {
            for (i, command) in commands.iter().enumerate() {
                server.run_hook(&pane, command, seq * 100 + i);
            }
            commands.join(" && ")
        };
        let bell = server.bell_flag(&window);
        // Selecting the window marks it viewed and clears the flag.
        server.tmux(&["select-window", "-t", "t:dummy"]);
        server.tmux(&["select-window", "-t", &format!("t:{window}")]);
        rows.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            name.trim_end_matches(".json"),
            event,
            command,
            server.pane_status(&pane),
            server.window_status(&window),
            bell
        ));
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
    let write = std::env::var_os("TAS_REPLAY_WRITE").is_some();

    let mut scenarios = 0;
    for entry in fs::read_dir(root.join(FIXTURES)).unwrap() {
        let dir = entry.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        let rows = replay(&dir, &hooks);
        scenarios += 1;
        let expected_path = dir.join("expected.tsv");
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
            dir.display()
        );
        for (actual, wanted) in rows.iter().zip(&expected_rows) {
            assert_eq!(actual, wanted, "{}: replay drifted", dir.display());
        }
    }
    assert!(scenarios > 0, "no lifecycle scenarios under {FIXTURES}");
}
