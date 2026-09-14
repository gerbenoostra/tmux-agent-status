//! Shared helpers for integration tests.
//!
//! Each integration test binary imports only the pieces it needs; suppress
//! dead-code warnings because the whole module is compiled for every test.

#![allow(dead_code)]

pub mod command;
pub mod markdown;
pub mod tempdir;

pub const BIN: &str = env!("CARGO_BIN_EXE_tmux-agent-status");

/// Whether there is a tmux to test against.
///
/// tmux is installed everywhere this suite runs and the CI job installs it
/// before running the tests; a machine without one skips rather than fails,
/// which is the same promise the tool itself makes.
pub fn tmux_or_skip() -> bool {
    let found = std::process::Command::new("tmux")
        .arg("-V")
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| out.status.success());
    if !found {
        eprintln!("no tmux on PATH: skipping");
    }
    found
}
