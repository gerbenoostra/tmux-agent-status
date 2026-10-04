//! The bound on test-side tmux calls names the call that blocked.

use std::process::Command;
use std::time::{Duration, Instant};

mod support;

use support::tmux::{Failure, try_output_within};

#[test]
fn a_command_that_outlives_its_limit_is_killed_and_named() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let started = Instant::now();

    let err = try_output_within(command, Duration::from_millis(200)).expect_err("sleep outlives");

    assert!(started.elapsed() < Duration::from_secs(10), "not killed");
    // A stall is `NotFinished`, not `NotStarted`: a probe may skip on an
    // absent program but never on a wedged one.
    assert!(matches!(err, Failure::NotFinished(_)), "{err:?}");
    let err = err.to_string();
    assert!(err.contains("`sleep 30`"), "{err}");
    assert!(err.contains("200ms"), "{err}");
}

#[test]
fn a_command_within_its_limit_returns_its_output() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo out; echo err >&2; exit 3"]);

    let out = try_output_within(command, Duration::from_secs(30)).expect("sh finishes");

    assert_eq!(out.status.code(), Some(3));
    assert_eq!(out.stdout, b"out\n");
    assert_eq!(out.stderr, b"err\n");
}

#[test]
fn a_child_that_exits_but_leaves_its_output_open_is_reported() {
    let mut command = Command::new("sh");
    // sh exits at once; the backgrounded sleep keeps both output pipes open,
    // the way a tmux server starting up can hold the client's pipes.
    command.args(["-c", "sleep 30 & exit 0"]);
    let started = Instant::now();

    let err =
        try_output_within(command, Duration::from_millis(200)).expect_err("output stays open");

    assert!(started.elapsed() < Duration::from_secs(10), "not abandoned");
    assert!(matches!(err, Failure::NotFinished(_)), "{err:?}");
    let err = err.to_string();
    assert!(err.contains("sh -c"), "{err}");
    assert!(err.contains("still open"), "{err}");
}

#[test]
fn a_command_that_cannot_start_is_named() {
    let command = Command::new("tmux-agent-status-no-such-program");

    let err = try_output_within(command, Duration::from_secs(1)).expect_err("no such program");

    assert!(matches!(err, Failure::NotStarted(_)), "{err:?}");
    let err = err.to_string();
    assert!(err.contains("tmux-agent-status-no-such-program"), "{err}");
}
