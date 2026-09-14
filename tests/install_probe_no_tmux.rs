//! What the probe does when there is no tmux to ask.
//!
//! A test binary of its own, holding exactly one test, because it works by
//! setting an environment variable: `cargo test` runs the tests of one binary
//! in threads of one process, and a variable set in one of them is set for all
//! of them. That is the same reason `write::Faults` is a value rather than a
//! read of the environment, and this is the one place where the switch has to
//! reach code that reads it directly.

mod support;

use support::tempdir::TempDir;
use tmux_agent_status::install::probe;

#[test]
fn a_tmux_that_stops_answering_is_no_answer_rather_than_a_wrong_one() {
    let dir = TempDir::new("probe-mute");
    let config = dir.write("tmux.conf", "set -g status on\n");

    // SAFETY: this binary holds one test, so nothing else is running.
    unsafe { std::env::set_var("TMUX_AGENT_STATUS_TEST_FAULT", "no-tmux") };

    // Every tmux invocation is optional and its failure is not an error, which
    // is what lets a config be installed before tmux is.
    assert_eq!(probe::dump(&config), None);
    assert_eq!(probe::check(&config), None);
    assert_eq!(probe::compiled_in_default(), None);
}
