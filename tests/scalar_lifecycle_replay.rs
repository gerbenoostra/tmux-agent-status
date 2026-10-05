//! Replay observed scalar-host lifecycle fixtures through their shipped drop-ins.

use std::fs;
use std::path::Path;

mod support;

use support::lifecycle;
use support::replay::{self, check_or_write};

/// The commands `hooks` runs for one record. `wrapped` drop-ins nest their
/// events under a top-level `hooks` key.
fn commands_for(
    record: &serde_json::Value,
    hooks: &serde_json::Value,
    wrapped: bool,
) -> Vec<String> {
    let events = if wrapped { &hooks["hooks"] } else { hooks };
    replay::commands(&events[record["event"].as_str().unwrap()], |_| true)
}

fn run(scenario: &lifecycle::Scenario, hooks: &serde_json::Value, wrapped: bool) -> Vec<String> {
    replay::replay(scenario, |record| commands_for(record, hooks, wrapped))
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

fn kiro_hooks(root: &Path) -> serde_json::Value {
    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("share/agents/kiro/tmux-agent-status.json")).unwrap(),
    )
    .unwrap();
    let mut events = serde_json::Map::new();
    for hook in config["hooks"].as_array().unwrap() {
        let event = hook["trigger"].as_str().unwrap();
        let command = hook["action"]["command"].as_str().unwrap();
        events.insert(
            event.to_owned(),
            serde_json::json!([{"hooks": [{"command": command}]}]),
        );
    }
    serde_json::json!({"hooks": events})
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
    let rows = run(scenario, &hooks, true);
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
    let rows = run(scenario, &hooks, false);
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
    let rows = run(scenario, &hooks, true);
    check_or_write(scenario, &rows);

    let final_row = rows.last().expect("post_agent is captured");
    assert!(final_row.contains("\tpost_agent\t"));
    let fields: Vec<&str> = final_row.split('\t').collect();
    assert_eq!(&fields[3..], ["done", "✅", "1"]);
}

#[test]
fn kiro_scalar_replay_finishes_a_normal_turn() {
    if !support::tmux_or_skip() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hooks = kiro_hooks(root);
    let scenarios = lifecycle::scenarios_for("kiro");
    assert_eq!(scenarios.len(), 1);
    let scenario = &scenarios[0];
    let rows = run(scenario, &hooks, true);
    check_or_write(scenario, &rows);

    let final_row = rows.last().expect("stop is captured");
    assert!(final_row.contains("\tstop\t"));
    let fields: Vec<&str> = final_row.split('\t').collect();
    assert_eq!(&fields[3..], ["done", "✅", "1"]);
}
