#![cfg(unix)]

//! Exercising the error paths of the tmux subprocess calls.
//!
//! These tests run the binary with a fake or missing `tmux` on `PATH` so the
//! `command::set` / `command::clear_window` / `tmux::tmux` error branches are
//! reached. The CLI's `hook()` wrapper swallows these errors, so every test
//! still expects exit 0 and no output.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-agent-status");
const TMUX_PANE: &str = "%0";
const TMUX: &str = "/tmp/tmux-agent-status-test";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn fake_tmux_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tmux-agent-status-fake-tmux-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_fake_tmux(dir: &std::path::Path, script: &str) {
    let path = dir.join("tmux");
    fs::write(&path, script).expect("write fake tmux");
    let mut perms = fs::metadata(&path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("chmod");
}

fn run(args: &[&str], path: &str) -> Output {
    Command::new(BIN)
        .args(args)
        .env("TMUX", TMUX)
        .env("TMUX_PANE", TMUX_PANE)
        .env("PATH", path)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

fn assert_ok_and_silent(out: &Output) {
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

#[test]
fn set_is_silent_when_tmux_cannot_be_spawned() {
    // An empty PATH directory means Command::new("tmux") returns an io error,
    // covering the `.output()?` path in tmux::tmux and the `?` in command::set.
    let dir = fake_tmux_dir();
    let out = run(&["set", "done"], &dir.display().to_string());
    assert_ok_and_silent(&out);
}

#[test]
fn set_is_silent_when_list_panes_fails() {
    let dir = fake_tmux_dir();
    write_fake_tmux(
        &dir,
        "#!/bin/sh\nif [ \"$1\" = \"set-option\" ]; then exit 0; fi\nexit 1\n",
    );
    let out = run(&["set", "done"], &format!("{}:", dir.display()));
    assert_ok_and_silent(&out);
}

#[test]
fn set_is_silent_when_setting_the_pane_fails() {
    let dir = fake_tmux_dir();
    write_fake_tmux(
        &dir,
        "#!/bin/sh\n\
if [ \"$1\" = \"list-panes\" ]; then\n\
    printf '%s\\t%s\\t%s\\t%s\\n' '%0' '' '0' '0'\n\
    exit 0\n\
fi\n\
exit 1\n",
    );
    let out = run(&["set", "working"], &format!("{}:", dir.display()));
    assert_ok_and_silent(&out);
}

#[test]
fn clear_window_is_silent_when_clearing_a_pane_fails() {
    let dir = fake_tmux_dir();
    write_fake_tmux(
        &dir,
        "#!/bin/sh\n\
if [ \"$1\" = \"list-panes\" ]; then\n\
    printf '%s\t%s\t%s\t%s\n' '%0' 'done' '0' '0'\n\
    exit 0\n\
fi\n\
if [ \"$1\" = \"set-option\" ] && [ \"$2\" = \"-p\" ] && [ \"$3\" = \"-u\" ]; then\n\
    exit 1\n\
fi\n\
exit 0\n",
    );
    let out = run(&["clear-window", TMUX_PANE], &format!("{}:", dir.display()));
    assert_ok_and_silent(&out);
}
