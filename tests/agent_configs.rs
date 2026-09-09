//! Validate the shipped agent hook drop-in files.
//!
//! Every JSON drop-in under `share/agents/` is walked and every command string
//! that starts with `tmux-agent-status ` must invoke one of the supported
//! subcommands with a valid state. TOML drop-ins are checked for the expected
//! notify command shape without a full parser.

use std::fs;
use std::path::{Path, PathBuf};

const STATES: [&str; 4] = ["working", "waiting", "done", "error"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn agents_dir() -> PathBuf {
    repo_root().join("share/agents")
}

#[test]
fn every_json_drop_in_has_valid_commands() {
    let dir = agents_dir();
    if !dir.is_dir() {
        panic!("{} does not exist", dir.display());
    }

    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let agent_dir = entry.path();
        if !agent_dir.is_dir() {
            continue;
        }
        for file in fs::read_dir(&agent_dir).unwrap() {
            let file = file.unwrap().path();
            if file.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            checked += 1;
            validate_json_file(&file);
        }
    }

    assert!(
        checked > 0,
        "no JSON drop-in files found under {}",
        dir.display()
    );
}

fn validate_json_file(path: &Path) {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
    walk(&value, path);
}

fn walk(value: &serde_json::Value, path: &Path) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(cmd) = s.strip_prefix("tmux-agent-status ") {
                validate_command(cmd, path);
            }
        }
        serde_json::Value::Array(arr) => arr.iter().for_each(|v| walk(v, path)),
        serde_json::Value::Object(map) => map.values().for_each(|v| walk(v, path)),
        _ => {}
    }
}

fn validate_command(cmd: &str, path: &Path) {
    // Commands may be followed by shell redirections or a `printf` wrapper.
    // Only the leading tmux-agent-status arguments are validated.
    let head = cmd
        .split(|c: char| {
            c.is_whitespace() || c == '>' || c == '<' || c == '|' || c == ';' || c == '&'
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if head.is_empty() {
        panic!("{}: command too short: {cmd}", path.display());
    }

    match head[0] {
        "set" => {
            if head.len() < 2 {
                panic!("{}: `set` requires a state: {cmd}", path.display());
            }
            assert!(
                STATES.contains(&head[1]),
                "{}: `set` has unknown state `{}`: {cmd}",
                path.display(),
                head[1]
            );
        }
        "reset" | "finish" | "clear-window" => {}
        other => panic!("{}: unknown subcommand `{other}`: {cmd}", path.display()),
    }
}

#[test]
fn every_toml_drop_in_uses_notify_with_its_agent() {
    let dir = agents_dir();
    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let agent_dir = entry.path();
        if !agent_dir.is_dir() {
            continue;
        }
        let agent = agent_dir.file_name().unwrap().to_string_lossy();
        for file in fs::read_dir(&agent_dir).unwrap() {
            let file = file.unwrap().path();
            if file.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            checked += 1;
            validate_toml_file(&file, &agent);
        }
    }
    assert!(
        checked > 0,
        "no TOML drop-in files found under {}",
        dir.display()
    );
}

fn validate_toml_file(path: &Path, agent: &str) {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let expected = format!("tmux-agent-status notify --agent {agent} --stdin");
    for line in text.lines() {
        if let Some(value) = line
            .split("command = \"")
            .nth(1)
            .and_then(|s| s.split('"').next())
        {
            assert_eq!(
                value,
                expected,
                "{}: every TOML command must be `{}`",
                path.display(),
                expected
            );
        }
    }
}
