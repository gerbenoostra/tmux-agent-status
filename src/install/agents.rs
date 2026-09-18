//! The agent table, and the three ways hook entries reach a machine.
//!
//! Delivery is plugin, then a file that is ours alone, then a merge into a file
//! the user maintains - in that order, so the riskiest route is the last resort.
//! On a machine with `claude` on `PATH`, `~/.claude/settings.json` is never
//! touched by us.
//!
//! Every drop-in is embedded with `include_str!` rather than read from
//! `share/agents/` at runtime: `cargo install` ships the binary and nothing
//! else, so a runtime path lookup would leave the largest install route unable
//! to install anything. It also means the embedded bytes *are* the shipped
//! files, which is what keeps the drift test meaningful.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{Map, Value};

use super::write::Plan;
use super::{Home, append_marked};

/// What a command of ours looks like, wherever it appears in a document.
///
/// This prefix is the merge key, and it is what a future `uninstall` will use
/// to find the same entries again.
pub const COMMAND_PREFIX: &str = "tmux-agent-status ";

/// How the entries get to the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Claude Code: the plugin, which writes its own bookkeeping and leaves
    /// `~/.claude/settings.json` alone. The merge below is the named fallback,
    /// reached only when `claude` is absent or `--claude-route=settings` asks.
    Plugin,
    /// A whole file that is ours alone. Lowest risk: nothing to merge.
    OwnFile,
    /// A file the user maintains. The safe write exists for this row.
    SharedFile,
}

/// How our entries sit inside the target file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The file is ours, so it is written whole.
    Whole,
    /// Event keys nested under a `hooks` object.
    JsonUnderHooks,
    /// Event keys at the top level, with no wrapping `hooks` key. Nesting them
    /// under `hooks` gives a config the agent ignores in silence.
    JsonTopLevel,
    /// A marked block of `[[hooks]]` tables appended to a TOML file.
    TomlBlock,
}

/// `$HOME`-relative, or relative to the config directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Home(&'static str),
    Config(&'static str),
}

impl Place {
    fn resolve(self, home: &Home) -> PathBuf {
        match self {
            Place::Home(tail) => home.join(tail),
            Place::Config(tail) => home.config(tail),
        }
    }
}

/// One agent, and everything the installer needs to know about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Agent {
    /// The name `--agents=` takes, and the directory name under `share/agents/`
    /// for the agents that ship a drop-in.
    pub name: &'static str,
    /// What to call it in a list a human reads.
    pub label: &'static str,
    /// Its command, for the `PATH` half of detection. Confirmed against a real
    /// install rather than guessed: a wrong binary name is a wrong detection,
    /// and this repo does not ship guesses.
    pub command: &'static str,
    /// Its configuration directory, for the other half.
    directory: Place,
    pub delivery: Delivery,
    /// Where the write lands. For Claude Code this is the fallback.
    target: Place,
    pub shape: Shape,
    /// The drop-in, embedded.
    pub contents: &'static str,
}

/// User scope for every agent, because a per-project install is a per-project
/// surprise. Devin has no user-scope drop-in, so it gets the `config.json`
/// merge and its project file is mentioned rather than written.
pub const AGENTS: [Agent; 10] = [
    Agent {
        name: "claude-code",
        label: "Claude Code",
        command: "claude",
        directory: Place::Home(".claude"),
        delivery: Delivery::Plugin,
        target: Place::Home(".claude/settings.json"),
        shape: Shape::JsonUnderHooks,
        contents: include_str!("../../plugins/tmux-agent-status/hooks/hooks.json"),
    },
    Agent {
        name: "codex",
        label: "Codex CLI",
        command: "codex",
        directory: Place::Home(".codex"),
        delivery: Delivery::SharedFile,
        target: Place::Home(".codex/hooks.json"),
        shape: Shape::JsonUnderHooks,
        contents: include_str!("../../share/agents/codex/hooks.json"),
    },
    Agent {
        name: "copilot",
        label: "GitHub Copilot CLI",
        command: "copilot",
        directory: Place::Home(".copilot"),
        delivery: Delivery::OwnFile,
        target: Place::Home(".copilot/hooks/tmux-agent-status.json"),
        shape: Shape::Whole,
        contents: include_str!("../../share/agents/copilot/tmux-agent-status.json"),
    },
    Agent {
        name: "cursor",
        label: "Cursor",
        command: "cursor-agent",
        directory: Place::Home(".cursor"),
        delivery: Delivery::SharedFile,
        target: Place::Home(".cursor/hooks.json"),
        shape: Shape::JsonUnderHooks,
        contents: include_str!("../../share/agents/cursor/hooks.json"),
    },
    Agent {
        name: "devin",
        label: "Devin CLI",
        command: "devin",
        directory: Place::Config("devin"),
        delivery: Delivery::SharedFile,
        // There is no user-level `hooks.v1.json`; the drop-in's top-level
        // object *is* the hooks object, so it nests under `hooks` here.
        target: Place::Config("devin/config.json"),
        shape: Shape::JsonUnderHooks,
        contents: include_str!("../../share/agents/devin/hooks.v1.json"),
    },
    Agent {
        name: "droid",
        label: "Droid (Factory)",
        command: "droid",
        directory: Place::Home(".factory"),
        delivery: Delivery::SharedFile,
        target: Place::Home(".factory/hooks.json"),
        // Droid puts the event names at the top level. Nesting them under
        // `hooks` gives a config Droid ignores silently.
        shape: Shape::JsonTopLevel,
        contents: include_str!("../../share/agents/droid/hooks.json"),
    },
    Agent {
        name: "gemini",
        label: "Gemini CLI",
        command: "gemini",
        directory: Place::Home(".gemini"),
        delivery: Delivery::SharedFile,
        target: Place::Home(".gemini/settings.json"),
        shape: Shape::JsonUnderHooks,
        contents: GEMINI,
    },
    Agent {
        name: "grok",
        label: "Grok CLI",
        command: "grok",
        directory: Place::Home(".grok"),
        delivery: Delivery::OwnFile,
        target: Place::Home(".grok/hooks/tmux-agent-status.json"),
        shape: Shape::Whole,
        contents: include_str!("../../share/agents/grok/tmux-agent-status.json"),
    },
    Agent {
        name: "kiro",
        label: "Kiro",
        command: "kiro-cli",
        directory: Place::Home(".kiro"),
        delivery: Delivery::OwnFile,
        target: Place::Home(".kiro/hooks/tmux-agent-status.json"),
        shape: Shape::Whole,
        contents: include_str!("../../share/agents/kiro/tmux-agent-status.json"),
    },
    Agent {
        name: "mistral-vibe",
        label: "Mistral Vibe",
        command: "vibe",
        directory: Place::Home(".vibe"),
        delivery: Delivery::SharedFile,
        target: Place::Home(".vibe/hooks.toml"),
        shape: Shape::TomlBlock,
        contents: include_str!("../../share/agents/mistral-vibe/hooks.toml"),
    },
];

