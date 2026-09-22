//! The agent step's two halves that need something outside the process: the
//! Claude Code plugin route, and detection against a real directory tree.
//!
//! The plugin route is driven against a stub `claude` rather than the real one,
//! because the three cases that must behave differently - it answers, it
//! refuses, it is not there - are exactly the three a real CLI will not produce
//! on demand. The one that matters is the middle one: a CLI that fails must
//! fail the step and never quietly fall back to editing
//! `~/.claude/settings.json`, which the plugin route promises not to touch.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tmux_agent_status::register::Home;
use tmux_agent_status::register::agents::{self, Claude, Delivery};

mod support;

use support::tempdir::TempDir;

/// A `claude` that prints `stdout` and exits with `code`.
fn stub(dir: &TempDir, name: &str, code: i32, stdout: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(
        &path,
        format!("#!/bin/sh\ncat <<'JSON'\n{stdout}\nJSON\nexit {code}\n"),
    )
    .expect("the stub is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[test]
fn an_installed_plugin_is_seen_and_stops_the_step_there() {
    let dir = TempDir::new("claude-installed");
    let claude = Claude::at(stub(
        &dir,
        "claude",
        0,
        r#"[{"id": "tmux-agent-status@tmux-agent-status"}]"#,
    ));

    assert_eq!(claude.plugin_installed(), Some(true));
}

#[test]
fn a_marketplace_pointing_somewhere_else_is_still_our_plugin() {
    // Verified as a real setup: a contributor registers the marketplace as a
    // `directory` source pointing at their own checkout. Re-adding it from
    // GitHub would swap their working copy for a released one, so idempotency
    // stops at "installed" and never checks where it came from.
    let dir = TempDir::new("claude-local");
    let claude = Claude::at(stub(
        &dir,
        "claude",
        0,
        r#"[{"id": "tmux-agent-status@my-local-checkout", "source": {"source": "directory"}}]"#,
    ));

    assert_eq!(claude.plugin_installed(), Some(true));
}

#[test]
fn a_clean_machine_has_no_plugin_installed() {
    let dir = TempDir::new("claude-empty");
    let claude = Claude::at(stub(&dir, "claude", 0, "[]"));

    assert_eq!(claude.plugin_installed(), Some(false));
}

#[test]
fn a_cli_that_cannot_answer_says_so_rather_than_guessing() {
    let dir = TempDir::new("claude-fails");
    assert_eq!(
        Claude::at(stub(&dir, "claude", 1, "")).plugin_installed(),
        None
    );
    // Output that is not JSON is not an answer either.
    assert_eq!(
        Claude::at(stub(&dir, "claude-noise", 0, "not json at all")).plugin_installed(),
        None
    );
    // And neither is a `claude` that is not there at all.
    assert_eq!(
        Claude::at(dir.join("no-such-claude")).plugin_installed(),
        None
    );
}

// 21. The Claude route does not fall back on failure.
#[test]
fn a_plugin_install_that_fails_fails_the_step_and_names_the_command() {
    let dir = TempDir::new("claude-install-fails");
    let claude = Claude::at(stub(&dir, "claude", 3, "marketplace unreachable"));

    let error = claude
        .install_plugin("gerbenoostra/tmux-agent-status")
        .expect_err("a failing CLI must fail the step");

    assert!(
        error.contains("plugin marketplace add"),
        "the command is not named: {error}"
    );
    assert!(error.contains('3'), "the exit status is not named: {error}");
}

#[test]
fn a_plugin_install_that_works_runs_both_commands_in_order() {
    let dir = TempDir::new("claude-install-works");
    let log = dir.join("log");
    let path = dir.join("claude");
    fs::write(
        &path,
        format!("#!/bin/sh\necho \"$@\" >> {}\nexit 0\n", log.display()),
    )
    .expect("the stub is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");

    Claude::at(&path)
        .install_plugin("gerbenoostra/tmux-agent-status")
        .expect("the stub succeeds");

    let ran = fs::read_to_string(&log).expect("the log was written");
    let lines: Vec<&str> = ran.lines().collect();
    assert_eq!(
        lines,
        [
            "plugin marketplace add gerbenoostra/tmux-agent-status",
            "plugin install tmux-agent-status@tmux-agent-status -y --json",
        ]
    );
}

#[test]
fn a_binary_that_cannot_be_run_is_reported_rather_than_panicking() {
    let dir = TempDir::new("claude-missing");
    let error = Claude::at(dir.join("no-such-claude"))
        .install_plugin("gerbenoostra/tmux-agent-status")
        .expect_err("a missing binary fails the step");
    assert!(error.contains("plugin marketplace add"), "{error}");
}

#[test]
fn the_plugin_route_is_only_offered_when_claude_is_on_the_path() {
    // Whatever this machine has, the two answers must agree with each other.
    let found = Claude::on_path();
    assert_eq!(
        found.is_some(),
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|dir| dir.join("claude").is_file())
    );
}

#[test]
fn detection_finds_an_agent_by_its_config_directory() {
    let dir = TempDir::new("detect");
    let home = Home {
        home: dir.path().to_path_buf(),
        xdg_config: None,
    };
    fs::create_dir_all(dir.join(".codex")).expect("the directory");
    fs::create_dir_all(dir.join(".config/devin")).expect("the directory");

    let codex = agents::by_name("codex").expect("a row");
    assert!(codex.detect(&home).directory);
    assert!(codex.detect(&home).preselected());

    // Devin's directory follows the config directory, not `$HOME` directly.
    let devin = agents::by_name("devin").expect("a row");
    assert!(devin.detect(&home).directory);

    // An agent with neither signal is listed, never preselected, and never
    // registered without appearing in the list.
    let kiro = agents::by_name("kiro").expect("a row");
    let found = kiro.detect(&home);
    assert!(!found.directory);
    assert_eq!(found.preselected(), found.command);
}

#[test]
fn every_target_lands_under_the_home_it_was_given() {
    let dir = TempDir::new("targets");
    let home = Home {
        home: dir.path().to_path_buf(),
        xdg_config: None,
    };
    for agent in agents::AGENTS {
        let target = agent.target(&home);
        assert!(
            target.starts_with(dir.path()),
            "{} writes outside the home it was given: {}",
            agent.name,
            target.display()
        );
        assert!(
            agent.directory(&home).starts_with(dir.path()),
            "{} detects outside the home it was given",
            agent.name
        );
    }
}

#[test]
fn an_own_file_target_is_never_a_file_the_user_maintains() {
    // The point of the class: these three are ours alone, so there is nothing
    // to merge and nothing of the user's to lose.
    for agent in agents::AGENTS {
        if agent.delivery != Delivery::OwnFile {
            continue;
        }
        let home = Home {
            home: PathBuf::from("/home/u"),
            xdg_config: None,
        };
        let target = agent.target(&home);
        assert_eq!(
            target.file_name().map(|n| n.to_string_lossy().into_owned()),
            Some("tmux-agent-status.json".to_owned()),
            "{} writes a whole file over a name that is not ours",
            agent.name
        );
    }
}
