//! Replay observed scalar-host lifecycle fixtures through their shipped drop-ins.

use std::fs;
use std::path::Path;
use std::time::Duration;

mod support;

use support::lifecycle::{self, Scenario, read_record};
use support::tempdir::TempDir;
use support::tmux::{Server, wait_for};

const SHELL: &str = "env HISTFILE=/dev/null bash --noprofile --norc";
const HEADER: &str = "# file\tevent\tcommand\tpane\twindow\tbell";

impl Server {
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
        wait_for(
            || self.tmux(&["capture-pane", "-t", pane, "-p"]),
            |screen| screen.lines().any(|line| line.trim() == marker),
        );
    }

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

fn commands_for(event: &str, hooks: &serde_json::Value, wrapped: bool) -> Vec<String> {
    let events = if wrapped { &hooks["hooks"] } else { hooks };
    events[event]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
        .filter_map(|hook| hook["command"].as_str().map(str::to_owned))
        .collect()
}

fn replay(scenario: &Scenario, hooks: &serde_json::Value, wrapped: bool) -> Vec<String> {
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    server.tmux(&["new-window", "-d", "-n", "dummy", support::tmux::IDLE]);
    let pane = server
        .tmux(&["list-panes", "-t", "t:0", "-F", "#{pane_id}"])
        .trim_end()
        .to_owned();
    let window = server
        .tmux(&["display-message", "-p", "-t", &pane, "#{window_id}"])
        .trim_end()
        .to_owned();
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
    let payloads = TempDir::new(&format!("scalar-replay-{}", scenario.name()));
    for (seq, path) in scenario.records.iter().enumerate() {
        let record = read_record(path);
        let event = record["event"].as_str().unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy();
        let commands = commands_for(event, hooks, wrapped);
        let stdin_file = commands
            .iter()
            .any(|command| command.contains("--stdin"))
            .then(|| {
                payloads.write(
                    &format!("{stem}.payload.json"),
                    &serde_json::to_string(&record["payload"]).unwrap(),
                )
            });
        for (i, command) in commands.iter().enumerate() {
            let run = match &stdin_file {
                Some(file) if command.contains("--stdin") => {
                    format!("{command} < '{}'", file.display())
                }
                _ => command.clone(),
            };
            server.run_hook(&pane, &run, seq * 100 + i);
        }
        let command = if commands.is_empty() {
            "-".to_owned()
        } else {
            commands
                .iter()
                .map(|command| match stdin_file {
                    Some(_) if command.contains("--stdin") => format!("{command} < {stem}"),
                    _ => command.clone(),
                })
                .collect::<Vec<_>>()
                .join(" && ")
        };
        rows.push(format!(
            "{stem}\t{event}\t{command}\t{}",
            server.observed(&pane)
        ));
        server.tmux(&["select-window", "-t", "t:dummy"]);
        server.tmux(&["select-window", "-t", &format!("t:{window}")]);
    }
    rows
}

fn vibe_hooks(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(root.join("share/agents/mistral-vibe/hooks.toml")).unwrap();
    let mut events = serde_json::Map::new();
    for block in text.split("[[hooks]]").skip(1) {
        let value = |key: &str| {
            block.lines().find_map(|line| {
                line.trim()
                    .strip_prefix(&format!("{key} = \""))
                    .and_then(|value| value.strip_suffix('"'))
                    .map(str::to_owned)
            })
        };
        let event = value("type").expect("a Vibe hook type");
        let command = value("command").expect("a Vibe hook command");
        events.insert(
            event,
            serde_json::json!([{"hooks": [{"command": command}]}]),
        );
    }
    serde_json::json!({"hooks": events})
}

fn check_or_write(scenario: &Scenario, rows: &[String]) {
    let expected_path = scenario.expected_path();
    if std::env::var_os("TAS_REPLAY_WRITE").is_some_and(|v| v == "1") {
        let text = format!("{HEADER}\n{}\n", rows.join("\n"));
        fs::write(expected_path, text).unwrap();
        return;
    }
    let expected = fs::read_to_string(&expected_path).unwrap_or_else(|e| {
        panic!(
            "{} missing (run with TAS_REPLAY_WRITE=1 to record): {e}",
            expected_path.display()
        )
    });
    let expected_rows: Vec<&str> = expected.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(rows, expected_rows, "{}: replay drifted", scenario.name());
}

#[test]
fn grok_scalar_replay_records_the_early_false_completion() {
    if !support::tmux_or_skip() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hooks: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("share/agents/grok/tmux-agent-status.json")).unwrap(),
    )
    .unwrap();
    let scenarios = lifecycle::scenarios_for("grok");
    assert_eq!(scenarios.len(), 1);
    let scenario = &scenarios[0];
    let rows = replay(scenario, &hooks, true);
    check_or_write(scenario, &rows);

    let early_stop = rows
        .iter()
        .position(|row| row.starts_with("007-stop\tStop\t"))
        .expect("the parent stop is captured");
    let child_stop = rows
        .iter()
        .position(|row| row.contains("\tSubagentStop\t"))
        .expect("the child stop is captured");
    assert!(early_stop < child_stop);
    let early_fields: Vec<&str> = rows[early_stop].split('\t').collect();
    assert_eq!(&early_fields[3..], ["done", "✅", "1"]);
    for row in &rows {
        if row.contains("\tStop\t") || row.contains("\tSessionEnd\t") {
            let fields: Vec<&str> = row.split('\t').collect();
            assert!(
                fields[3] != "working" && fields[4] != "🤖",
                "scalar completion left working visible: {row}"
            );
        }
    }
}

#[test]
fn devin_scalar_replay_records_unattributed_worker_completion() {
    if !support::tmux_or_skip() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hooks: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("share/agents/devin/hooks.v1.json")).unwrap(),
    )
    .unwrap();
    let scenarios = lifecycle::scenarios_for("devin");
    assert_eq!(scenarios.len(), 1);
    let scenario = &scenarios[0];
    let rows = replay(scenario, &hooks, false);
    check_or_write(scenario, &rows);

    let early_stop = rows
        .iter()
        .position(|row| row.starts_with("006-stop\tStop\t"))
        .expect("the parent stop is captured");
    let worker_stop = rows
        .iter()
        .position(|row| row.starts_with("008-stop\tStop\t"))
        .expect("the worker stop is captured");
    assert!(early_stop < worker_stop);
    let early_fields: Vec<&str> = rows[early_stop].split('\t').collect();
    assert_eq!(&early_fields[3..], ["done", "✅", "1"]);
    for row in &rows[early_stop..] {
        let fields: Vec<&str> = row.split('\t').collect();
        assert!(
            fields[3] != "working" && fields[4] != "🤖",
            "scalar completion exposed working again: {row}"
        );
    }
}

#[test]
fn vibe_scalar_replay_finishes_while_the_child_is_running() {
    if !support::tmux_or_skip() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hooks = vibe_hooks(root);
    let scenarios = lifecycle::scenarios_for("mistral-vibe");
    assert_eq!(scenarios.len(), 1);
    let scenario = &scenarios[0];
    let rows = replay(scenario, &hooks, true);
    check_or_write(scenario, &rows);

    let final_row = rows.last().expect("post_agent is captured");
    assert!(final_row.contains("\tpost_agent\t"));
    let fields: Vec<&str> = final_row.split('\t').collect();
    assert_eq!(&fields[3..], ["done", "✅", "1"]);
}