/// Gemini has no drop-in file to ship, because it has no drop-in mechanism:
/// its hooks live inside `settings.json` and nowhere else. This is the block
/// `docs/agents/gemini.md` tells a user to merge by hand, and a drift test
/// holds the two together.
const GEMINI: &str = r#"{
  "hooks": {
    "SessionStart": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ],
    "PreToolUse": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ],
    "PostToolUse": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ],
    "SessionEnd": [
      { "command": "tmux-agent-status notify --agent gemini --stdin", "type": "command" }
    ]
  }
}
"#;

/// The agent that name refers to.
///
/// A name that is not here is a usage error rather than a silent skip: a
/// typo'd `--agents=cursur` must never be read as "install nothing,
/// successfully".
pub fn by_name(name: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|agent| agent.name == name)
}

/// Every valid `--agents=` name, for the error message that lists them.
pub fn names() -> Vec<&'static str> {
    AGENTS.iter().map(|agent| agent.name).collect()
}

/// Why an agent is, or is not, preselected.
///
/// Both signals are reported rather than collapsed, so the user can see why
/// something is ticked. An agent is never installed without appearing in the
/// list, and an agent named explicitly installs even when undetected:
/// installing hooks before the agent is a legitimate order to do things in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detected {
    pub directory: bool,
    pub command: bool,
}

impl Detected {
    pub fn preselected(self) -> bool {
        self.directory || self.command
    }

    /// What to show beside the name in the list.
    pub fn why(self) -> &'static str {
        match (self.directory, self.command) {
            (true, true) => "config directory and command found",
            (true, false) => "config directory found",
            (false, true) => "command found",
            (false, false) => "not detected",
        }
    }
}

impl Agent {
    /// Where this agent's entries are written.
    pub fn target(&self, home: &Home) -> PathBuf {
        self.target.resolve(home)
    }

    /// The configuration directory whose presence half-detects it.
    pub fn directory(&self, home: &Home) -> PathBuf {
        self.directory.resolve(home)
    }

    pub fn detect(&self, home: &Home) -> Detected {
        Detected {
            directory: self.directory(home).is_dir(),
            command: on_path(self.command),
        }
    }

    /// Whether some bytes are a complete document in this file's own language.
    ///
    /// Handed to the safe write, where it is what tells a write that did not
    /// land intact from a writer that raced us.
    pub fn parses(&self) -> fn(&str) -> bool {
        match self.shape {
            Shape::TomlBlock => parses_as_toml,
            _ => parses_as_json,
        }
    }

    /// The document with our entries in it, or the news that they are already
    /// there.
    ///
    /// Pure: this is a function of the bytes on disk and the embedded drop-in,
    /// which is what lets the merge tests be exhaustive and what lets the plan
    /// phase show a diff before anything is written.
    pub fn merge(&self, current: &str) -> Result<Plan, Refused> {
        match self.shape {
            Shape::Whole => Ok(match current == self.contents {
                true => Plan::AlreadyInstalled,
                false => Plan::Write(self.contents.to_owned()),
            }),
            Shape::JsonUnderHooks | Shape::JsonTopLevel => self.merge_json(current),
            Shape::TomlBlock => self.append_toml(current),
        }
    }

    /// Whether our hooks are present but not in a block we manage.
    ///
    /// The case of a user who copied the shipped drop-in by hand before this
    /// subcommand existed. Marker-only idempotency would append a second copy
    /// of every hook to that file.
    pub fn present_unmarked(&self, current: &str) -> bool {
        self.shape == Shape::TomlBlock
            && !super::has_marked_block(current)
            && toml_names(self.contents)
                .iter()
                .all(|name| toml_names(current).contains(name))
            && !toml_names(self.contents).is_empty()
    }

    /// Mark a file whose hooks are already present as seen by us.
    ///
    /// For TOML this wraps the existing `[[hooks]]` tables that carry our names
    /// inside the marker block, so the future `uninstall` can remove what is
    /// ours without reparsing the whole file. TOML entries are still found by
    /// their `name` keys when they need to be merged or removed.
    pub fn adopt(&self, current: &str) -> String {
        match self.shape {
            Shape::TomlBlock => adopt_toml(current),
            _ => append_marked(current, ""),
        }
    }

    /// The event keys we own, which are the only ones a merge touches.
    ///
    /// The drop-in is embedded in this binary, and
    /// `every_json_drop_in_parses_as_an_object_of_events` below asserts that
    /// every one that reaches here is a JSON object - while
    /// `tests/agent_configs.rs` asserts those embedded bytes are the shipped
    /// file. Between them this is an invariant rather than something that can
    /// go wrong at a user's machine: a binary that got here shipped a broken
    /// drop-in.
    fn events(&self) -> Map<String, Value> {
        let parsed: Value = serde_json::from_str(self.contents).unwrap_or_else(|error| {
            panic!("{}'s embedded drop-in is not JSON: {error}", self.name)
        });
        let object = match self.shape {
            // The drop-in wraps its events; the target nests them the same way.
            Shape::JsonUnderHooks => parsed.get("hooks").cloned().unwrap_or(parsed),
            _ => parsed,
        };
        object
            .as_object()
            .cloned()
            .unwrap_or_else(|| panic!("{}'s embedded drop-in is not an object", self.name))
    }

