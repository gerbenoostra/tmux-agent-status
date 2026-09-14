//! The probe, against a real tmux.
//!
//! These are the tests that make the probe worth its cost. Each one is a config
//! the parser might get wrong, checked by the thing that actually decides.
//!
//! The behaviour they rest on was confirmed on tmux 3.6a: a stray argument, or
//! an unknown command name, makes tmux discard the **whole** config file and
//! fall back to its compiled-in defaults - silently, exiting 0, writing nothing
//! to stderr. Nothing short of comparing the whole option dump can see that.

use std::fs;
use std::path::Path;

use tmux_agent_status::install::format;
use tmux_agent_status::install::probe;

mod support;

use support::tempdir::TempDir;
use support::tmux_or_skip;

fn dump(path: &Path) -> probe::Dump {
    probe::dump(path).expect("a throwaway server can be started on the config")
}

#[test]
fn the_compiled_in_default_is_read_from_this_tmux() {
    if !tmux_or_skip() {
        return;
    }
    let found = probe::compiled_in_default().expect("this tmux answers");

    // Not asserted to equal the 3.6a value: the point of asking is that the
    // default has changed between versions. What must hold is that it is a
    // format, and that our splice lands somewhere sensible in it.
    assert!(!found.is_empty());
    assert!(found.contains("#I") || found.contains("#W"), "{found}");
    assert!(format::splice(&found).contains(format::TERM));
}

// 24. The probe passes a good splice, and the dump diff contains only the two
// format values.
#[test]
fn a_good_splice_changes_only_the_two_format_values() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-good");
    let before = dir.write(
        "tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\nset -g status-left 'LEFT'\n",
    );
    let baseline = dump(&before);

    let after = dir.write(
        "after.conf",
        &format!(
            "set -g window-status-format '#I:#W{term}'\nset -g window-status-current-format '#I:#W{term}'\nset -g status-left 'LEFT'\n",
            term = format::TERM
        ),
    );
    let candidate = dump(&after);

    let changed: Vec<String> = probe::changes(&baseline, &candidate)
        .into_iter()
        .map(|change| change.name)
        .collect();
    let mut expected: Vec<String> = format::OPTIONS.iter().map(|o| (*o).to_owned()).collect();
    expected.sort();
    assert_eq!(changed, expected, "unexpected changes: {changed:?}");
}

// 23. The probe catches an abandoned config.
#[test]
fn the_probe_catches_a_config_tmux_abandons() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-abandoned");
    let before = dir.write(
        "tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g status-left 'LEFT'\n",
    );
    let baseline = dump(&before);

    // A stray argument after the value: exactly what a quoting bug produces.
    let broken = dir.write(
        "broken.conf",
        "set -g window-status-format '#I:#W' stray\nset -g status-left 'LEFT'\n",
    );
    let candidate = dump(&broken);

    let changed = probe::changes(&baseline, &candidate);
    assert!(
        changed.len() > 1,
        "an abandoned config must not look like one intended change: {changed:?}"
    );
    assert!(
        changed.iter().any(|change| change.name == "status-left"),
        "a line the edit never touched reverted, and that is the tell: {changed:?}"
    );
}

#[test]
fn an_unknown_command_abandons_the_config_too() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-unknown");
    let before = dir.write("tmux.conf", "set -g status-left 'LEFT'\n");
    let baseline = dump(&before);

    let broken = dir.write(
        "broken.conf",
        "set -g status-left 'LEFT'\nnot-a-tmux-command foo\n",
    );
    let candidate = dump(&broken);

    assert!(
        !probe::changes(&baseline, &candidate).is_empty(),
        "an unknown command must show up in the dump"
    );
}

// 25. A pre-broken config is refused, not edited: the baseline is defaults, and
// the tool can see that before it writes anything.
#[test]
fn a_config_that_was_already_broken_shows_it_in_the_baseline() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-pre-broken");
    let empty = dir.write("empty.conf", "");
    let defaults = dump(&empty);

    let pre_broken = dir.write(
        "tmux.conf",
        "set -g status-left 'LEFT'\nset -g window-status-format '#I:#W' stray\n",
    );
    let baseline = dump(&pre_broken);

    // The config plainly sets `status-left`, and the baseline does not carry
    // it: the user's config was abandoned before we arrived, and editing it
    // would produce an install nobody could validate.
    assert!(
        probe::changes(&defaults, &baseline).is_empty(),
        "a pre-broken config must read back as tmux's defaults"
    );
    let text = fs::read_to_string(&pre_broken).expect("the config can be read");
    assert!(text.contains("status-left"), "the fixture sets something");
}

