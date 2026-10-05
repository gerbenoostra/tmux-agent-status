//! Replay observed scalar-host lifecycle fixtures through their shipped drop-ins.

use std::fs;
use std::path::Path;

mod support;

use support::lifecycle;
use support::replay::{self, check_or_write};

/// How a host applies a hook entry's `matcher` to one record: `(event,
/// matcher, payload)`. A host rule panics on a matcher it cannot evaluate
/// rather than guessing, so a drop-in change cannot silently replay wrong.
type Matches = fn(&str, &str, &serde_json::Value) -> bool;

/// The commands `hooks` runs for one record. `wrapped` drop-ins nest their
/// events under a top-level `hooks` key.
fn commands_for(
    record: &serde_json::Value,
    hooks: &serde_json::Value,
    wrapped: bool,
    matches: Matches,
) -> Vec<String> {
    let event = record["event"].as_str().unwrap();
    let events = if wrapped { &hooks["hooks"] } else { hooks };
    replay::commands(&events[event], |matcher| {
        matches(event, matcher, &record["payload"])
    })
}

fn run(
    scenario: &lifecycle::Scenario,
    hooks: &serde_json::Value,
    wrapped: bool,
    matches: Matches,
) -> Vec<String> {
    replay::replay(scenario, |record| {
        commands_for(record, hooks, wrapped, matches)
    })
}

/// For a drop-in that ships no matchers.
fn no_matchers(event: &str, matcher: &str, _: &serde_json::Value) -> bool {
    panic!("replay has no matcher rule for `{matcher}` on {event}")
}

/// Vibe's `match` field: the shipped drop-in only uses `*`.
fn vibe_matches(event: &str, matcher: &str, payload: &serde_json::Value) -> bool {
    matcher == "*" || no_matchers(event, matcher, payload)
}

/// Devin's matcher is a regex over the tool event's `tool_name`
/// (docs/agents/devin.md). This replay has no regex engine: it evaluates an
/// anchored list of literal names, `^name$` or `^(a|b)$`, which is the only
/// form the drop-in ships, and fails on anything else.
fn devin_matches(event: &str, matcher: &str, payload: &serde_json::Value) -> bool {
    assert!(
        matches!(event, "PreToolUse" | "PostToolUse" | "PermissionRequest"),
        "replay does not know how Devin matches {event}"
    );
    let subject = payload["tool_name"]
        .as_str()
        .unwrap_or_else(|| panic!("{event} payload has no string `tool_name`: {payload}"));
    let inner = matcher
        .strip_prefix('^')
        .and_then(|m| m.strip_suffix('$'))
        // Only a group makes `|` a list of names: ungrouped, `^a|b$` is
        // `(^a)|(b$)`.
        .map(
            |m| match m.strip_prefix('(').and_then(|m| m.strip_suffix(')')) {
                Some(names) => (names, true),
                None => (m, false),
            },
        )
        .filter(|(names, grouped)| {
            names
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || (*grouped && c == '|'))
        })
        .map(|(names, _)| names)
        .unwrap_or_else(|| panic!("replay cannot evaluate Devin matcher `{matcher}`"));
    inner.split('|').any(|name| name == subject)
}

#[test]
fn devin_matchers_are_anchored_name_lists() {
    let tool = |name| serde_json::json!({ "tool_name": name });
    let shipped = "^(ask_user_question|exit_plan_mode)$";
    assert!(devin_matches(
        "PreToolUse",
        shipped,
        &tool("exit_plan_mode")
    ));
    assert!(!devin_matches("PreToolUse", shipped, &tool("run_subagent")));
    assert!(!devin_matches("PreToolUse", shipped, &tool("exec")));
    assert!(devin_matches("PreToolUse", "^exec$", &tool("exec")));
    assert!(!devin_matches("PreToolUse", "^exec$", &tool("exec_bg")));
}

#[test]
#[should_panic(expected = "cannot evaluate Devin matcher")]
fn an_unanchored_devin_matcher_fails_the_replay() {
    devin_matches(
        "PreToolUse",
        "exec",
        &serde_json::json!({ "tool_name": "exec" }),
    );
}

/// `^a|b$` is the regex `(^a)|(b$)`, a prefix-or-suffix test, not a list.
#[test]
#[should_panic(expected = "cannot evaluate Devin matcher")]
fn an_ungrouped_devin_alternation_fails_the_replay() {
    devin_matches(
        "PreToolUse",
        "^exec|read$",
        &serde_json::json!({ "tool_name": "exec" }),
    );
}

/// The shipped TOML as `{hooks: {type: [{matcher?, hooks: [{command}]}]}}`.
/// Each `[[hooks]]` block holds only flat `key = "string"` lines.
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
        let mut entry = serde_json::json!({
            "hooks": [{"command": value("command").expect("a Vibe hook command")}]
        });
        if let Some(matcher) = value("match") {
            entry["matcher"] = matcher.into();
        }
        push(&mut events, event, entry);
    }
    serde_json::json!({"hooks": events})
}

/// The shipped standalone file as `{hooks: {trigger: [{matcher?, hooks:
/// [{command}]}]}}`.
fn kiro_hooks(root: &Path) -> serde_json::Value {
    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("share/agents/kiro/tmux-agent-status.json")).unwrap(),
    )
    .unwrap();
    let mut events = serde_json::Map::new();
    for hook in config["hooks"].as_array().unwrap() {
        let mut entry = serde_json::json!({"hooks": [{"command": hook["action"]["command"]}]});
        if let Some(matcher) = hook.get("matcher") {
            entry["matcher"] = matcher.clone();
        }
        push(
            &mut events,
            hook["trigger"].as_str().unwrap().to_owned(),
            entry,
        );
    }
    serde_json::json!({"hooks": events})
}

/// Append `entry` to `event`'s entries: several hooks may share an event.
fn push(
    events: &mut serde_json::Map<String, serde_json::Value>,
    event: String,
    entry: serde_json::Value,
) {
    events
        .entry(event)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .unwrap()
        .push(entry);
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
    let rows = run(scenario, &hooks, true, no_matchers);
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
    let rows = run(scenario, &hooks, false, devin_matches);
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
    let rows = run(scenario, &hooks, true, vibe_matches);
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
    let rows = run(scenario, &hooks, true, no_matchers);
    check_or_write(scenario, &rows);

    let final_row = rows.last().expect("stop is captured");
    assert!(final_row.contains("\tstop\t"));
    let fields: Vec<&str> = final_row.split('\t').collect();
    assert_eq!(&fields[3..], ["done", "✅", "1"]);
}
