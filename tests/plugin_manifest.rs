//! The Claude Code hook set exists three times: as the plugin's `hooks/hooks.json`, which is what
//! actually runs, as `share/agents/claude-code/hooks.json`, which is what an installed user copies or
//! merges, and as the README's table, which is what a reader believes. These tests fail when they
//! stop saying the same thing, and when the plugin's version stops tracking the crate's.
//!
//! Everything here parses files this repository owns, so the parsing is deliberately strict: a
//! shape it does not recognise is a failure, not something to skip over.

use std::fs;
use std::path::{Path, PathBuf};

/// One watched event, in the form both sources can be reduced to.
///
/// `matcher` is `None` for "all", which the manifest expresses by omitting the key and the README
/// by writing "all".
type HookEntry = (String, Option<String>, String);

/// Sort, and reject a repeated entry rather than collapsing it.
///
/// A set would silently absorb a second identical hook entry, which is the one manifest mistake
/// with a user-visible cost: the state write is idempotent, so the glyph stays right, but every
/// turn end rings the bell twice.
fn normalise(mut entries: Vec<HookEntry>, source: &str) -> Vec<HookEntry> {
    entries.sort();
    for pair in entries.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "{source} lists the same hook entry twice, which would fire it twice"
        );
    }
    entries
}

const STATES: [&str; 4] = ["working", "waiting", "done", "error"];
const EXPECTED_EVENTS: usize = 8;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn plugin_dir() -> PathBuf {
    repo_root().join("plugins/tmux-agent-status")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The arguments passed to the binary, rejecting anything outside the exact hook command surface.
fn arguments_of(command: &str) -> String {
    let arguments = command
        .strip_prefix("tmux-agent-status ")
        .unwrap_or_else(|| panic!("hook command does not invoke `tmux-agent-status`: {command}"));
    if let Some(state) = arguments.strip_prefix("set ") {
        assert!(
            STATES.contains(&state),
            "hook command sets an unknown state: {command}"
        );
    } else {
        assert!(
            ["reset", "finish"].contains(&arguments),
            "hook command has unknown arguments: {command}"
        );
    }
    arguments.to_string()
}

fn manifest_entries() -> Vec<HookEntry> {
    let path = plugin_dir().join("hooks/hooks.json");
    let json: serde_json::Value = serde_json::from_str(&read(&path))
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
    let events = json["hooks"]
        .as_object()
        .unwrap_or_else(|| panic!("{} has no `hooks` object", path.display()));

    let mut entries = Vec::new();
    for (event, groups) in events {
        for group in groups
            .as_array()
            .unwrap_or_else(|| panic!("`{event}` is not an array"))
        {
            let matcher = match &group["matcher"] {
                serde_json::Value::Null => None,
                serde_json::Value::String(s) => Some(s.clone()),
                other => panic!("`{event}` has a non-string matcher: {other}"),
            };
            for hook in group["hooks"]
                .as_array()
                .unwrap_or_else(|| panic!("`{event}` has no `hooks` array"))
            {
                assert_eq!(
                    hook["type"].as_str(),
                    Some("command"),
                    "`{event}` has a hook that is not a command"
                );
                let command = hook["command"]
                    .as_str()
                    .unwrap_or_else(|| panic!("`{event}` has a hook with no command"));
                entries.push((event.clone(), matcher.clone(), arguments_of(command)));
            }
        }
    }
    normalise(entries, "hooks.json")
}

/// Split one table row into cells on the `|` separators, leaving the `\|` a markdown cell needs to
/// carry a literal pipe - the `PreToolUse` matcher is exactly that case.
fn split_cells(row: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for c in row.chars() {
        match c {
            '|' if !escaped => cells.push(String::new()),
            _ => {
                escaped = c == '\\' && !escaped;
                cells.last_mut().expect("never empty").push(c);
            }
        }
    }
    cells.iter().map(|c| c.trim().to_string()).collect()
}

/// The first markdown table after the given heading, as rows of trimmed cells.
fn table_after(markdown: &str, heading: &str) -> Vec<Vec<String>> {
    let section = markdown
        .split_once(heading)
        .unwrap_or_else(|| panic!("README has no `{heading}` heading"))
        .1;
    let mut rows = Vec::new();
    for line in section.lines().skip_while(|l| !l.starts_with('|')) {
        if !line.starts_with('|') {
            break;
        }
        rows.push(split_cells(line.trim_matches('|')));
    }
    assert!(!rows.is_empty(), "no table found after `{heading}`");
    rows
}

/// A README cell holding a value: backticks stripped, an escaped pipe restored.
fn unformat(cell: &str) -> String {
    cell.trim().trim_matches('`').replace("\\|", "|")
}

fn readme_entries() -> Vec<HookEntry> {
    let readme = read(&repo_root().join("README.md"));
    let rows = table_after(&readme, "#### Claude Code");
    assert_eq!(
        rows[0],
        ["Event", "Matcher", "Command"],
        "the Claude Code hook table's header changed"
    );

    let mut entries = Vec::new();
    // Row 0 is the header, row 1 the `| --- |` separator.
    for row in &rows[2..] {
        assert_eq!(row.len(), 3, "hook table row is not three cells: {row:?}");
        let matcher = if row[1].starts_with("all") {
            None
        } else {
            Some(unformat(&row[1]))
        };
        entries.push((unformat(&row[0]), matcher, unformat(&row[2])));
    }
    normalise(entries, "the README's Claude Code table")
}

fn manifest_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&read(path))
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