    fn merge_json(&self, current: &str) -> Result<Plan, Refused> {
        let trimmed = current.trim();
        let mut document: Value = match trimmed.is_empty() {
            true => Value::Object(Map::new()),
            false => serde_json::from_str(current)
                .map_err(|error| Refused::Unparseable(error.to_string()))?,
        };
        let before = document.clone();
        let ours = self.events();

        let root = document
            .as_object_mut()
            .ok_or_else(|| Refused::Unparseable("the file is not a JSON object".to_owned()))?;
        let holder = match self.shape {
            Shape::JsonUnderHooks => {
                // A `hooks` key that is not an object is the user's data, and
                // replacing it is the one thing a merge must never do.
                let existing = root
                    .entry("hooks")
                    .or_insert_with(|| Value::Object(Map::new()));
                existing.as_object_mut().ok_or_else(|| {
                    Refused::Unparseable("`hooks` is present but is not an object".to_owned())
                })?
            }
            _ => root,
        };

        for (event, entries) in &ours {
            // Drop every existing entry of ours on this event, then insert
            // ours. Everything else - other events, other keys, key order,
            // unrelated hooks on the same event - is preserved, which is what
            // `preserve_order` makes true rather than aspirational.
            let kept: Vec<Value> = holder
                .get(event)
                .and_then(Value::as_array)
                .map(|existing| {
                    existing
                        .iter()
                        .filter(|entry| !is_ours(entry))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let mut merged = kept;
            merged.extend(entries.as_array().cloned().unwrap_or_default());
            holder.insert(event.clone(), Value::Array(merged));
        }

        if self.name == "devin" {
            // One unknown event key discards Devin's entire hook map, so the
            // merge must never contribute a key outside its documented events.
            devin_keys_are_known(holder)?;
        }

        // The no-op test is semantic: an already-correct hand-maintained file
        // must not be rewritten just because our serialiser spells it
        // differently.
        if document == before {
            return Ok(Plan::AlreadyInstalled);
        }
        // Serialising a `Value` cannot fail: there is no type in it that has
        // no JSON spelling.
        let mut out =
            serde_json::to_string_pretty(&document).expect("a JSON value serialises as JSON");
        out.push('\n');
        Ok(Plan::Write(out))
    }

    fn append_toml(&self, current: &str) -> Result<Plan, Refused> {
        // Idempotency keys on our own `name = "tmux-agent-status-..."` entries
        // rather than on the markers, because a user who copied the shipped
        // file by hand has the hooks and no markers, and appending would give
        // them a second copy of every one.
        let ours = toml_names(self.contents);
        let present = toml_names(current);
        if !ours.is_empty() && ours.iter().all(|name| present.contains(name)) {
            return Ok(Plan::AlreadyInstalled);
        }
        // "The original bytes are still a prefix" is not the check this needs:
        // it proves the old content survived, not that the new content is
        // valid. An appended `[[hooks]]` header is valid TOML after almost
        // anything, and the exception is real - a file whose last line is
        // inside an unclosed multi-line string swallows the block into it.
        if !ends_at_top_level(current) {
            return Err(Refused::TomlNotAtTopLevel);
        }
        Ok(Plan::Write(append_marked(current, self.contents)))
    }
}

/// Why an agent's entries will not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The user's file is not a document we can read.
    Unparseable(String),
    /// The file does not end at TOML's top level, so an append would land
    /// inside a string or a table that is not ours.
    TomlNotAtTopLevel,
    /// Devin's eight-key constraint would be broken by the result.
    DevinUnknownEvent(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Unparseable(why) => write!(f, "the file could not be read: {why}"),
            Refused::TomlNotAtTopLevel => f.write_str(
                "the file does not end at TOML's top level - its last line is inside a \
                 string or a table, and appending there would change what it means",
            ),
            Refused::DevinUnknownEvent(key) => write!(
                f,
                "`{key}` is not one of Devin's documented events, and one unknown key \
                 discards its entire hook map"
            ),
        }
    }
}

/// Devin's eight documented events, from `docs/agents/devin.md`.
///
/// One key outside this set discards the *whole* hook map - Devin warns
/// `Ignoring invalid value for "hooks" ... Using the default ({})` and every
/// hook stops firing. A Claude Code hook set is not a Devin hook set for
/// exactly this reason: it carries `StopFailure` and `Notification`, which
/// Devin does not know.
pub const DEVIN_EVENTS: [&str; 8] = [
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "PostCompaction",
];

/// Every key in the result, not only the ones we contributed.
///
/// A key the user already had is still fatal: one unknown name disables the
/// lot, ours included, so writing would produce a successful-looking install
/// where no hook ever fires. Refusing and saying which key it is leaves them
/// something to act on.
fn devin_keys_are_known(holder: &Map<String, Value>) -> Result<(), Refused> {
    match holder
        .keys()
        .find(|key| !DEVIN_EVENTS.contains(&key.as_str()))
    {
        Some(key) => Err(Refused::DevinUnknownEvent(key.clone())),
        None => Ok(()),
    }
}

/// Whether an entry is one of ours, whatever shape the agent wraps it in.
///
/// The command string is the key, and it is looked for anywhere inside the
/// entry: the agents nest it differently - `{"type","command"}` directly, or
/// inside a `hooks` array - and one rule that reads all of them is better than
/// four that each read one.
fn is_ours(entry: &Value) -> bool {
    match entry {
        Value::String(text) => text.trim_start().starts_with(COMMAND_PREFIX),
        Value::Array(items) => items.iter().any(is_ours),
        Value::Object(map) => map.values().any(is_ours),
        _ => false,
    }
}

fn parses_as_json(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok()
}

/// TOML has no parser here, so "a complete document" is "it ends where a TOML
/// document can end". That is the same lexical scan the append is guarded by,
/// and it catches a truncation, which is what it is for.
fn parses_as_toml(text: &str) -> bool {
    !text.is_empty() && ends_at_top_level(text)
}

