//! What `register` does in a process with no `$HOME`.
//!
//! A test binary of its own, holding one test, because it works by removing an
//! environment variable: `cargo test` runs the tests of one binary in threads
//! of one process, and a variable removed in one of them is removed for all of
//! them. `tests/register_probe_no_tmux.rs` is the same shape for the same
//! reason.
//!
//! `$HOME` is the one input the probe resolves a relative `source-file`
//! against. Everything else that used to read it here no longer can: `main`
//! refuses to run `register` without a `$HOME`, and the walk resolves against
//! the `Home` it is handed rather than the environment. A daemon, a `systemd`
//! unit and a `su -c` all run without it, and none of them should make this
//! crash.

mod support;

use support::tempdir::TempDir;
use tmux_agent_status::register::probe;

#[test]
fn with_no_home_the_probe_runs_from_wherever_it_is() {
    let dir = TempDir::new("no-home");
    let config = dir.write("tmux.conf", "set -g window-status-format 'in the config'\n");

    // SAFETY: this binary holds one test, so nothing else is running.
    unsafe { std::env::remove_var("HOME") };

    // The probe runs from wherever it is rather than refusing to start, so a
    // config is still checked even when there is no `$HOME` to put its cwd in.
    if support::tmux_or_skip() {
        assert!(probe::dump(&config).is_some());
    }
}
