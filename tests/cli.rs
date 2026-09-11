//! The command-line surface: help, version, and usage errors.
//!
//! These tests run the binary as a separate process, the same way a human or a
//! misconfigured hook would invoke it.

use std::io::Write;
use std::process::{Command, Output, Stdio};

mod support;

/// The binary outside tmux, with every `TMUX_AGENT_STATUS_*` variable cleared.
///
/// Every per-agent page recommends exporting `TMUX_AGENT_STATUS_PANE`, and the
/// other two switches change what the binary does at all: inherited state must
/// never decide what a test proves.
fn command(args: &[&str]) -> Command {
    let mut cmd = Command::new(support::BIN);
    cmd.args(args)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("TMUX_AGENT_STATUS_PANE")
        .env_remove("TMUX_AGENT_STATUS_DISABLED")
        .env_remove("TMUX_AGENT_STATUS_DEBUG")
        .stdin(Stdio::null());
    cmd
}

fn run(args: &[&str]) -> Output {
    command(args).output().expect("the binary runs")
}

fn run_env(args: &[&str], key: &str, value: &str) -> Output {
    command(args)
        .env(key, value)
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
    for args in [["surprise"].as_slice(), ["surprise", "extra"].as_slice()] {
        let out = run(args);
        assert!(!out.status.success());
        assert_eq!(out.status.code(), Some(2));
        let err = stderr(&out);
        assert!(err.contains(&format!("unexpected arguments: {}", args.join(" "))));
        assert!(err.contains("usage:"));
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_are_usage_errors() {
    use std::os::unix::ffi::OsStringExt;

    for prefix in [
        [].as_slice(),
        ["surprise"].as_slice(),
        ["set", "done"].as_slice(),
        ["reset"].as_slice(),
        ["clear-window"].as_slice(),
        ["notify", "--agent", "mistral-vibe"].as_slice(),
    ] {
        let out = command(prefix)
            .arg(std::ffi::OsString::from_vec(vec![0xff]))
            .output()
            .expect("the binary runs");
        assert_eq!(out.status.code(), Some(2));
        assert!(stderr(&out).contains("UTF-8"));
    }
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
fn set_rejects_extra_arguments() {
    let out = run(&["set", "done", "extra"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unexpected arguments: set done extra"));
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
    for args in [
        ["set", "done", "--pane"].as_slice(),
        ["reset", "--pane"].as_slice(),
        ["finish", "--pane"].as_slice(),
        ["clear-window", "--pane"].as_slice(),
        ["notify", "--agent", "mistral-vibe", "{}", "--pane"].as_slice(),
    ] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(2));
        assert!(stderr(&out).contains("--pane requires a value"));
    }
}

#[test]
fn clear_window_rejects_extra_arguments() {
    let out = run(&["clear-window", "%0", "%1"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unexpected arguments: clear-window %0 %1"));
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
fn json_flag_prints_empty_object_on_hook_commands() {
    for args in [
        ["set", "done", "--json"].as_slice(),
        ["reset", "--json"].as_slice(),
        ["finish", "--json"].as_slice(),
        ["clear-window", "--json"].as_slice(),
    ] {
        let out = run(args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(out.stderr.is_empty(), "{args:?}");
        assert_eq!(stdout(&out), "{}\n", "{args:?}");
    }
}

#[test]
fn json_flag_prints_empty_object_when_disabled() {
    for args in [
        ["set", "done", "--json"].as_slice(),
        ["reset", "--json"].as_slice(),
        ["finish", "--json"].as_slice(),
        ["clear-window", "--json"].as_slice(),
    ] {
        let out = run_env(args, "TMUX_AGENT_STATUS_DISABLED", "1");
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(out.stderr.is_empty(), "{args:?}");
        assert_eq!(stdout(&out), "{}\n", "{args:?}");
    }
}

#[test]
fn json_flag_does_not_hide_usage_errors() {
    let out = run(&["set", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("set requires a state"));
    assert!(out.stdout.is_empty());
}

#[test]
fn json_flag_works_with_pane_flag() {
    let out = run(&["set", "done", "--pane", "%0", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stderr.is_empty());
    assert_eq!(stdout(&out), "{}\n");
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
fn notify_json_flag_prints_empty_object() {
    let out = run(&[
        "notify",
        "--agent",
        "mistral-vibe",
        r#"{"hook_event_name":"post_agent"}"#,
        "--json",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "{}\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_json_flag_prints_empty_object_for_unknown_agent() {
    let out = run(&["notify", "--agent", "no-such-agent", "{}", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "{}\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_json_flag_prints_empty_object_when_disabled() {
    let out = run_env(
        &[
            "notify",
            "--agent",
            "mistral-vibe",
            r#"{"hook_event_name":"post_agent"}"#,
            "--json",
        ],
        "TMUX_AGENT_STATUS_DISABLED",
        "1",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "{}\n");
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
    let mut child = command(&["notify", "--agent", "mistral-vibe", "--stdin"])
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
    let out = run_env(
        &["notify", "--agent", "mistral-vibe", "{}"],
        "TMUX_AGENT_STATUS_DISABLED",
        "1",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn notify_debug_logs_dropped_payloads() {
    let out = run_env(
        &[
            "notify",
            "--agent",
            "mistral-vibe",
            r#"{"hook_event_name":"unknown"}"#,
        ],
        "TMUX_AGENT_STATUS_DEBUG",
        "1",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    let err = stderr(&out);
    assert!(err.contains("dropped payload"));
    assert!(err.contains("mistral-vibe"));
}

#[cfg(unix)]
fn notify_stdin_with_terminal(json: bool) {
    // Allocate a pseudo-terminal and hand the slave fd to the child as stdin.
    // The binary must detect the terminal and return without reading.
    //
    // The master fd stays open for the whole wait on purpose: closing it first
    // makes a read on the slave fail with EIO immediately, so the child exits
    // either way and the test passes with the `is_terminal()` guard removed.
    // With the master held open a read would block forever, which is exactly
    // the hung agent this guards against, so the wait is bounded instead.
    use std::fs::File;
    use std::os::fd::FromRawFd;
    use std::time::{Duration, Instant};

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
    let mut args = vec!["notify", "--agent", "mistral-vibe", "--stdin"];
    if json {
        args.push("--json");
    }
    let mut child = command(&args)
        .stdin(slave_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary spawns");

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait().expect("the child can be waited on") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => break None,
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };

    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    unsafe {
        let _ = libc::close(master);
    }
    assert!(
        status.is_some(),
        "--stdin with a terminal stdin blocked instead of returning"
    );

    let out = child.wait_with_output().expect("the binary runs");
    assert_eq!(stdout(&out), if json { "{}\n" } else { "" });
    assert!(out.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn notify_stdin_with_terminal_is_a_no_op() {
    notify_stdin_with_terminal(false);
}

#[cfg(unix)]
#[test]
fn notify_stdin_with_terminal_supports_json() {
    notify_stdin_with_terminal(true);
}

#[test]
fn notify_stdin_rejects_extra_arguments() {
    // A stray word in a hook line is a typo, and the argv path is loud about
    // them; `--stdin` swallowing them silently is the one place a typo hides.
    let out = run(&["notify", "--agent", "mistral-vibe", "--stdin", "junk"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unexpected arguments: notify junk"));
}

#[test]
fn debug_accepts_any_non_empty_value() {
    // `TMUX_AGENT_STATUS_DISABLED` takes any non-empty value, and one namespace
    // with two spellings is a switch you cannot tell from silence.
    for value in ["1", "true", "yes"] {
        let out = run_env(
            &[
                "notify",
                "--agent",
                "mistral-vibe",
                r#"{"hook_event_name":"unknown"}"#,
            ],
            "TMUX_AGENT_STATUS_DEBUG",
            value,
        );
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("dropped payload"),
            "DEBUG={value} logged nothing"
        );
    }
}

#[test]
fn disabled_notify_still_drains_stdin() {
    // Being disabled must be invisible to the agent. A payload larger than the
    // pipe buffer blocks the agent's write until someone reads it, so exiting
    // early would hand the agent an EPIPE on a hook it was told is a no-op.
    let payload = format!(
        r#"{{"hook_event_name":"pre_tool","pad":"{}"}}"#,
        "x".repeat(256 * 1024)
    );
    let mut child = command(&["notify", "--agent", "mistral-vibe", "--stdin"])
        .env("TMUX_AGENT_STATUS_DISABLED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary spawns");
    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        stdin
            .write_all(payload.as_bytes())
            .expect("a disabled notify must still read what the agent sends");
    }
    let out = child.wait_with_output().expect("the binary runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
