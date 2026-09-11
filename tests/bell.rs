//! The terminal-bell path.
//!
//! `bell::ring()` writes to `/dev/tty` and is best-effort: it must stay silent
//! both when a tty is present and when there is none.

use std::process::{Command, Output, Stdio};

mod support;

/// Run the binary in a new session so there is no controlling terminal.
/// This exercises the `Err(_)` branch of `bell::ring()`.
#[cfg(unix)]
fn run_without_tty(args: &[&str]) -> Output {
    use std::os::unix::process::CommandExt;

    // SAFETY: `setsid` is async-signal-safe and is called immediately after
    // fork, before exec. It detaches the child from the parent's terminal.
    unsafe {
        Command::new(support::BIN)
            .args(args)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .stdin(Stdio::null())
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .output()
    }
    .expect("the binary runs")
}

#[test]
#[cfg(unix)]
fn bell_is_silent_without_a_controlling_terminal() {
    let out = run_without_tty(&["set", "done"]);
    assert!(out.status.success(), "exit: {:?}", out.status);
    assert!(
        out.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