/// The `name = "..."` values a TOML file carries.
///
/// Enough to answer "are our hooks in this file", without a TOML parser: our
/// names are a fixed set of literals we wrote ourselves.
fn toml_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("name"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .map(str::trim)
        .filter_map(|value| value.strip_prefix('"')?.strip_suffix('"'))
        .filter(|name| name.starts_with("tmux-agent-status"))
        .map(str::to_owned)
        .collect()
}

/// Whether a TOML file ends somewhere a `[[hooks]]` header may legally follow.
///
/// A lexical scan, tracking only what changes where a table header may appear:
/// `#` comments to end of line, single-line `"` and `'` strings, and `"""` and
/// `'''` multi-line strings. Perhaps sixty lines, exhaustively testable from
/// fixtures, and the difference between "probably fine" and "checked".
fn ends_at_top_level(text: &str) -> bool {
    let bytes: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            '#' => {
                while at < bytes.len() && bytes[at] != '\n' {
                    at += 1;
                }
            }
            '"' | '\'' => {
                let quote = bytes[at];
                let multi = bytes[at..].starts_with(&[quote, quote, quote]);
                let Some(end) = close(&bytes, at, quote, multi) else {
                    // Unterminated: the file ends inside a string, and our
                    // block would be swallowed into it.
                    return false;
                };
                at = end;
            }
            _ => at += 1,
        }
    }
    true
}

/// Wrap the existing `[[hooks]]` tables that carry our names in a marked block.
///
/// This is the "adopt" path: the hooks are already present but unmarked, and
/// the only change is to wrap them in markers so later runs know which block
/// is ours to manage. Any tables that are not ours are left outside the block.
fn adopt_toml(current: &str) -> String {
    let lines: Vec<&str> = current.lines().collect();
    let headers: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == "[[hooks]]")
        .map(|(index, _)| index)
        .collect();

    // Identify each [[hooks]] table that belongs to us by looking for a
    // `name = "tmux-agent-status..."` line before the next table header.
    let mut our_blocks: Vec<(usize, usize)> = Vec::new();
    for (index, &start) in headers.iter().enumerate() {
        let end = headers.get(index + 1).copied().unwrap_or(lines.len());
        let ours = lines[start..end].iter().any(|line| {
            line.trim()
                .strip_prefix("name")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .map(str::trim)
                .and_then(|value| value.strip_prefix('"')?.strip_suffix('"'))
                .is_some_and(|name| name.starts_with("tmux-agent-status"))
        });
        if ours {
            our_blocks.push((start, end));
        }
    }

    // Merge adjacent blocks so a single marked block wraps a contiguous set
    // of our tables, even if a comment or blank line sits between them.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in our_blocks {
        if let Some(last) = merged.last_mut() {
            if last.1 >= start {
                last.1 = end;
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut out = String::new();
    let mut position = 0;
    for (start, end) in merged {
        for line in &lines[position..start] {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(super::MARKER_START);
        out.push('\n');
        for line in &lines[start..end] {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(super::MARKER_END);
        out.push('\n');
        position = end;
    }
    for line in &lines[position..] {
        out.push_str(line);
        out.push('\n');
    }

    out
}

/// Where a string that starts at `at` closes, or `None` if it never does.
fn close(bytes: &[char], at: usize, quote: char, multi: bool) -> Option<usize> {
    let width = match multi {
        true => 3,
        false => 1,
    };
    let mut at = at + width;
    loop {
        match bytes.get(at)? {
            // Nothing is an escape inside a literal (single-quoted) string.
            '\\' if quote == '"' => at += 2,
            c if *c == quote && (!multi || bytes[at..].starts_with(&[quote, quote, quote])) => {
                return Some(at + width);
            }
            // A single-line string cannot span a newline; a file that ends
            // mid-line is still inside it.
            '\n' if !multi => return None,
            _ => at += 1,
        }
    }
}

/// Whether a command is on the `PATH` the installer inherited.
fn on_path(command: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(command).is_file()))
}

/// Which route the Claude Code step takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClaudeRoute {
    /// The plugin when `claude` is there, the merge when it is not.
    #[default]
    Auto,
    /// The plugin, failing if it cannot be used.
    Plugin,
    /// The merge into `~/.claude/settings.json`, which nobody gets by accident.
    Settings,
}

/// The marketplace to install from.
///
/// Derived from the manifest rather than written out, so a fork gets its own
/// slug by changing the file it was going to change anyway. The slug is the
/// project's own identity, not personal configuration: a stranger who cloned
/// the repo needs exactly this string.
pub fn default_marketplace() -> String {
    let repository = env!("CARGO_PKG_REPOSITORY");
    repository
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("/")
}

/// The plugin id an install registers, whatever marketplace it came from.
const PLUGIN_ID_PREFIX: &str = "tmux-agent-status@";

/// The Claude Code CLI, as something that can be addressed.
///
/// The program is a field rather than a literal so a test can point this at a
/// stub and watch what the route does with a CLI that answers, refuses, or is
/// not there at all - the three cases the plan says must behave differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claude {
    program: PathBuf,
}

impl Claude {
    /// The `claude` on the `PATH` we inherited, if there is one.
    ///
    /// `None` means the plugin route was never available, which is the only
    /// case that falls through to the merge.
    pub fn on_path() -> Option<Claude> {
        on_path("claude").then(|| Claude::at("claude"))
    }

    pub fn at(program: impl Into<PathBuf>) -> Claude {
        Claude {
            program: program.into(),
        }
    }

    /// Whether the plugin is already installed.
    ///
    /// `None` when the CLI cannot answer at all. Idempotency keys on this and
    /// stops: it must not go on to check *where* the marketplace points, and
    /// must never re-run `marketplace add` to "correct" it. A contributor's
    /// marketplace is a `directory` source pointing at their own checkout,
    /// which is how they test the plugin they are developing; re-adding it
    /// from GitHub would silently swap their working copy for a released one,
    /// and the symptom is maddening to trace back to an installer they ran
    /// once.
    pub fn plugin_installed(&self) -> Option<bool> {
        let listed = self.ask(&["plugin", "list", "--json"])?;
        let parsed: Value = serde_json::from_str(&listed).ok()?;
        Some(mentions_plugin(&parsed))
    }

