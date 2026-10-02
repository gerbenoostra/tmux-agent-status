//! Consistency between the Background work matrix and the shipped configs.
//!
//! `docs/agents/README.md` carries a host-by-eligibility matrix under
//! "## Background work". These tests pin its invariants: every registered
//! agent has a row, and a row marked scalar must not claim lifecycle commands
//! its drop-in cannot honour - a scalar host never feeds `--stdin` to `reset`
//! or `finish` because it has nothing to identify the session with. The
//! `notify --agent <name> --stdin` shape-B route is unaffected: `notify` is
//! the single-callback form, not the lifecycle commands.

use std::fs;
use std::path::PathBuf;

use tmux_agent_status::register::agents;

fn readme() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/agents/README.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The lines of the Background work matrix's host rows.
fn matrix_rows(readme: &str) -> Vec<&str> {
    let section = readme
        .split("## Background work")
        .nth(1)
        .expect("docs/agents/README.md has no '## Background work' section");
    section
        .lines()
        .take_while(|line| !line.starts_with("## "))
        .filter(|line| line.starts_with("| ["))
        .collect()
}

#[test]
fn every_registered_agent_has_a_matrix_row() {
    let readme = readme();
    let rows = matrix_rows(&readme);
    assert!(
        !rows.is_empty(),
        "the Background work matrix has no host rows"
    );
    for name in agents::names() {
        let link = format!("]({name}.md)");
        assert!(
            rows.iter().any(|row| row.contains(&link)),
            "registered agent `{name}` has no row in the Background work matrix"
        );
    }
}

/// The commands a scalar drop-in runs, collected from its embedded template
/// and any `share/agents/<name>/` file, which are what `register` writes.
fn shipped_commands(name: &str) -> Vec<String> {
    let mut texts = vec![
        agents::by_name(name)
            .unwrap_or_else(|| panic!("{name} is not a registered agent"))
            .contents
            .to_owned(),
    ];
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("share/agents")
        .join(name);
    if dir.is_dir() {
        for entry in fs::read_dir(&dir).unwrap() {
            let file = entry.unwrap().path();
            if file.is_file() {
                // The embedded template is read back verbatim for JSON and
                // TOML alike; only raw `tmux-agent-status` strings matter.
                texts.push(
                    fs::read_to_string(&file)
                        .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display())),
                );
            }
        }
    }
    let mut commands = Vec::new();
    for text in &texts {
        for line in text.lines() {
            for part in line.split('"') {
                if part.trim_start().starts_with("tmux-agent-status ") {
                    commands.push(part.trim().to_owned());
                }
            }
        }
    }
    commands
}

#[test]
fn a_scalar_host_never_feeds_stdin_to_lifecycle_commands() {
    let readme = readme();
    for row in matrix_rows(&readme) {
        let cells: Vec<&str> = row
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        let shipped_tier = cells.last().copied().unwrap_or("");
        if shipped_tier != "scalar" {
            continue;
        }
        // The row links to docs/agents/<name>.md; recover the registered name.
        let name = cells[1]
            .rsplit('(')
            .next()
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.strip_suffix(".md"))
            .unwrap_or("");
        assert!(
            agents::by_name(name).is_some(),
            "matrix row `{name}` does not name a registered agent"
        );
        for command in shipped_commands(name) {
            let mut args = command
                .trim_start_matches("tmux-agent-status")
                .split_whitespace();
            let subcommand = args.next().unwrap_or("");
            let takes_stdin = args.any(|a| a == "--stdin");
            if matches!(subcommand, "reset" | "finish") {
                assert!(
                    !takes_stdin,
                    "scalar host `{name}` ships `{command}` - reset/finish cannot take --stdin"
                );
            }
            // `notify --agent <name> --stdin` is the shape-B route and stays legal.
        }
    }
}
