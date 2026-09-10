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
    // Commands may be followed by shell redirections, a `printf` wrapper, or the
    // `--json` flag. Only the leading tmux-agent-status arguments are validated;
    // `--json` is an output-formatting detail for agents that parse stdout.
    let mut head = cmd
        .split(|c: char| {
            c.is_whitespace() || c == '>' || c == '<' || c == '|' || c == ';' || c == '&'
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if head.is_empty() {
        panic!("{}: command too short: {cmd}", path.display());
    }
    if head.last() == Some(&"--json") {
        head.pop();
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
        "notify" => {
            assert!(
                head.contains(&"--agent"),
                "{}: `notify` requires --agent: {cmd}",
                path.display()
            );
        }
        "reset" | "finish" | "clear-window" => {}
        other => panic!("{}: unknown subcommand `{other}`: {cmd}", path.display()),
    }
}

/// The command a docs table row must carry for the state it names.
///
/// `reset` and `finish` are their own commands; every other row is a state and
/// so a `set`. This is the rule that catches a turn-end row wired to `finish`,
/// which writes the glyph but never rings the bell.
fn expected_command(state: &str) -> Option<String> {
    match state {
        "reset" | "finish" => Some(format!("tmux-agent-status {state}")),
        state if STATES.contains(&state) => Some(format!("tmux-agent-status set {state}")),
        _ => None,
    }
}

/// The `tmux-agent-status ...` commands a drop-in file actually invokes, with
/// shell redirections, the `printf` wrapper, and the trailing `--json` flag
/// stripped off. `--json` is an output-formatting detail for agents that parse
/// stdout; the mapping table documents the base command.
fn commands_in_file(path: &Path) -> Vec<String> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
    let mut found = Vec::new();
    collect(&value, &mut found);
    found
}

fn collect(value: &serde_json::Value, found: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(cmd) = s.strip_prefix("tmux-agent-status ") {
                let head = cmd.split(['>', '<', '|', ';', '&']).next().unwrap_or(cmd);
                let head = head.trim().strip_suffix("--json").unwrap_or(head).trim();
                found.push(format!("tmux-agent-status {head}"));
            }
        }
        serde_json::Value::Array(arr) => arr.iter().for_each(|v| collect(v, found)),
        serde_json::Value::Object(map) => map.values().for_each(|v| collect(v, found)),
        _ => {}
    }
}

/// A markdown table row's cells. A `\|` inside a cell is one of the agent's own
/// matcher alternations, not a column separator.
fn split_row(row: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for c in row.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            '|' if !escaped => cells.push(String::new()),
            _ => {
                if escaped && c != '|' {
                    cells.last_mut().unwrap().push('\\');
                }
                escaped = false;
                cells.last_mut().unwrap().push(c);
            }
        }
    }
    cells.iter().map(|cell| cell.trim().to_owned()).collect()
}

/// The `state -> command` rows of a docs page's mapping table.
///
/// Rows whose command cell is a dash are states the agent cannot express and
/// carry no command.
fn docs_table(path: &Path) -> Vec<(String, String)> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells = split_row(line.trim_matches('|'));
        if cells.len() < 3 {
            continue;
        }
        let state = cells[0].as_str();
        let command = cells[2].trim_matches('`');
        if state == "State" || state.starts_with("---") || command == "—" {
            continue;
        }
        rows.push((state.to_owned(), command.to_owned()));
    }
    assert!(
        !rows.is_empty(),
        "{}: no mapping table rows found",
        path.display()
    );
    rows
}

/// The shipped file and the page that documents it cannot drift.
///
/// Two things are asserted: a row's command matches the state it claims (a
/// `done` row wired to `finish` writes ✅ and never rings), and the file invokes
/// exactly the commands the table lists - no more, no fewer.
#[test]
fn every_json_drop_in_matches_its_docs_table() {
    let mut checked = 0;
    for entry in fs::read_dir(agents_dir()).unwrap() {
        let agent_dir = entry.unwrap().path();
        if !agent_dir.is_dir() {
            continue;
        }
        let agent = agent_dir.file_name().unwrap().to_string_lossy().to_string();
        let files: Vec<PathBuf> = fs::read_dir(&agent_dir)
            .unwrap()
            .map(|f| f.unwrap().path())
            .filter(|f| f.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        if files.is_empty() {
            continue;
        }
        let page = repo_root().join(format!("docs/agents/{agent}.md"));
        assert!(
            page.is_file(),
            "{} ships a drop-in but has no docs page at {}",
            agent,
            page.display()
        );

        let mut documented: Vec<String> = Vec::new();
        for (state, command) in docs_table(&page) {
            let expected = expected_command(&state).unwrap_or_else(|| {
                panic!(
                    "{}: row `{state}` is not one of the four states, `reset` or `finish`",
                    page.display()
                )
            });
            assert_eq!(
                command,
                expected,
                "{}: the `{state}` row must run `{expected}`",
                page.display()
            );
            documented.push(command);
        }
        documented.sort();
        documented.dedup();

        let mut used: Vec<String> = files.iter().flat_map(|f| commands_in_file(f)).collect();
        used.sort();
        used.dedup();
        assert_eq!(
            used,
            documented,
            "{agent}: {} and the mapping table in {} list different commands",
            agent_dir.display(),
            page.display()
        );
        checked += 1;
    }
    assert!(checked > 0, "no JSON drop-ins were checked against a page");
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
