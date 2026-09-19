//! What `register` does in a process with no `$HOME`.
//!
//! A test binary of its own, holding one test, because it works by removing an
//! environment variable: `cargo test` runs the tests of one binary in threads
//! of one process, and a variable removed in one of them is removed for all of
//! them. `tests/register_probe_no_tmux.rs` is the same shape for the same
//! reason.
//!
//! `$HOME` is where both the walk and the probe resolve a relative
//! `source-file`, so its absence is the one input that changes what either of
//! them does with one. A daemon, a `systemd` unit and a `su -c` all run without
//! it, and none of them should make this crash or guess.

mod support;

use support::tempdir::TempDir;
use tmux_agent_status::register::{probe, tmux_conf};

#[test]
fn with_no_home_a_relative_source_stands_as_the_config_wrote_it() {
    let dir = TempDir::new("no-home");
    // A fragment beside the config, which is the directory we must *not* fall
    // back to: reading it would put a winner in the walk out of a file tmux
    // never opened.
    dir.write(
        "fragment.conf",
        "set -g window-status-format 'beside the config'\n",
    );
    let config = dir.write(
        "tmux.conf",
        "source-file fragment.conf\nset -g window-status-format 'in the config'\n",
    );

    // SAFETY: this binary holds one test, so nothing else is running.
    unsafe { std::env::remove_var("HOME") };

    let walked = tmux_conf::walk(&config);

    // The relative path resolves against nothing, so it names `fragment.conf`
    // in the process's own working directory, which is not where it is. The
    // config's own assignment is the only one found.
    let values: Vec<String> = walked
        .assignments
        .into_iter()
        .filter_map(|found| found.candidate.line())
        .map(|line| line.raw)
        .collect();
    assert_eq!(values, ["in the config"]);
    // And the guess is still reported, which is what it is for.
    assert_eq!(walked.relative_sources, ["fragment.conf"]);

    // The probe runs from wherever it is rather than refusing to start, so a
    // config with nothing relative in it is still checked.
    if support::tmux_or_skip() {
        assert!(probe::dump(&config).is_some());
    }
}