#[test]
fn manifest_and_readme_watch_the_same_events() {
    let manifest = manifest_entries();
    let readme = readme_entries();
    assert_eq!(
        manifest, readme,
        "plugins/tmux-agent-status/hooks/hooks.json and the README's Claude Code table disagree"
    );
    assert_eq!(
        manifest.len(),
        EXPECTED_EVENTS,
        "the hook set changed size; 001 decides which events are watched, so update it there first"
    );
}

#[test]
fn plugin_version_tracks_the_crate_version() {
    let plugin = manifest_json(&plugin_dir().join(".claude-plugin/plugin.json"));
    let cargo = read(&repo_root().join("Cargo.toml"));
    let crate_version = cargo
        .lines()
        .find_map(|l| l.strip_prefix("version = "))
        .expect("Cargo.toml has no version")
        .trim()
        .trim_matches('"');
    assert_eq!(
        plugin["version"].as_str(),
        Some(crate_version),
        "plugin.json's version must be bumped with the crate's"
    );
}

#[test]
fn marketplace_points_at_the_plugin() {
    let marketplace = manifest_json(&repo_root().join(".claude-plugin/marketplace.json"));
    let plugin = manifest_json(&plugin_dir().join(".claude-plugin/plugin.json"));

    let listed = marketplace["plugins"]
        .as_array()
        .expect("marketplace.json has no `plugins` array");
    assert_eq!(listed.len(), 1, "this marketplace ships exactly one plugin");
    assert_eq!(
        listed[0]["name"], plugin["name"],
        "the marketplace entry and plugin.json disagree on the name"
    );

    let source = listed[0]["source"]
        .as_str()
        .expect("the marketplace entry has no string `source`");
    let dir = repo_root().join(source.trim_start_matches("./"));
    assert!(
        dir.join(".claude-plugin/plugin.json").is_file(),
        "the marketplace entry's source {source} holds no plugin manifest"
    );
}

/// The shipped drop-in and the plugin's hooks are one file, reached by two paths.
///
/// The plugin is only reachable from a checkout or the marketplace; a user who installed the
/// binary through nix, a tarball or cargo gets `share/agents/claude-code/hooks.json` instead. Two
/// copies of a hook set drift, and a drifted copy is a glyph that is wrong for exactly the users
/// who never see the plugin, so the shipped path is a symlink onto the plugin's file rather than a
/// copy of it. Every packaging route dereferences it - cargo, GNU `install` under nix, and the
/// release workflow's `cp -RL` - so replacing the link with a real file is the one regression this
/// guards, and it would be invisible in a diff.
#[test]
fn the_shipped_claude_drop_in_is_the_plugin_hook_set() {
    let plugin = plugin_dir().join("hooks/hooks.json");
    let shipped = repo_root().join("share/agents/claude-code/hooks.json");

    let link = fs::symlink_metadata(&shipped)
        .unwrap_or_else(|e| panic!("cannot stat {}: {e}", shipped.display()));
    assert!(
        link.file_type().is_symlink(),
        "{} must stay a symlink onto {}, not become a second copy of it",
        shipped.display(),
        plugin.display()
    );
    assert_eq!(
        fs::canonicalize(&shipped).expect("the shipped drop-in link dangles"),
        fs::canonicalize(&plugin).expect("the plugin hook set is missing"),
        "{} points somewhere other than the plugin hook set",
        shipped.display()
    );
    assert_eq!(
        read(&shipped),
        read(&plugin),
        "{} and {} must be identical",
        shipped.display(),
        plugin.display()
    );
}