    /// The two commands that install the plugin, in order.
    ///
    /// Both are fully non-interactive and machine-readable, confirmed against
    /// the installed CLI. `marketplace add` clones from GitHub, which is why
    /// the confirmation says so and why `--dry-run` prints them unrun.
    pub fn plugin_commands(&self, marketplace: &str) -> [Vec<String>; 2] {
        let program = self.program.to_string_lossy().into_owned();
        [
            vec![
                program.clone(),
                "plugin".to_owned(),
                "marketplace".to_owned(),
                "add".to_owned(),
                marketplace.to_owned(),
            ],
            vec![
                program,
                "plugin".to_owned(),
                "install".to_owned(),
                format!("{PLUGIN_ID_PREFIX}{}", marketplace_name(marketplace)),
                "-y".to_owned(),
                "--json".to_owned(),
            ],
        ]
    }

    /// Run the plugin install. The step fails rather than falling back.
    ///
    /// Falling back on failure is the tempting choice and the wrong one: it
    /// would edit `~/.claude/settings.json` on a machine where the user was
    /// promised it would not be, as the silent consequence of a network blip.
    /// A user who wants that outcome can have it by asking for it.
    pub fn install_plugin(&self, marketplace: &str) -> Result<(), String> {
        for command in self.plugin_commands(marketplace) {
            let (program, args) = command.split_first().expect("every command has a program");
            let out = output_retrying(Command::new(program).args(args).stdin(Stdio::null()))
                .map_err(|error| format!("{}: {error}", command.join(" ")))?;
            if !out.status.success() {
                return Err(format!(
                    "{} exited with {}\n{}",
                    command.join(" "),
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        }
        Ok(())
    }

    fn ask(&self, args: &[&str]) -> Option<String> {
        let out = output_retrying(
            Command::new(&self.program)
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::null()),
        )
        .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// Run a freshly-written executable, retrying a transient `ETXTBSY`.
///
/// A script just written and chmod'd can still be reported busy for a moment
/// on some filesystems - observed under the Linux Nix build sandbox, where a
/// stub written by a test and exec'd immediately after occasionally raced the
/// kernel's own close of the write handle. A real `claude` binary mid-upgrade
/// (its own file being replaced) can hit the same error, so the retry belongs
/// here rather than only in a test helper.
fn output_retrying(command: &mut Command) -> std::io::Result<Output> {
    retry_while_busy(|| command.output())
}

/// The backoff loop `output_retrying` runs, pulled out so it can be tested
/// without a real child process: `attempt` stands in for `Command::output`.
fn retry_while_busy<T>(mut attempt: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut delay = Duration::from_millis(5);
    loop {
        match attempt() {
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && delay < Duration::from_millis(200) =>
            {
                std::thread::sleep(delay);
                delay *= 2;
            }
            result => return result,
        }
    }
}

fn mentions_plugin(value: &Value) -> bool {
    match value {
        Value::String(text) => text.starts_with(PLUGIN_ID_PREFIX),
        Value::Array(items) => items.iter().any(mentions_plugin),
        Value::Object(map) => map.values().any(mentions_plugin),
        _ => false,
    }
}

/// The name a marketplace registers under, which is its last path segment.
fn marketplace_name(source: &str) -> String {
    source
        .trim_end_matches('/')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source)
        .to_owned()
}

/// Devin's project-scope file, which is mentioned and never written.
pub fn devin_project_file() -> &'static Path {
    Path::new(".devin/hooks.v1.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(name: &str) -> &'static Agent {
        by_name(name).expect("the agent is in the table")
    }

    fn written(plan: Plan) -> String {
        plan.written().expect("the merge produces a write")
    }

    /// The invariant `events` rests on, held by a test rather than by hope.
    /// `tests/agent_configs.rs` holds the other half: that these embedded
    /// bytes are byte-for-byte the file a user is shipped.
    #[test]
    fn every_json_drop_in_parses_as_an_object_of_events() {
        let mut checked = 0;
        for name in names() {
            let row = by_name(name).expect("a name from the table resolves");
            if !matches!(row.shape, Shape::JsonUnderHooks | Shape::JsonTopLevel) {
                continue;
            }
            assert!(!row.events().is_empty(), "{name} carries no events");
            checked += 1;
        }
        assert!(checked > 0, "no JSON drop-ins were checked");
    }

    /// The embedded drop-ins are an invariant the drift tests keep, so a
    /// binary that got here shipped a broken one. These two pin the message
    /// rather than the possibility.
    #[test]
    #[should_panic(expected = "embedded drop-in is not JSON")]
    fn a_drop_in_that_is_not_json_is_a_bug_in_this_binary() {
        let broken = Agent {
            contents: "not json at all",
            ..*agent("codex")
        };
        let _ = broken.merge("{}");
    }

    #[test]
    #[should_panic(expected = "embedded drop-in is not an object")]
    fn a_drop_in_that_is_not_an_object_is_a_bug_in_this_binary() {
        let broken = Agent {
            contents: "[1, 2, 3]",
            shape: Shape::JsonTopLevel,
            ..*agent("droid")
        };
        let _ = broken.merge("{}");
    }

    #[test]
    fn every_name_is_unique_and_resolvable() {
        let mut names = names();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "a name is repeated");
        for name in names {
            assert_eq!(agent(name).name, name);
        }
        assert!(by_name("cursur").is_none(), "a typo must not resolve");
    }

    #[test]
    fn every_drop_in_is_embedded_and_invokes_us() {
        for agent in AGENTS {
            assert!(
                agent.contents.contains(COMMAND_PREFIX),
                "{} embeds no command of ours",
                agent.name
            );
            match agent.shape {
                Shape::TomlBlock => assert!(parses_as_toml(agent.contents), "{}", agent.name),
                _ => assert!(parses_as_json(agent.contents), "{}", agent.name),
            }
        }
    }

    #[test]
    fn the_delivery_order_puts_the_riskiest_route_last() {
        assert_eq!(agent("claude-code").delivery, Delivery::Plugin);
        for name in ["copilot", "grok", "kiro"] {
            assert_eq!(agent(name).delivery, Delivery::OwnFile, "{name}");
            assert_eq!(agent(name).shape, Shape::Whole, "{name}");
        }
        for name in [
            "codex",
            "cursor",
            "devin",
            "droid",
            "gemini",
            "mistral-vibe",
        ] {
            assert_eq!(agent(name).delivery, Delivery::SharedFile, "{name}");
        }
    }

    #[test]
    fn droid_keeps_its_events_at_the_top_level() {
        // Nesting them under `hooks` gives a config Droid ignores silently.
        assert_eq!(agent("droid").shape, Shape::JsonTopLevel);
        let out = written(agent("droid").merge("{}").unwrap());
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("hooks").is_none(), "{out}");
        assert!(parsed.get("SessionStart").is_some(), "{out}");
    }

