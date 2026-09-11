#![cfg(unix)]

//! Exercising the error paths of the tmux subprocess calls.
//!
//! These tests run the binary with a fake or missing `tmux` on `PATH` so the
//! command policy and `tmux::tmux` error branches are reached. The CLI's
//! `hook()` wrapper swallows these errors, so every test
//! still expects exit 0 and no output.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

mod support;

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

/// The binary, in a pane, with every `TMUX_AGENT_STATUS_*` variable cleared.
///
/// Every per-agent page recommends exporting `TMUX_AGENT_STATUS_PANE`, so a
/// developer who took that advice would otherwise change the pane these tests
/// assert on; an exported `TMUX_AGENT_STATUS_DISABLED` would turn most of this
/// file into a vacuous pass. Inherited state must not decide what a test proves.
fn command(args: &[&str], path: &str) -> Command {
    let mut cmd = Command::new(support::BIN);
    cmd.args(args)
        .env("TMUX", TMUX)
        .env("TMUX_PANE", TMUX_PANE)
        .env("PATH", path)
        .env_remove("TMUX_AGENT_STATUS_PANE")
        .env_remove("TMUX_AGENT_STATUS_DISABLED")
        .env_remove("TMUX_AGENT_STATUS_DEBUG")
        .stdin(Stdio::null());
    cmd
}

fn run(args: &[&str], path: &str) -> Output {
    command(args, path).output().expect("the binary runs")
}

