//! Validate the shipped agent hook drop-in files.
//!
//! Every JSON drop-in under `share/agents/` is walked and every command string
//! that starts with `tmux-agent-status ` must invoke one of the supported
//! subcommands with a valid state. TOML drop-ins are checked for the expected
//! notify command shape without a full parser.

use std::fs;
use std::path::{Path, PathBuf};

use tmux_agent_status::state::State;

mod support;

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
            if s.starts_with("tmux-agent-status ") {
                support::command::parse_command(&path.display().to_string(), s);
            }
        }
        serde_json::Value::Array(arr) => arr.iter().for_each(|v| walk(v, path)),
        serde_json::Value::Object(map) => map.values().for_each(|v| walk(v, path)),
        _ => {}
    }
}

/// The command a docs table row must carry for the state it names.
///
/// `start`, `reset` and `finish` are their own commands; every other row is a state and
/// so a `set`. This is the rule that catches a turn-end row wired to `finish`,
/// which writes the glyph but never rings the bell.
fn expected_command(state: &str) -> Option<String> {
    match state {
        "start" => Some("tmux-agent-status start".to_string()),
        "reset" => Some("tmux-agent-status reset".to_string()),
        "finish" => Some("tmux-agent-status finish".to_string()),
        state if State::ALL.iter().any(|s| s.name() == state) => {
            Some(format!("tmux-agent-status set {state}"))
        }
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
    collect(&value, &mut found, path);
    found
}

fn collect(value: &serde_json::Value, found: &mut Vec<String>, path: &Path) {
    match value {
        serde_json::Value::String(s) => {
            if s.starts_with("tmux-agent-status ") {
                let parsed = support::command::parse_command(&path.display().to_string(), s);
                found.push(parsed.as_str());
            }
        }
        serde_json::Value::Array(arr) => arr.iter().for_each(|v| collect(v, found, path)),
        serde_json::Value::Object(map) => map.values().for_each(|v| collect(v, found, path)),
        _ => {}
    }
}

/// The `state -> command` rows of a docs page's mapping table.
///
/// Rows whose command cell is a dash are states the agent cannot express and
/// carry no command.
fn docs_table(path: &Path) -> Vec<(String, String)> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut rows = Vec::new();
    for row in support::markdown::table_after(&text, "## Supported states") {
        if row.len() < 3 {
            continue;
        }
        let state = row[0].as_str();
        let command = row[2].trim_matches('`');
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
                    "{}: row `{state}` is not one of the four states, `start`, `reset` or `finish`",
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

/// A new agent cannot be added without `register` learning about it.
///
/// Every directory under `share/agents/` has a row in `register`'s table,
/// and every row's embedded contents are byte-for-byte the shipped file. That
/// second half is what keeps `include_str!` honest: the embedded bytes *are*
/// the shipped files, so these drift checks still mean something about what a
/// user ends up with.
#[test]
fn every_shipped_drop_in_has_a_row_in_the_register_table() {
    use tmux_agent_status::register::agents;

    let mut checked = 0;
    for entry in fs::read_dir(agents_dir()).unwrap() {
        let agent_dir = entry.unwrap().path();
        if !agent_dir.is_dir() {
            continue;
        }
        let name = agent_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let row = agents::by_name(&name).unwrap_or_else(|| {
            panic!(
                "share/agents/{name}/ ships a drop-in but `register` has no row for it; \
                 valid names are {:?}",
                agents::names()
            )
        });

        let files: Vec<PathBuf> = fs::read_dir(&agent_dir)
            .unwrap()
            .map(|file| file.unwrap().path())
            .filter(|file| file.file_name().is_some())
            .collect();
        assert_eq!(
            files.len(),
            1,
            "share/agents/{name}/ ships {} files; `register` embeds one",
            files.len()
        );
        let shipped = fs::read_to_string(&files[0]).unwrap();
        assert_eq!(
            row.contents,
            shipped,
            "the `register` embedded copy of {} has drifted from the shipped file",
            files[0].display()
        );
        checked += 1;
    }
    assert!(checked > 0, "no shipped drop-ins were checked");
}

/// The two agents with no drop-in directory still have to match their page.
///
/// Claude Code's entries live in the plugin that ships them, and Gemini has no
/// drop-in mechanism at all - its hooks live in `settings.json` and nowhere
/// else. Both are embedded, so both need something holding them to the docs.
#[test]
fn the_agents_without_a_drop_in_directory_match_their_source() {
    use tmux_agent_status::register::agents;

    let claude = agents::by_name("claude-code").expect("a row for Claude Code");
    let plugin = repo_root().join("plugins/tmux-agent-status/hooks/hooks.json");
    assert_eq!(
        claude.contents,
        fs::read_to_string(&plugin).unwrap(),
        "the `register` Claude Code entries have drifted from {}",
        plugin.display()
    );

    // Gemini's block is the one `docs/agents/gemini.md` tells a user to merge
    // by hand, so `register` must offer to write exactly that.
    let gemini = agents::by_name("gemini").expect("a row for Gemini");
    let page = repo_root().join("docs/agents/gemini.md");
    let text = fs::read_to_string(&page).unwrap();
    let documented = fenced_json(&text).unwrap_or_else(|| {
        panic!(
            "{}: no fenced json block to compare against",
            page.display()
        )
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(gemini.contents).unwrap(),
        serde_json::from_str::<serde_json::Value>(&documented).unwrap(),
        "the `register` Gemini block has drifted from {}",
        page.display()
    );
}

/// The first ```json block in a page.
fn fenced_json(text: &str) -> Option<String> {
    let start = text.find("```json\n")? + "```json\n".len();
    let end = start + text[start..].find("\n```")?;
    Some(text[start..end].to_owned())
}