    #[test]
    fn an_own_file_is_written_whole_and_then_left_alone() {
        let kiro = agent("kiro");
        assert_eq!(kiro.merge(""), Ok(Plan::Write(kiro.contents.to_owned())));
        assert_eq!(kiro.merge(kiro.contents), Ok(Plan::AlreadyInstalled));
    }

    #[test]
    fn a_merge_into_an_empty_file_writes_our_events() {
        let out = written(agent("codex").merge("").unwrap());
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["hooks"]["SessionStart"].is_array(), "{out}");
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn a_merge_preserves_unrelated_keys_and_their_order() {
        let before = r#"{"zebra": 1, "hooks": {"OtherEvent": ["theirs"]}, "alpha": 2}"#;
        let out = written(agent("codex").merge(before).unwrap());
        let parsed: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(parsed["zebra"], 1);
        assert_eq!(parsed["alpha"], 2);
        assert_eq!(parsed["hooks"]["OtherEvent"][0], "theirs");
        // `preserve_order` is what makes this true rather than aspirational.
        let keys: Vec<&String> = parsed.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["zebra", "hooks", "alpha"]);
    }

    #[test]
    fn an_unrelated_hook_on_an_event_we_own_survives() {
        let before = r#"{"hooks": {"SessionStart": [{"type": "command", "command": "theirs"}]}}"#;
        let out = written(agent("codex").merge(before).unwrap());
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let entries = parsed["hooks"]["SessionStart"].as_array().unwrap();

        assert_eq!(entries[0]["command"], "theirs");
        assert!(entries.len() > 1, "ours was not added: {out}");
    }

    #[test]
    fn a_stale_entry_of_ours_is_replaced_rather_than_duplicated() {
        let before = r#"{"hooks": {"SessionStart": [
            {"type": "command", "command": "tmux-agent-status set something-old"}
        ]}}"#;
        let out = written(agent("codex").merge(before).unwrap());
        let parsed: Value = serde_json::from_str(&out).unwrap();

        assert!(
            !out.contains("something-old"),
            "the stale entry survived: {out}"
        );
        assert_eq!(parsed["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_document_that_already_carries_our_entries_is_not_rewritten() {
        let codex = agent("codex");
        let once = written(codex.merge("").unwrap());
        // The semantic no-op: run the merge again over its own output.
        assert_eq!(codex.merge(&once), Ok(Plan::AlreadyInstalled));
    }

    #[test]
    fn a_hand_maintained_file_is_not_reformatted_for_nothing() {
        // Reserialising an already-correct file changes nothing semantically
        // and everything textually; doing so would take a pointless backup and
        // put a large no-op diff in someone's dotfiles repo.
        let codex = agent("codex");
        let merged = written(codex.merge("").unwrap());
        let compact = serde_json::to_string(&serde_json::from_str::<Value>(&merged).unwrap())
            .expect("it reserialises");
        assert_ne!(compact, merged, "the fixture must differ textually");
        assert_eq!(codex.merge(&compact), Ok(Plan::AlreadyInstalled));
    }

    #[test]
    fn a_file_that_is_not_json_is_refused_rather_than_replaced() {
        for text in [
            "this is not json",
            "[1, 2, 3]",
            r#"{"hooks": "not an object"}"#,
        ] {
            let refused = agent("codex").merge(text).expect_err("must be refused");
            assert!(!refused.to_string().is_empty(), "{text:?}");
            assert!(
                // The arm that says "no" is only reached by a failing run.
                matches!(refused, Refused::Unparseable(_)), // coverage: off
                "{text:?}: {refused}"
            );
        }
    }

    #[test]
    fn devins_drop_in_nests_under_hooks_and_stays_inside_its_events() {
        let devin = agent("devin");
        let out = written(devin.merge("{}").unwrap());
        let parsed: Value = serde_json::from_str(&out).unwrap();

        let hooks = parsed["hooks"].as_object().expect("a hooks object");
        assert!(!hooks.is_empty());
        for key in hooks.keys() {
            assert!(
                DEVIN_EVENTS.contains(&key.as_str()),
                "{key} is not one of Devin's documented events"
            );
        }
    }

    #[test]
    fn a_devin_event_outside_the_documented_eight_is_refused() {
        // One unknown key discards the entire hook map, ours included, so the
        // constraint is checked on the whole result rather than trusted.
        let devin = agent("devin");
        assert_eq!(
            devin.merge(r#"{"hooks": {"StopFailure": []}}"#),
            Err(Refused::DevinUnknownEvent("StopFailure".to_owned()))
        );

        // And a key the user had before we arrived is fatal in the same way,
        // which is worth saying rather than writing hooks that never fire.
        assert_eq!(
            devin.merge(r#"{"hooks": {"TheirTypo": []}}"#),
            Err(Refused::DevinUnknownEvent("TheirTypo".to_owned()))
        );
    }

    #[test]
    fn our_own_devin_drop_in_stays_inside_the_eight() {
        let devin = agent("devin");
        let ours: Value = serde_json::from_str(devin.contents).unwrap();
        for key in ours.as_object().unwrap().keys() {
            assert!(
                DEVIN_EVENTS.contains(&key.as_str()),
                "the shipped drop-in uses {key}, which Devin does not know"
            );
        }
    }

    #[test]
    fn the_vibe_block_is_appended_once_and_then_recognised() {
        let vibe = agent("mistral-vibe");
        let out = written(vibe.merge("[[hooks]]\nname = \"theirs\"\n").unwrap());

        assert!(out.contains("theirs"), "the user's hook was lost: {out}");
        assert!(super::super::has_marked_block(&out));
        assert!(out.ends_with('\n'));
        assert_eq!(vibe.merge(&out), Ok(Plan::AlreadyInstalled));
    }

    #[test]
    fn hooks_copied_by_hand_are_recognised_without_markers() {
        // Verified as a real setup: byte-identical to the shipped file and
        // carrying no markers. Appending would give them a second copy of
        // every hook.
        let vibe = agent("mistral-vibe");
        assert_eq!(vibe.merge(vibe.contents), Ok(Plan::AlreadyInstalled));
        assert!(vibe.present_unmarked(vibe.contents));
        let adopted = vibe.adopt(vibe.contents);
        assert!(super::super::has_marked_block(&adopted));
        // The hooks must sit between the markers, not after an empty marked
        // block at the end of the file.
        let start = adopted.find(super::super::MARKER_START).unwrap();
        let end = adopted.find(super::super::MARKER_END).unwrap();
        assert!(start < end);
        let between = &adopted[start + super::super::MARKER_START.len()..end];
        assert!(between.contains("tmux-agent-status-pre-tool"), "{adopted}");
        assert!(between.contains("tmux-agent-status-post-tool"), "{adopted}");
        assert!(
            between.contains("tmux-agent-status-post-agent"),
            "{adopted}"
        );
        assert!(!vibe.present_unmarked("[[hooks]]\nname = \"theirs\"\n"));
    }

    #[test]
    fn adopt_wraps_unmarked_hooks_in_place() {
        let vibe = agent("mistral-vibe");
        let ours = vibe.contents;
        let before = format!(
            "# user prefix\n[[hooks]]\nname = \"theirs-before\"\n\n{ours}[[hooks]]\nname = \"theirs-after\"\n"
        );
        let adopted = vibe.adopt(&before);
        assert!(super::super::has_marked_block(&adopted));
        assert!(adopted.starts_with("# user prefix\n"), "{adopted}");
        assert!(adopted.contains("name = \"theirs-before\""), "{adopted}");
        assert!(adopted.contains("name = \"theirs-after\""), "{adopted}");
        let start = adopted.find(super::super::MARKER_START).unwrap();
        let end = adopted.find(super::super::MARKER_END).unwrap();
        let between = &adopted[start + super::super::MARKER_START.len()..end];
        assert!(between.contains("tmux-agent-status-pre-tool"), "{adopted}");
        assert!(between.contains("tmux-agent-status-post-tool"), "{adopted}");
        assert!(
            between.contains("tmux-agent-status-post-agent"),
            "{adopted}"
        );
    }

    #[test]
    fn adopt_merges_adjacent_unmarked_blocks_into_one() {
        let vibe = agent("mistral-vibe");
        let ours = vibe.contents;
        let before = format!("{ours}\n# a note\n{ours}");
        let adopted = vibe.adopt(&before);
        assert_eq!(
            adopted.matches(super::super::MARKER_START).count(),
            1,
            "expected one marked block, got:\n{adopted}"
        );
        assert!(adopted.contains("# a note"), "{adopted}");
        assert!(
            adopted.matches("tmux-agent-status-pre-tool").count() == 2,
            "the two copies of our hooks should survive:\n{adopted}"
        );
    }

    #[test]
    fn adopt_keeps_non_our_tables_outside_the_marked_block() {
        let vibe = agent("mistral-vibe");
        let ours = vibe.contents;
        // Single-quoted names are not parsed as ours and must stay outside the
        // marked block, which also exercises the early-exit branch in the name
        // parser.
        let before = format!("{ours}\n[[hooks]]\nname = 'theirs'\n{ours}");
        let adopted = vibe.adopt(&before);
        assert_eq!(
            adopted.matches(super::super::MARKER_START).count(),
            2,
            "expected two marked blocks, got:\n{adopted}"
        );
        assert!(adopted.contains("name = 'theirs'"), "{adopted}");
    }

    #[test]
    fn a_non_toml_adopt_appends_an_empty_marked_block() {
        let codex = agent("codex");
        let out = codex.adopt("{}");
        assert!(super::super::has_marked_block(&out));
    }

    #[test]
    fn a_toml_file_that_does_not_end_at_the_top_level_is_refused() {
        let vibe = agent("mistral-vibe");
        for ending in [
            "description = \"\"\"\nstill open\n",
            "description = '''\nstill open\n",
            "description = \"still open\n",
            "description = 'still open\n",
        ] {
            assert_eq!(
                vibe.merge(ending),
                Err(Refused::TomlNotAtTopLevel),
                "ending {ending:?}"
            );
        }
    }

    #[test]
    fn a_toml_file_that_does_end_at_the_top_level_is_appended_to() {
        for ending in [
            "",
            "# a comment with an odd \" quote in it\n",
            "description = \"\"\"\nclosed again\n\"\"\"\n",
            "description = '''\nclosed again\n'''\n",
            "description = \"closed\"\n",
            "description = 'it''s closed'\n",
            "description = \"an escaped \\\" quote\"\n",
            "[table]\nkey = 1",
        ] {
            assert!(ends_at_top_level(ending), "ending {ending:?}");
        }
    }

    #[test]
    fn a_truncated_document_does_not_parse_in_its_own_language() {
        assert!(!parses_as_json("{\"hooks\": "));
        assert!(parses_as_json("{}"));
        assert!(!parses_as_toml(""));
        assert!(!parses_as_toml("name = \"unterminated\n"));
        assert!(parses_as_toml("name = \"fine\"\n"));
    }

    #[test]
    fn an_entry_of_ours_is_recognised_in_any_nesting() {
        assert!(is_ours(
            &serde_json::json!({"command": "tmux-agent-status set done"})
        ));
        assert!(is_ours(
            &serde_json::json!({"hooks": [{"command": "tmux-agent-status reset"}]})
        ));
        assert!(is_ours(&serde_json::json!(["tmux-agent-status finish"])));
        assert!(!is_ours(&serde_json::json!({"command": "something-else"})));
        assert!(!is_ours(
            &serde_json::json!({"command": "my-tmux-agent-status-wrapper"})
        ));
        assert!(!is_ours(&serde_json::json!(42)));
    }

    #[test]
    fn detection_reports_both_signals() {
        let home = Home {
            home: PathBuf::from("/nonexistent-home"),
            xdg_config: None,
        };
        let found = agent("codex").detect(&home);
        assert!(!found.directory);
        assert_eq!(found.preselected(), found.command);

        for (directory, command) in [(true, true), (true, false), (false, true), (false, false)] {
            let detected = Detected { directory, command };
            assert_eq!(detected.preselected(), directory || command);
            assert!(!detected.why().is_empty());
        }
    }

    #[test]
    fn targets_follow_home_and_the_config_directory() {
        let home = Home {
            home: PathBuf::from("/home/u"),
            xdg_config: None,
        };
        assert_eq!(
            agent("codex").target(&home),
            PathBuf::from("/home/u/.codex/hooks.json")
        );
        assert_eq!(
            agent("devin").target(&home),
            PathBuf::from("/home/u/.config/devin/config.json")
        );
        assert_eq!(
            agent("devin").directory(&home),
            PathBuf::from("/home/u/.config/devin")
        );
        assert_eq!(devin_project_file(), Path::new(".devin/hooks.v1.json"));
    }

    #[test]
    fn the_marketplace_comes_from_the_manifest() {
        assert_eq!(default_marketplace(), "gerbenoostra/tmux-agent-status");
        assert_eq!(
            marketplace_name("gerbenoostra/tmux-agent-status"),
            "tmux-agent-status"
        );
        assert_eq!(
            marketplace_name("/home/u/checkouts/tmux-agent-status/"),
            "tmux-agent-status"
        );
        assert_eq!(marketplace_name("tmux-agent-status"), "tmux-agent-status");
    }

    #[test]
    fn the_plugin_commands_are_the_two_the_docs_give() {
        let [add, install] = Claude::at("claude").plugin_commands("gerbenoostra/tmux-agent-status");
        assert_eq!(
            add,
            [
                "claude",
                "plugin",
                "marketplace",
                "add",
                "gerbenoostra/tmux-agent-status"
            ]
        );
        assert_eq!(
            install,
            [
                "claude",
                "plugin",
                "install",
                "tmux-agent-status@tmux-agent-status",
                "-y",
                "--json"
            ]
        );
    }

    #[test]
    fn an_installed_plugin_is_recognised_by_its_id_prefix() {
        assert!(mentions_plugin(&serde_json::json!([
            {"id": "tmux-agent-status@tmux-agent-status"}
        ])));
        // A different marketplace is still our plugin; where it points is not
        // ours to check, and re-adding it would swap a contributor's working
        // copy for a released one.
        assert!(mentions_plugin(&serde_json::json!([
            {"id": "tmux-agent-status@some-local-checkout"}
        ])));
        assert!(!mentions_plugin(
            &serde_json::json!([{"id": "something-else@x"}])
        ));
        assert!(!mentions_plugin(&serde_json::json!({})));
        assert!(!mentions_plugin(&serde_json::json!(7)));
    }

    #[test]
    fn each_shape_knows_its_own_language() {
        assert!(agent("mistral-vibe").parses()("name = \"x\"\n"));
        assert!(!agent("mistral-vibe").parses()("name = \"x\n"));
        assert!(agent("codex").parses()("{}"));
        assert!(!agent("codex").parses()("{"));
        assert!(agent("kiro").parses()("{}"));
    }

    #[test]
    fn the_default_route_is_the_one_that_leaves_settings_alone() {
        assert_eq!(ClaudeRoute::default(), ClaudeRoute::Auto);
    }

    #[test]
    fn every_refusal_says_what_the_user_should_know() {
        for refused in [
            Refused::Unparseable("trailing comma".to_owned()),
            Refused::TomlNotAtTopLevel,
            Refused::DevinUnknownEvent("MadeUp".to_owned()),
        ] {
            assert!(refused.to_string().len() > 20, "{refused:?}");
        }
    }

    #[test]
    fn a_command_is_looked_for_on_the_path_we_inherited() {
        // `sh` is on every machine this runs on; the negative is a name
        // nothing could plausibly install.
        assert!(on_path("sh"));
        assert!(!on_path("tmux-agent-status-no-such-command"));
    }

    #[test]
    fn the_toml_names_read_are_only_ours() {
        let names = toml_names(agent("mistral-vibe").contents);
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(
            names
                .iter()
                .all(|name| name.starts_with("tmux-agent-status"))
        );
        assert!(toml_names("name = \"theirs\"\n").is_empty());
        assert!(toml_names("notname = \"tmux-agent-status-x\"\n").is_empty());
        assert!(toml_names("name: \"tmux-agent-status-x\"\n").is_empty());
        assert!(toml_names("name = tmux-agent-status-x\n").is_empty());
    }

    fn busy() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::ExecutableFileBusy)
    }

    #[test]
    fn a_busy_attempt_is_retried_until_it_succeeds() {
        let mut calls = 0;
        let result = retry_while_busy(|| {
            calls += 1;
            if calls < 3 { Err(busy()) } else { Ok(calls) }
        });
        assert_eq!(result.expect("the third attempt succeeds"), 3);
    }

    #[test]
    fn an_error_that_is_not_busy_is_not_retried() {
        let mut calls = 0;
        let result = retry_while_busy(|| {
            calls += 1;
            Err::<(), _>(std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        assert_eq!(calls, 1);
        assert_eq!(
            result.expect_err("not-found is reported as-is").kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn a_busy_attempt_that_never_clears_still_gives_up() {
        let mut calls = 0;
        let result = retry_while_busy(|| {
            calls += 1;
            Err::<(), _>(busy())
        });
        assert!(calls > 1, "never retried at all");
        assert_eq!(
            result.expect_err("busy forever is still an error").kind(),
            std::io::ErrorKind::ExecutableFileBusy
        );
    }
}