// 26. The probe cleans up: no server on the probe socket afterwards.
#[test]
fn the_probe_leaves_no_server_behind() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-cleanup");
    let config = dir.write("tmux.conf", "set -g status-left 'LEFT'\n");
    let _ = dump(&config);
    let _ = probe::compiled_in_default();

    let mine = format!("tmux-agent-status-probe-{}-", std::process::id());
    assert_eq!(probe_sockets(&mine), 0, "a probe server was left running");
}

/// How many probe sockets this process still has out there.
fn probe_sockets(prefix: &str) -> usize {
    fs::read_dir(probe::socket_dir())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
                .count()
        })
        .unwrap_or(0)
}

// 27. A relative `source-file` in the config: the probe runs with cwd `$HOME`,
// which is where tmux itself would resolve it from.
#[test]
fn a_relative_source_is_resolved_against_home_not_the_config() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-relative");
    dir.write("fragment.conf", "set -g status-left 'FROM THE FRAGMENT'\n");
    let config = dir.write("tmux.conf", "source-file fragment.conf\n");

    let found = dump(&config);
    let defaults = dump(&dir.write("empty.conf", ""));

    // The fragment sits beside the config, not in `$HOME`, so tmux does not
    // find it - which is the case being pinned, because it is exactly what the
    // user's own tmux does with that line.
    assert!(
        probe::changes(&defaults, &found).is_empty(),
        "a relative source resolved against the config's own directory"
    );
}

#[test]
fn a_config_that_is_not_there_reads_back_as_tmuxs_defaults() {
    if !tmux_or_skip() {
        return;
    }
    // Verified on 3.6a: `tmux -f <missing>` starts happily and exits 0, so a
    // missing file is indistinguishable from an empty one. The probe is only
    // ever pointed at a file that was just written, which is what makes that
    // harmless - and it is worth pinning, because the opposite assumption
    // would have the caller read "no answer" into a perfectly good dump.
    let dir = TempDir::new("probe-missing");
    let missing = probe::dump(Path::new("/nowhere/at/all.conf")).expect("tmux starts anyway");
    let empty = dump(&dir.write("empty.conf", ""));

    assert!(probe::changes(&empty, &missing).is_empty());
}

// A config that blocks must not block the install.
#[test]
fn a_config_that_blocks_is_given_up_on_and_leaves_nothing_running() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-slow");
    // Verified on 3.6a: `run-shell` without `-b` holds up `new-session -d` for
    // as long as the command takes.
    let config = dir.write("tmux.conf", "run-shell 'sleep 3'\n");
    let mine = format!("tmux-agent-status-probe-{}-", std::process::id());

    let started = std::time::Instant::now();
    let found = probe::dump_within(&config, std::time::Duration::from_millis(500));
    let waited = started.elapsed();

    assert_eq!(found, None, "a config that blocks must not produce a dump");
    assert!(
        waited < std::time::Duration::from_secs(2),
        "the install was held up for {waited:?}"
    );

    // The server is wedged in its own config and cannot answer `kill-server`
    // until it finishes, so the kill was spawned rather than waited on. It
    // still goes, a moment later.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while probe_sockets(&mine) > 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(probe_sockets(&mine), 0, "a probe server was left running");
}

#[test]
fn a_config_tmux_will_not_read_is_named_line_and_reason() {
    if !tmux_or_skip() {
        return;
    }
    let dir = TempDir::new("probe-check");
    let good = dir.write("good.conf", "set -g status-left 'LEFT'\n");
    assert_eq!(probe::check(&good), Some(Ok(())));

    // Verified on 3.6a: at server start the same file is abandoned whole, in
    // silence, exiting 0. `source-file` on a running server is the only channel
    // that says anything at all.
    let bad = dir.write("bad.conf", "set -g status-left 'LEFT' stray-argument\n");
    let complaint = probe::check(&bad)
        .expect("tmux answered")
        .expect_err("a broken config is refused");
    assert!(complaint.contains("bad.conf"), "{complaint}");
    assert!(complaint.contains(":1:"), "no line number: {complaint}");
    assert!(complaint.contains("too many arguments"), "{complaint}");
}

#[test]
fn a_reload_that_tmux_refuses_is_reported_rather_than_swallowed() {
    if !tmux_or_skip() {
        return;
    }
    // A path nothing can source. Whatever server this reaches, nothing is
    // applied to it: the point is that the refusal comes back as an error.
    let error = probe::reload(Path::new("/nonexistent/tmux-agent-status-no-such.conf"))
        .expect_err("tmux cannot source a file that is not there");
    assert!(error.to_string().contains("source-file"), "{error}");
}

#[test]
fn the_running_server_can_be_asked_what_it_loaded() {
    if !tmux_or_skip() {
        return;
    }
    // Outside tmux there is usually no server, and every one of these is
    // optional by design: `None` is an answer, not a failure.
    let _ = probe::config_files();
}
