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

mod support;

/// The tiers a matrix row's last cell may name.
const TIERS: &[&str] = &["scalar", "tracked aggregate"];

fn readme() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/agents/README.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// One host row of the Background work matrix.
struct Row {
    /// The registered agent name, recovered from the `[..](<name>.md)` link.
    name: String,
    shipped_tier: String,
}

/// The host rows of the Background work matrix, every one checked to link a
/// registered agent and name a known shipped tier.
fn matrix_rows() -> Vec<Row> {
    let rows: Vec<Row> = support::markdown::table_after(&readme(), "## Background work")
        .into_iter()
        .filter(|cells| cells[0].starts_with('['))
        .map(|cells| {
            let name = cells[0]
                .rsplit_once("](")
                .and_then(|(_, link)| link.strip_suffix(".md)"))
                .unwrap_or_else(|| panic!("matrix row `{}` does not link a host page", cells[0]))
                .to_owned();
            assert!(
                agents::by_name(&name).is_some(),
                "matrix row `{name}` does not name a registered agent"
            );
            let shipped_tier = cells.last().expect("a row has cells").clone();
            assert!(
                TIERS.contains(&shipped_tier.as_str()),
                "matrix row `{name}` ships unknown tier `{shipped_tier}`; expected one of {TIERS:?}"
            );
            Row { name, shipped_tier }
        })
        .collect();
    assert!(
        !rows.is_empty(),
        "the Background work matrix has no host rows"
    );
    rows
}

#[test]
fn every_registered_agent_has_a_matrix_row() {
    let rows = matrix_rows();
    for name in agents::names() {
        assert!(
            rows.iter().any(|row| row.name == name),
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
    for Row { name, shipped_tier } in matrix_rows() {
        if shipped_tier != "scalar" {
            continue;
        }
        let commands = shipped_commands(&name);
        // An extraction that finds nothing would pass every check below.
        assert!(
            !commands.is_empty(),
            "found no `tmux-agent-status` command in `{name}`'s drop-in"
        );
        for command in commands {
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
