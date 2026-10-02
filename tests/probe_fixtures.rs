//! Schema and hygiene rules for the probe lifecycle fixtures.
//!
//! `tests/fixtures/claude-code/lifecycle/<scenario>/NNN-<event>.json` files
//! are real captured hook payloads reduced by `probe/sanitize.jq` to the
//! fields an adapter may read. These tests pin that contract: the envelope
//! shape, the key allowlist, and no absolute paths or content-bearing fields
//! anywhere in the tree.

use std::fs;
use std::path::{Path, PathBuf};

const FIXTURES: &str = "tests/fixtures/claude-code/lifecycle";

/// Top-level payload keys a fixture may carry. Anything outside this list is
/// either content (prompt, message, tool bodies) or a local path - both are
/// stripped by the sanitizer and must stay out of committed fixtures.
const PAYLOAD_KEYS: &[&str] = &[
    "hook_event_name",
    "session_id",
    "prompt_id",
    "permission_mode",
    "source",
    "reason",
    "stop_hook_active",
    "notification_type",
    "agent_id",
    "agent_type",
    "tool_name",
    "tool_use_id",
    "trigger",
    "name",
    "error",
    "tool_input",
    "tool_response",
    "background_tasks",
];

const TOOL_INPUT_KEYS: &[&str] = &["task_id", "run_in_background", "subagent_type", "isolation"];
const TOOL_RESPONSE_KEYS: &[&str] = &[
    "task_id",
    "task_type",
    "agentId",
    "status",
    "isAsync",
    "success",
];
const BACKGROUND_TASK_KEYS: &[&str] = &["id", "type", "status", "agent_type"];

fn fixture_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES);
    let mut files = Vec::new();
    for entry in fs::read_dir(&root).unwrap_or_else(|e| panic!("{FIXTURES}: {e}")) {
        let dir = entry.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        for file in fs::read_dir(&dir).unwrap() {
            let path = file.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Every string leaf in the fixture, to be checked for absolute paths.
fn leaves<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(s) => out.push(s),
        serde_json::Value::Array(items) => {
            for item in items {
                leaves(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                leaves(item, out);
            }
        }
        _ => {}
    }
}

fn check_keys(value: &serde_json::Value, allowed: &[&str], at: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{at}: not an object"));
    for key in object.keys() {
        assert!(
            allowed.contains(&key.as_str()),
            "{at}: field `{key}` is not adapter-readable"
        );
    }
}

#[test]
fn every_fixture_is_a_sanitized_probe_record() {
    let files = fixture_files();
    assert!(!files.is_empty(), "no fixtures under {FIXTURES}");
    for file in &files {
        let record: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display())),
        )
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", file.display()));

        for key in ["event", "ts_enter", "ts_exit", "payload"] {
            assert!(
                record.get(key).is_some(),
                "{}: missing `{key}`",
                file.display()
            );
        }
        let name = file.file_name().unwrap().to_string_lossy();
        let event = record["event"].as_str().unwrap();
        assert!(
            name.ends_with(&format!("-{}.json", event.to_lowercase())),
            "{}: filename must end with the event name in lower case",
            file.display()
        );

        let payload = &record["payload"];
        check_keys(payload, PAYLOAD_KEYS, &file.display().to_string());
        if let Some(input) = payload.get("tool_input") {
            check_keys(
                input,
                TOOL_INPUT_KEYS,
                &format!("{}:tool_input", file.display()),
            );
        }
        if let Some(response) = payload.get("tool_response") {
            check_keys(
                response,
                TOOL_RESPONSE_KEYS,
                &format!("{}:tool_response", file.display()),
            );
        }
        if let Some(tasks) = payload.get("background_tasks") {
            for (i, task) in tasks.as_array().unwrap().iter().enumerate() {
                check_keys(
                    task,
                    BACKGROUND_TASK_KEYS,
                    &format!("{}:background_tasks[{i}]", file.display()),
                );
            }
        }

        // StopFailure's `error` is a code; a failed tool's `error` is free
        // text and must have been stripped.
        if let Some(error) = payload.get("error") {
            let code = error.as_str().unwrap_or("");
            assert!(
                !code.is_empty() && code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{}: `error` must be a snake_case code, not `{error}`",
                file.display()
            );
        }

        let mut strings = Vec::new();
        leaves(payload, &mut strings);
        for s in strings {
            assert!(
                !s.starts_with('/') && !s.contains("/Users/") && !s.contains("/home/"),
                "{}: `{s}` looks like a local path",
                file.display()
            );
        }
    }
}

/// A scenario without an `expected.tsv` replay record cannot be verified, so
/// the directory layout refuses to let one slip in untested.
#[test]
fn every_scenario_has_an_expected_replay() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES);
    for entry in fs::read_dir(&root).unwrap() {
        let dir = entry.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        let expected = dir.join("expected.tsv");
        assert!(
            expected.exists(),
            "{} has no expected.tsv - record one with TAS_REPLAY_WRITE=1",
            dir.display()
        );
        let rows = fs::read_to_string(&expected)
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with('#'))
            .count();
        let fixtures = fs::read_dir(&dir)
            .unwrap()
            .filter(|f| {
                f.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    == Some("json")
            })
            .count();
        assert_eq!(
            rows,
            fixtures,
            "{}: expected.tsv has {rows} rows for {fixtures} fixtures",
            dir.display()
        );
    }
}

fn read_record(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// A tracked adapter cancels a background agent by the `task_id` of
/// `PostToolUse(TaskStop)`, which must equal the `agent_id` its
/// `SubagentStart` opened. The S5 capture is the evidence; this keeps the
/// sanitizer from silently dropping either half of it.
#[test]
fn task_stop_cancels_by_the_subagent_start_id() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURES)
        .join("s5-model-cancellation");
    let mut started = Vec::new();
    let mut cancelled = Vec::new();
    for file in fixture_files().iter().filter(|f| f.starts_with(&dir)) {
        let payload = read_record(file)["payload"].clone();
        match (
            payload["hook_event_name"].as_str(),
            payload["tool_name"].as_str(),
        ) {
            (Some("SubagentStart"), _) => started.push(payload["agent_id"].clone()),
            (Some("PostToolUse"), Some("TaskStop")) => {
                cancelled.push(payload["tool_input"]["task_id"].clone());
                cancelled.push(payload["tool_response"]["task_id"].clone());
            }
            _ => {}
        }
    }
    assert_eq!(started.len(), 1, "S5 has one SubagentStart: {started:?}");
    assert_eq!(cancelled.len(), 2, "S5 has one TaskStop: {cancelled:?}");
    assert!(
        started[0].is_string() && cancelled.iter().all(|id| *id == started[0]),
        "TaskStop ids {cancelled:?} do not match SubagentStart {started:?}"
    );
}
