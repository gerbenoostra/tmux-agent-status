//! The probe leaves no residue: no socket in the user's tmux socket dir, and
//! no private dir under `/tmp`.
//!
//! A test binary of its own, holding exactly one test, because it checks what
//! *this pid* left under `/tmp`: `cargo test` runs one binary's tests as
//! threads of one process, so a pid-wide check in `register_probe.rs` would
//! see other tests' live private dirs.

use std::fs;
use std::time::{Duration, Instant};

use tmux_agent_status::register::probe;

mod support;

use support::tmux_or_skip;

/// Whether `<dir>/tmux-<uid>/` has any entry at all.
fn socket_dir_has_entries(scratch: &std::path::Path) -> bool {
    let uid_dir = scratch.join(format!("tmux-{}", unsafe { libc::getuid() }));
    fs::read_dir(&uid_dir)
        .map(|entries| entries.flatten().count() > 0)
        .unwrap_or(false)
}

/// How many private dirs this process still has under the probe private root.
fn private_dirs_for_pid(pid: u32) -> Vec<String> {
    let prefix = format!("{}-{}-", probe::PRIVATE_PREFIX, pid);
    fs::read_dir(probe::private_root())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn probes_leave_no_residue_in_the_socket_dir_or_under_tmp() {
    if !tmux_or_skip() {
        return;
    }
    let scratch = probe::private_root().join(format!(
        "tmux-agent-status-residue-test-{}-{}",
        std::process::id(),
        0
    ));
    fs::create_dir_all(&scratch).expect("scratch dir");

    // SAFETY: this binary holds one test, so nothing else is running.
    unsafe { std::env::set_var("TMUX_TMPDIR", &scratch) };

    // A normal dump.
    let config = scratch.join("tmux.conf");
    fs::write(&config, "set -g status-left 'LEFT'\n").expect("write config");
    let _ = probe::dump(&config);

    // A compiled-in default probe.
    let _ = probe::compiled_in_default();

    // A check probe.
    let _ = probe::check(&config);

    // A stuck config: the probe must time out and still clean up.
    let stuck = scratch.join("stuck.conf");
    fs::write(&stuck, "run-shell 'sleep 3'\n").expect("write stuck config");
    let started = Instant::now();
    let found = probe::dump_within(&stuck, Duration::from_millis(500));
    let waited = started.elapsed();
    assert_eq!(found, None, "a stuck config must not produce a dump");
    assert!(
        waited < Duration::from_millis(900),
        "Drop waited a second time after handing cleanup to the reaper: {waited:?}"
    );

    // The user's tmux socket dir holds no entry the probes created.
    assert!(
        !socket_dir_has_entries(&scratch),
        "a probe created something in the socket dir"
    );

    // Wait for the timed-out server's reaper to finish, with a bound.
    let pid = std::process::id();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let remaining = private_dirs_for_pid(pid);
        if remaining.is_empty() || Instant::now() >= deadline {
            assert!(
                remaining.is_empty(),
                "private dirs remain after 30 s: {remaining:?}"
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Clean up the scratch dir.
    unsafe { std::env::remove_var("TMUX_TMPDIR") };
    let _ = fs::remove_dir_all(scratch);
}
