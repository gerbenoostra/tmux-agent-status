//! The command-line surface: help, version, and usage errors.
//!
//! These tests run the binary as a separate process, the same way a human or a
//! misconfigured hook would invoke it.

use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-agent-status");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn help_flag_prints_usage() {
    for flag in ["--help", "-h"] {
        let out = run(&[flag]);
        assert!(out.status.success(), "{flag} should exit 0");
        assert!(out.stderr.is_empty(), "help must not write to stderr");
        let text = stdout(&out);
        assert!(
            text.contains("tmux-agent-status"),
            "{flag} missing program name"
        );
        assert!(text.contains("usage:"), "{flag} missing usage");
        assert!(text.contains("set <state>"), "{flag} missing set command");
        assert!(
            text.contains("clear-window"),
            "{flag} missing clear-window command"
        );
        assert!(text.contains("states:"), "{flag} missing states list");
    }
}

#[test]
fn version_flag_prints_name_version_and_path() {
    for flag in ["--version", "-V"] {
        let out = run(&[flag]);
        assert!(out.status.success(), "{flag} should exit 0");
        assert!(out.stderr.is_empty(), "version must not write to stderr");
        let text = stdout(&out);
        assert!(
            text.contains("tmux-agent-status"),
            "{flag} missing package name"
        );
        assert!(
            text.contains(env!("CARGO_PKG_VERSION")),
            "{flag} missing version"
        );
        assert!(
            text.contains("running from /"),
            "{flag} should print the absolute executable path"
        );
    }
}

#[test]
fn no_command_is_a_usage_error() {
    let out = run(&[]);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("no command given"));
    assert!(err.contains("usage:"));
}

#[test]
fn unexpected_arguments_are_a_usage_error() {
    let out = run(&["surprise", "extra"]);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("unexpected arguments: surprise extra"));
    assert!(err.contains("usage:"));
}

#[test]
fn set_without_a_state_is_a_usage_error() {
    let out = run(&["set"]);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("set requires a state"));
    assert!(err.contains("usage:"));
}
