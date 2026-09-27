//! When all 16 private-dir slots are taken, the probe returns `None` (D4);
//! the next probe succeeds on the slot that opened (the counter moved past
//! the pre-created range).
//!
//! A test binary of its own (D7): it pre-creates dirs under `/tmp` keyed on
//! this pid and checks the probe's interaction with them, which is a pid-wide
//! concern.

use std::fs;

use tmux_agent_status::register::probe;

mod support;

use support::tmux_or_skip;

#[test]
fn exhausted_slots_return_none_and_the_next_probe_succeeds() {
    if !tmux_or_skip() {
        return;
    }
    let pid = std::process::id();

    // Pre-create slots 0..16 (the retry cap). The process-wide counter starts
    // at 0 for this binary, so the first 16 attempts collide.
    let mut pre_created = Vec::new();
    for n in 0..16 {
        let name = format!("/tmp/{}-{}-{}", probe::PRIVATE_PREFIX, pid, n);
        fs::create_dir_all(&name).unwrap_or_else(|e| panic!("create {name}: {e}"));
        pre_created.push(name);
    }

    let config_dir = format!("/tmp/tmux-agent-status-dir-taken-test-{}", pid);
    fs::create_dir_all(&config_dir).expect("scratch dir");
    let config = std::path::PathBuf::from(&config_dir).join("tmux.conf");
    fs::write(&config, "set -g status-left 'LEFT'\n").expect("write config");

    let result = probe::dump(&config);
    assert_eq!(
        result, None,
        "a probe with all slots taken must return None"
    );

    // The pre-created dirs were not touched by the probe.
    for dir in &pre_created {
        assert!(
            std::path::Path::new(dir).is_dir(),
            "the probe removed a pre-created dir: {dir}"
        );
    }

    // The next probe succeeds: the counter moved past 15 to 16, so the slot
    // name no longer collides.
    let result = probe::dump(&config);
    assert!(
        result.is_some(),
        "the probe after exhaustion must succeed on the next slot"
    );

    // Clean up.
    for dir in &pre_created {
        let _ = fs::remove_dir_all(dir);
    }
    let _ = fs::remove_dir_all(&config_dir);
}
