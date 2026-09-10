//! The command-line surface: help, version, and usage errors.
//!
//! These tests run the binary as a separate process, the same way a human or a
//! misconfigured hook would invoke it.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-agent-status");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("TMUX_AGENT_STATUS_DISABLED")
        .env_remove("TMUX_AGENT_STATUS_DEBUG")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

fn run_env(args: &[&str], key: &str, value: &str) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env(key, value)
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
        assert!(text.contains("reset"), "{flag} missing reset command");
        assert!(text.contains("finish"), "{flag} missing finish command");
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

#[test]
fn boundary_commands_reject_arguments() {
    for command in ["reset", "finish"] {
        let out = run(&[command, "extra"]);
        assert_eq!(out.status.code(), Some(2), "{command}");
        let err = stderr(&out);
        assert!(
            err.contains(&format!("unexpected arguments: {command} extra")),
            "{command}: {err}"
        );
    }
}

#[test]
fn pane_flag_requires_a_value() {
    let out = run(&["reset", "--pane"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--pane requires a value"));
}

#[test]
fn pane_flag_is_allowed_on_hook_commands() {
    // No tmux in the test environment, so the pane override is accepted and
    // the missing tmux is silently ignored.
    for args in [
        ["set", "done", "--pane", "%0"].as_slice(),
        ["reset", "--pane", "%0"].as_slice(),
        ["finish", "--pane", "%0"].as_slice(),
        ["clear-window", "--pane", "%0"].as_slice(),
        ["clear-window", "%0", "--pane", "%1"].as_slice(),
    ] {
        let out = run(args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn disabled_turns_hook_commands_into_no_ops() {
    for args in [
        ["set", "done"].as_slice(),
        ["reset"].as_slice(),
        ["finish"].as_slice(),
        ["clear-window"].as_slice(),
    ] {
        let out = run_env(args, "TMUX_AGENT_STATUS_DISABLED", "1");
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn disabled_does_not_hide_usage_errors() {
    // `set` with no state is still a usage error: the opt-out is about writes,
    // not about diagnosing a broken hook config.
    let out = run_env(&["set"], "TMUX_AGENT_STATUS_DISABLED", "1");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("set requires a state"));
}

#[test]
fn help_mentions_disabled_and_debug() {
    let out = run(&["--help"]);
    let text = stdout(&out);
    assert!(text.contains("TMUX_AGENT_STATUS_DISABLED"));
    assert!(text.contains("TMUX_AGENT_STATUS_DEBUG"));
    assert!(text.contains("notify"));
}

#[test]
fn notify_requires_agent() {
    let out = run(&["notify", "{}"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("notify requires --agent"));
}

#[test]
fn notify_requires_payload_or_stdin() {
    let out = run(&["notify", "--agent", "mistral-vibe"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("notify requires a payload or --stdin"));
}

#[test]
fn notify_agent_requires_a_value() {
    let out = run(&["notify", "--agent"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--agent requires a value"));
}

#[test]
fn notify_with_unknown_agent_is_silent_no_op() {
    let out = run(&["notify", "--agent", "no-such-agent", "{}"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_with_unparseable_payload_is_silent_no_op() {
    let out = run(&["notify", "--agent", "mistral-vibe", "not-json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_stdin_reads_payload() {
    let payload = r#"{"hook_event_name":"pre_tool"}"#;
    let mut child = Command::new(BIN)
        .args(["notify", "--agent", "mistral-vibe", "--stdin"])
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary spawns");
    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        stdin.write_all(payload.as_bytes()).expect("write payload");
    }
    let out = child.wait_with_output().expect("the binary runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_payload_is_mapped_and_run_as_hook_command() {
    // `post_agent` maps to `done`. No tmux is available in the test process, so
    // the pane resolution short-circuits and the command exits 0.
    let out = run(&[
        "notify",
        "--agent",
        "mistral-vibe",
        r#"{"hook_event_name":"post_agent"}"#,
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_rejects_extra_arguments() {
    let out = run(&["notify", "--agent", "mistral-vibe", "one", "two"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unexpected arguments: notify one two"));
}

#[test]
fn notify_disabled_is_a_no_op() {
    let out = Command::new(BIN)
        .args(["notify", "--agent", "mistral-vibe", "{}"])
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("TMUX_AGENT_STATUS_DISABLED", "1")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_debug_logs_dropped_payloads() {
    let out = Command::new(BIN)
        .args([
            "notify",
            "--agent",
            "mistral-vibe",
            r#"{"hook_event_name":"unknown"}"#,
        ])
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("TMUX_AGENT_STATUS_DEBUG", "1")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    let err = stderr(&out);
    assert!(err.contains("dropped payload"));
    assert!(err.contains("mistral-vibe"));
}

#[cfg(unix)]
#[test]
fn notify_stdin_with_terminal_is_a_no_op() {
    // Allocate a pseudo-terminal and hand the slave fd to the child as stdin.
    // The binary must detect the terminal and return without reading.
    use std::fs::File;
    use std::os::fd::FromRawFd;

    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut::<libc::termios>(),
            std::ptr::null_mut::<libc::winsize>(),
        )
    };
    assert_eq!(rc, 0, "openpty failed");

    let slave_file = unsafe { File::from_raw_fd(slave) };
    let child = Command::new(BIN)
        .args(["notify", "--agent", "mistral-vibe", "--stdin"])
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(slave_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary spawns");

    unsafe {
        let _ = libc::close(master);
    }

    let out = child.wait_with_output().expect("the binary runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
