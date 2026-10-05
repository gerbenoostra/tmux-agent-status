//! The Claude Code lifecycle probe fixtures.
//!
//! `tests/fixtures/claude-code/lifecycle/<scenario>/` holds one
//! `NNN-<event>.json` record per hook event of a probed run, plus the
//! scenario's `expected.tsv` replay rows.

use std::fs;
use std::path::{Path, PathBuf};

/// One probed scenario directory.
pub struct Scenario {
    pub dir: PathBuf,
    /// The scenario's `*.json` records, in event order.
    pub records: Vec<PathBuf>,
}

impl Scenario {
    pub fn name(&self) -> String {
        self.dir
            .file_name()
            .expect("a scenario directory has a name")
            .to_string_lossy()
            .into_owned()
    }

    pub fn expected_path(&self) -> PathBuf {
        self.dir.join("expected.tsv")
    }
}

/// Every scenario, sorted by name; fails when there is none.
pub fn scenarios() -> Vec<Scenario> {
    scenarios_for("claude-code")
}

pub fn scenarios_for(host: &str) -> Vec<Scenario> {
    let fixtures = format!("tests/fixtures/{host}/lifecycle");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixtures);
    let mut scenarios: Vec<Scenario> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|dir| dir.is_dir())
        .map(|dir| Scenario {
            records: json_files(&dir),
            dir,
        })
        .collect();
    scenarios.sort_by(|a, b| a.dir.cmp(&b.dir));
    assert!(
        !scenarios.is_empty(),
        "no lifecycle scenarios under {fixtures}"
    );
    scenarios
}

pub fn all_scenarios() -> Vec<Scenario> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut scenarios = Vec::new();
    for entry in
        fs::read_dir(&root).unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
    {
        let lifecycle = entry
            .expect("a readable fixture host")
            .path()
            .join("lifecycle");
        if !lifecycle.is_dir() {
            continue;
        }
        let host = lifecycle
            .parent()
            .and_then(Path::file_name)
            .expect("a lifecycle directory has a host")
            .to_string_lossy();
        scenarios.extend(scenarios_for(&host));
    }
    scenarios.sort_by(|a, b| a.dir.cmp(&b.dir));
    assert!(!scenarios.is_empty(), "no lifecycle scenarios");
    scenarios
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|file| file.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    files
}

/// One fixture record, parsed.
pub fn read_record(path: &Path) -> serde_json::Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}