fn run_disabled(args: &[&str], path: &str) -> Output {
    command(args, path)
        .env("TMUX_AGENT_STATUS_DISABLED", "1")
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
fn boundary_commands_are_silent_when_list_panes_fails() {
    let dir = fake_tmux_dir();
    write_fake_tmux(&dir, "#!/bin/sh\nexit 1\n");
    for command in ["reset", "finish"] {
        let out = run(&[command], &format!("{}:", dir.display()));
        assert_ok_and_silent(&out);
    }
}

#[test]
fn set_is_silent_when_tmux_lists_no_panes() {
    // An empty list-panes output means the window has no panes; the rollup code
    // must still handle the empty-slice branch.
    let dir = fake_tmux_dir();
    write_fake_tmux(
        &dir,
        "#!/bin/sh\nif [ \"$1\" = \"list-panes\" ]; then exit 0; fi\nexit 0\n",
    );
    let out = run(&["set", "done"], &format!("{}:", dir.display()));
    assert_ok_and_silent(&out);
}

#[test]
fn set_is_silent_when_list_panes_ignores_the_format() {
    // The tabs in the format are ours, so a line without them is a tmux that
    // did not answer the question asked. Unreadable *values* degrade to "not
    // watched"; an unreadable *line* is an error, and a hook still exits 0.
    let dir = fake_tmux_dir();
    write_fake_tmux(
        &dir,
        "#!/bin/sh\n\
if [ \"$1\" = \"list-panes\" ]; then\n\
    echo badline\n\
    exit 0\n\
fi\n\
exit 0\n",
    );
    let out = run(&["set", "done"], &format!("{}:", dir.display()));
    assert_ok_and_silent(&out);
}

#[test]
fn a_flag_that_cannot_be_read_still_sets_the_state() {
    // The point of reading the flags leniently: a tmux whose `window_active` or
    // `session_attached` this cannot parse loses the immediate clear, not the
    // glyph. Nobody would ever see the error, so it must not cost the feature.
    let dir = fake_tmux_dir();
    let log = dir.join("calls");
    write_fake_tmux(
        &dir,
        &format!(
            "#!/bin/sh\n\
if [ \"$1\" = \"list-panes\" ]; then\n\
    printf '%s\\t%s\\t%s\\t%s\\n' '{TMUX_PANE}' '' 'yes' 'many'\n\
    exit 0\n\
fi\n\
echo \"$@\" >> '{log}'\n\
exit 0\n",
            log = log.display()
        ),
    );

    let out = run(&["set", "done"], &format!("{}:", dir.display()));

    assert_ok_and_silent(&out);
    let calls = fs::read_to_string(&log).expect("the fake tmux logged its calls");
    assert!(
        calls.contains(&format!(
            "set-option -p -t {TMUX_PANE} @agent_pane_status done"
        )),
        "calls: {calls}"
    );
    assert!(
        calls.contains(&format!("set-option -w -t {TMUX_PANE} @agent_status ✅")),
        "calls: {calls}"
    );
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
fn set_is_silent_when_setting_the_window_status_fails() {
    let dir = fake_tmux_dir();
    write_fake_tmux(
        &dir,
        "#!/bin/sh\n\
if [ \"$1\" = \"set-option\" ] && [ \"$2\" = \"-w\" ]; then exit 1; fi\n\
exit 0\n",
    );
    let out = run(&["set", "done"], &format!("{}:", dir.display()));
    assert_ok_and_silent(&out);
}

#[test]
fn clearing_commands_are_silent_when_clearing_a_pane_fails() {
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
    for args in [["clear-window", TMUX_PANE].as_slice(), ["reset"].as_slice()] {
        let out = run(args, &format!("{}:", dir.display()));
        assert_ok_and_silent(&out);
    }
}

#[test]
fn disabled_runs_no_tmux_command() {
    let dir = fake_tmux_dir();
    let log = dir.join("calls");
    write_fake_tmux(
        &dir,
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 1\n", log.display()),
    );

    for args in [
        ["set", "done"].as_slice(),
        ["reset"].as_slice(),
        ["finish"].as_slice(),
        ["clear-window", TMUX_PANE].as_slice(),
    ] {
        let out = run_disabled(args, &format!("{}:", dir.display()));
        assert_ok_and_silent(&out);
    }

    assert!(!log.exists(), "disabled must not invoke tmux");
}

#[test]
fn hook_is_silent_when_tmux_is_not_on_path() {
    // `Command::new("tmux")` fails before it can run anything. The hook wrapper
    // still turns this into a silent exit 0.
    let out = run(&["set", "done"], "/nonexistent");
    assert_ok_and_silent(&out);
}

#[test]
fn set_is_silent_when_tmux_prints_invalid_utf8() {
    // `String::from_utf8` can fail; the error path still exits 0 from a hook.
    let dir = fake_tmux_dir();
    write_fake_tmux(
        &dir,
        "#!/bin/sh\nif [ \"$1\" = \"list-panes\" ]; then python3 -c \"import sys; sys.stdout.buffer.write(b'\\xff\\n')\"; exit 0; fi\nexit 0\n",
    );
    let out = run(&["set", "done"], &format!("{}:", dir.display()));
    assert_ok_and_silent(&out);
}

#[test]
fn hook_is_silent_when_tmux_is_not_executable() {
    // A file called tmux that exists but cannot be executed hits a different
    // spawn error than a missing binary.
    let dir = fake_tmux_dir();
    fs::write(dir.join("tmux"), "#!/bin/sh\nexit 0\n").expect("write tmux stub");
    let out = run(&["set", "done"], &dir.display().to_string());
    assert_ok_and_silent(&out);
}

#[test]
fn tmux_agent_status_pane_overrides_tmux_pane() {
    let dir = fake_tmux_dir();
    let log = dir.join("calls");
    write_fake_tmux(
        &dir,
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", log.display()),
    );
    let out = command(&["set", "done"], &format!("{}:", dir.display()))
        .env("TMUX_AGENT_STATUS_PANE", "%override")
        .output()
        .expect("the binary runs");
    assert_ok_and_silent(&out);
    let calls = fs::read_to_string(&log).expect("fake tmux logged calls");
    assert!(
        calls.contains("set-option -p -t %override @agent_pane_status done"),
        "calls: {calls}"
    );
}

#[test]
fn an_empty_pane_flag_falls_back_to_tmux_pane() {
    // `--pane #{pane_id}` and `--pane "$TMUX_PANE"` are what every per-agent
    // page documents, and both expand to nothing outside tmux. tmux reads an
    // empty `-t` as the *current* pane, so an empty flag must be no flag at all.
    let dir = fake_tmux_dir();
    let log = dir.join("calls");
    write_fake_tmux(
        &dir,
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", log.display()),
    );
    let out = run(
        &["set", "done", "--pane", ""],
        &format!("{}:", dir.display()),
    );
    assert_ok_and_silent(&out);
    let calls = fs::read_to_string(&log).expect("fake tmux logged calls");
    assert!(
        calls.contains(&format!(
            "set-option -p -t {TMUX_PANE} @agent_pane_status done"
        )),
        "calls: {calls}"
    );
    assert!(
        !calls.contains("set-option -p -t @agent_pane_status"),
        "an empty pane must never be passed to tmux: {calls}"
    );
}

#[test]
fn an_empty_pane_flag_outside_tmux_runs_no_tmux_command() {
    let dir = fake_tmux_dir();
    let log = dir.join("calls");
    write_fake_tmux(
        &dir,
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", log.display()),
    );
    let out = command(
        &["set", "done", "--pane", ""],
        &format!("{}:", dir.display()),
    )
    .env_remove("TMUX")
    .env_remove("TMUX_PANE")
    .output()
    .expect("the binary runs");
    assert_ok_and_silent(&out);
    assert!(
        !log.exists(),
        "an empty pane outside tmux must not invoke tmux"
    );
}

#[test]
fn empty_tmux_agent_status_pane_falls_back_to_tmux_pane() {
    let dir = fake_tmux_dir();
    let log = dir.join("calls");
    write_fake_tmux(
        &dir,
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", log.display()),
    );
    let out = command(&["set", "done"], &format!("{}:", dir.display()))
        .env("TMUX_AGENT_STATUS_PANE", "")
        .output()
        .expect("the binary runs");
    assert_ok_and_silent(&out);
    let calls = fs::read_to_string(&log).expect("fake tmux logged calls");
    assert!(
        calls.contains(&format!(
            "set-option -p -t {TMUX_PANE} @agent_pane_status done"
        )),
        "calls: {calls}"
    );
}

#[test]
fn empty_tmux_pane_means_not_in_tmux() {
    // The binary must treat an empty TMUX_PANE as "not inside tmux" and not
    // attempt to invoke tmux at all.
    let dir = fake_tmux_dir();
    let log = dir.join("calls");
    write_fake_tmux(
        &dir,
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 1\n", log.display()),
    );
    let out = command(&["set", "done"], &format!("{}:", dir.display()))
        .env("TMUX_PANE", "")
        .output()
        .expect("the binary runs");
    assert_ok_and_silent(&out);
    assert!(
        !log.exists(),
        "must not invoke tmux when TMUX_PANE is empty"
    );
}
