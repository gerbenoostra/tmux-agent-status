//! `register` as a user runs it: a real process, a real `$HOME`, real files.
//!
//! Every test here points the binary at a temp home, because the whole point of
//! reading `$HOME` from the environment rather than `getpwuid` is that a tool
//! which cannot be redirected cannot be tested.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod support;

use support::tempdir::TempDir;

/// The binary, pointed at a temp home, with nothing inherited that could
/// decide what a test proves.
fn register(home: &TempDir, args: &[&str]) -> Output {
    command(home, args).output().expect("the binary runs")
}

fn command(home: &TempDir, args: &[&str]) -> Command {
    let mut cmd = Command::new(support::BIN);
    cmd.arg("register")
        .args(args)
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("TMUX_AGENT_STATUS_PANE")
        .env_remove("TMUX_AGENT_STATUS_DISABLED")
        .env_remove("TMUX_AGENT_STATUS_TEST_FAULT")
        // No terminal: `cargo test` gives its children none either way, and
        // that is the case worth pinning.
        .stdin(Stdio::null());
    cmd
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("the process exited normally")
}

/// A `$PATH` holding only a stub of our own, so nothing installed on the test
/// machine decides what a test sees.
fn only(dir: &Path) -> String {
    dir.display().to_string()
}

/// A shell script that behaves as `name`, and nothing else does.
fn stub(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).expect("the bin directory");
    let path = bin.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("the stub");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// Everything a run could leave behind that it promised not to.
fn residue(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            found.extend(residue(&path));
        } else if name.contains(".bak-") || name.contains(".tmp-") || name.contains(".lock") {
            found.push(name);
        }
    }
    found
}

// 19. Non-TTY without `-y` exits 2 with a message naming `-y`.
#[test]
fn a_question_with_nobody_to_answer_it_is_a_usage_error() {
    let home = TempDir::new("cli-no-tty");
    let out = register(&home, &[]);

    assert_eq!(code(&out), 2, "{}", stderr(&out));
    let message = stderr(&out);
    assert!(message.contains("-y"), "{message}");
    assert!(message.contains("--dry-run"), "{message}");
    assert!(residue(home.path()).is_empty());
}

// 18. `--dry-run` changes nothing, and leaves nothing behind.
#[test]
fn a_dry_run_prints_a_plan_and_touches_nothing() {
    let home = TempDir::new("cli-dry-run");
    fs::create_dir_all(home.join(".codex")).expect("the directory");
    let before = home.entries();

    let out = register(&home, &["--dry-run", "--agents=codex"]);

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("This is what register would do:"), "{text}");
    assert!(text.contains("Codex CLI"), "{text}");
    assert!(text.contains(".codex/hooks.json"), "{text}");
    assert!(
        text.contains("--dry-run: nothing above was written."),
        "{text}"
    );

    assert_eq!(home.entries(), before, "a dry run created something");
    assert!(!home.join(".codex/hooks.json").exists());
    assert!(
        residue(home.path()).is_empty(),
        "{:?}",
        residue(home.path())
    );
}

#[test]
fn a_dry_run_of_the_tmux_steps_leaves_no_probe_socket_behind() {
    let home = TempDir::new("cli-dry-run-tmux");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    fs::write(
        home.join(".config/tmux/tmux.conf"),
        "set -g status-left 'LEFT'\n",
    )
    .expect("the config");

    let out = register(&home, &["--dry-run", "--no-agents"]);

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(residue(home.path()).is_empty());

    // The probe server's socket is the one thing a dry run does create, and it
    // is created and killed inside the call.
    let mine = "tmux-agent-status-probe-";
    let left = fs::read_dir(
        std::env::var_os("TMUX_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp")),
    )
    .map(|entries| {
        entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(mine))
            .count()
    })
    .unwrap_or(0);
    assert_eq!(left, 0, "a probe server was left running");
}

#[test]
fn an_unknown_agent_name_is_a_usage_error_listing_the_valid_ones() {
    let home = TempDir::new("cli-bad-agent");
    let out = register(&home, &["--dry-run", "--agents=cursur"]);

    assert_eq!(code(&out), 2);
    let message = stderr(&out);
    assert!(message.contains("unknown agent `cursur`"), "{message}");
    assert!(message.contains("cursor"), "{message}");
}

#[test]
fn mixing_positive_and_negative_step_flags_is_a_usage_error() {
    let home = TempDir::new("cli-mixed-steps");
    let out = register(&home, &["--dry-run", "--agents=codex", "--no-tmux-hook"]);

    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("cannot be given together"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_unknown_claude_route_is_a_usage_error() {
    let home = TempDir::new("cli-bad-route");
    let out = register(&home, &["--dry-run", "--claude-route=sideways"]);

    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("valid routes are"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_real_run_registers_an_agent_and_a_second_run_writes_nothing() {
    let home = TempDir::new("cli-idempotent");
    let target = home.join(".codex/hooks.json");

    let first = register(&home, &["-y", "--agents=codex"]);
    assert_eq!(code(&first), 0, "{}", stderr(&first));
    assert!(target.is_file(), "{}", stdout(&first));
    let written = fs::read_to_string(&target).expect("the file");
    assert!(written.contains("tmux-agent-status"), "{written}");
    assert!(stdout(&first).contains("created"), "{}", stdout(&first));
    // Created, so nothing was lost and no backup was taken.
    assert!(
        residue(home.path()).is_empty(),
        "{:?}",
        residue(home.path())
    );

    // 17, through the CLI: the second run writes no file and takes no backup.
    let mtime = fs::metadata(&target)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let second = register(&home, &["-y", "--agents=codex"]);
    assert_eq!(code(&second), 0, "{}", stderr(&second));
    assert!(
        stdout(&second).contains("already registered"),
        "{}",
        stdout(&second)
    );
    assert_eq!(
        fs::metadata(&target)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        mtime,
        "the second run rewrote the file"
    );
    assert!(
        residue(home.path()).is_empty(),
        "{:?}",
        residue(home.path())
    );
}

#[test]
fn a_merge_into_a_file_the_user_maintains_keeps_what_was_there() {
    let home = TempDir::new("cli-merge");
    fs::create_dir_all(home.join(".codex")).expect("the directory");
    fs::write(
        home.join(".codex/hooks.json"),
        r#"{"theirs": 1, "hooks": {"SessionStart": [{"command": "theirs"}]}}"#,
    )
    .expect("the config");

    let out = register(&home, &["-y", "--agents=codex"]);

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let after = fs::read_to_string(home.join(".codex/hooks.json")).expect("the file");
    assert!(after.contains("\"theirs\""), "{after}");
    assert!(after.contains("tmux-agent-status"), "{after}");
    // An edit, so a backup, and it is named in the output.
    let backups = residue(home.path());
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert!(stdout(&out).contains(".bak-"), "{}", stdout(&out));
}

// 21. With `claude` absent, the same run merges into `settings.json`.
#[test]
fn with_no_claude_on_the_path_the_settings_merge_is_the_route() {
    let home = TempDir::new("cli-no-claude");
    let empty = home.join("bin");
    fs::create_dir_all(&empty).expect("the bin directory");

    let out = command(&home, &["-y", "--agents=claude-code"])
        .env("PATH", only(&empty))
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let settings = fs::read_to_string(home.join(".claude/settings.json")).expect("the file");
    assert!(settings.contains("tmux-agent-status"), "{settings}");
}

// 21. The Claude route does not fall back on failure: with a stub `claude` that
// exits non-zero, the step fails and `~/.claude/settings.json` is untouched.
#[test]
fn a_failing_claude_fails_the_step_and_never_writes_settings_json() {
    let home = TempDir::new("cli-claude-fails");
    stub(&home, "claude", "echo '[]'; exit 0");
    // `plugin list` answers, so the plugin is "not installed"; the install
    // itself is what fails.
    stub(
        &home,
        "claude",
        "case \"$2\" in list) echo '[]' ;; *) exit 7 ;; esac",
    );

    let out = command(&home, &["-y", "--agents=claude-code"])
        .env("PATH", only(&home.join("bin")))
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 1, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("--claude-route=settings"), "{text}");
    assert!(
        !home.join(".claude/settings.json").exists(),
        "settings.json was written as a consequence of a failure: {text}"
    );
}

#[test]
fn an_installed_plugin_leaves_settings_json_alone() {
    // 004's promise, through the CLI: on a machine with the plugin installed,
    // nothing of ours goes anywhere near that file.
    let home = TempDir::new("cli-claude-installed");
    stub(
        &home,
        "claude",
        "echo '[{\"id\": \"tmux-agent-status@tmux-agent-status\"}]'",
    );

    let out = command(&home, &["-y", "--agents=claude-code"])
        .env("PATH", only(&home.join("bin")))
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("already registered"),
        "{}",
        stdout(&out)
    );
    assert!(!home.join(".claude/settings.json").exists());
}

#[test]
fn forcing_the_settings_route_skips_the_plugin_even_when_claude_is_there() {
    let home = TempDir::new("cli-claude-forced");
    stub(&home, "claude", "echo '[]'");

    let out = command(
        &home,
        &["-y", "--agents=claude-code", "--claude-route=settings"],
    )
    .env("PATH", only(&home.join("bin")))
    .output()
    .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        home.join(".claude/settings.json").is_file(),
        "{}",
        stdout(&out)
    );
}

#[test]
fn forcing_the_plugin_route_without_claude_fails_rather_than_merging() {
    let home = TempDir::new("cli-claude-forced-plugin");
    let empty = home.join("bin");
    fs::create_dir_all(&empty).expect("the bin directory");

    let out = command(
        &home,
        &["-y", "--agents=claude-code", "--claude-route=plugin"],
    )
    .env("PATH", only(&empty))
    .output()
    .expect("the binary runs");

    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert!(!home.join(".claude/settings.json").exists());
}

// 22. No tmux on `PATH`: everything degrades, and the run still registers what
// it can.
#[test]
fn with_no_tmux_at_all_the_run_still_registers_what_it_can() {
    let home = TempDir::new("cli-no-tmux");
    let empty = home.join("bin");
    fs::create_dir_all(&empty).expect("the bin directory");
    fs::create_dir_all(home.join(".codex")).expect("the directory");

    let out = command(&home, &["-y"])
        .env("PATH", only(&empty))
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(home.join(".codex/hooks.json").is_file(), "{text}");
    // Registering the config before installing tmux is a legitimate order to do
    // things in, so the config is still written even though nothing could be
    // checked against a live tmux.
    assert!(
        home.join(".config/tmux/tmux.conf").is_file(),
        "the tmux config was not created: {text}"
    );
    let written = fs::read_to_string(home.join(".config/tmux/tmux.conf")).expect("the config");
    assert!(written.contains("@agent_status"), "{written}");
    assert!(written.contains("source-file"), "{written}");
}

#[test]
fn a_refusal_the_user_can_act_on_is_not_a_failure() {
    let home = TempDir::new("cli-mixed");
    fs::create_dir_all(home.join(".codex")).expect("the directory");
    // A file that is not JSON, so the agent step cannot merge into it.
    fs::write(home.join(".codex/hooks.json"), "not json at all\n").expect("the config");

    let out = register(&home, &["-y", "--agents=codex"]);

    // A refusal the user can act on is not a failure: the step reports "not
    // registered, here is what to do" and the run exits 0.
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("not registered"), "{text}");
    assert!(text.contains("by hand"), "{text}");
    assert_eq!(
        fs::read_to_string(home.join(".codex/hooks.json")).expect("the file"),
        "not json at all\n",
        "the file was changed despite the refusal"
    );
}

#[test]
fn a_write_that_does_not_land_fails_the_run_and_restores_the_file() {
    let home = TempDir::new("cli-fault");
    fs::create_dir_all(home.join(".codex")).expect("the directory");
    fs::write(home.join(".codex/hooks.json"), "{\"theirs\": 1}\n").expect("the config");

    let out = command(&home, &["-y", "--agents=codex"])
        .env("TMUX_AGENT_STATUS_TEST_FAULT", "truncate")
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert_eq!(
        fs::read_to_string(home.join(".codex/hooks.json")).expect("the file"),
        "{\"theirs\": 1}\n",
        "the file was not restored"
    );
    assert!(stdout(&out).contains("restored from"), "{}", stdout(&out));
}

// 20. Exit codes for a mixed run: one target already registered, one applied,
// one failed. The run does what it can and says exactly what it did not.
#[test]
fn a_mixed_run_finishes_the_work_it_can_and_exits_one() {
    let home = TempDir::new("cli-mixed-outcomes");
    // Already registered: a Kiro file byte-identical to the shipped one.
    let kiro = home.join(".kiro/hooks/tmux-agent-status.json");
    fs::create_dir_all(kiro.parent().expect("a parent")).expect("the directory");
    let shipped = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("share/agents/kiro/tmux-agent-status.json"),
    )
    .expect("the shipped file");
    fs::write(&kiro, &shipped).expect("the file");
    // To fail: a `claude` that answers `plugin list` and refuses to install.
    stub(
        &home,
        "claude",
        "case \"$2\" in list) echo '[]' ;; *) exit 7 ;; esac",
    );

    let out = command(&home, &["-y", "--agents=kiro,grok,claude-code"])
        .env("PATH", only(&home.join("bin")))
        .output()
        .expect("the binary runs");

    let text = stdout(&out);
    assert_eq!(code(&out), 1, "{text}");
    assert!(text.contains("already registered"), "{text}");
    // The step that could be done was not abandoned because another failed.
    assert!(
        home.join(".grok/hooks/tmux-agent-status.json").is_file(),
        "the work that could be finished was not: {text}"
    );
    assert_eq!(
        fs::read_to_string(&kiro).expect("the file"),
        shipped,
        "an already-registered file was rewritten"
    );
    // And the failure is loud, retryable, and names the way out.
    assert!(text.contains("--claude-route=settings"), "{text}");
    assert!(!home.join(".claude/settings.json").exists(), "{text}");
}

#[test]
fn a_negative_step_flag_skips_exactly_that_step() {
    let home = TempDir::new("cli-no-format");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    fs::write(
        home.join(".config/tmux/tmux.conf"),
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    )
    .expect("the config");

    let out = register(&home, &["-y", "--no-agents", "--no-tmux-format"]);

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let written = fs::read_to_string(home.join(".config/tmux/tmux.conf")).expect("the config");
    assert!(
        written.contains("source-file"),
        "the hook step was skipped too"
    );
    assert!(
        !written.contains("@agent_status"),
        "--no-tmux-format did not skip the format step:\n{written}"
    );
}

#[test]
fn the_probe_can_be_turned_off_and_the_edit_still_lands() {
    let home = TempDir::new("cli-no-probe");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    fs::write(home.join(".config/tmux/tmux.conf"), "set -g status on\n").expect("the config");
    // Every throwaway probe server runs on a socket named with this prefix
    // (see `Server::start_on`); a stub that logs its own invocations lets the
    // test tell "no probe ran" from "no tmux is installed to probe with".
    let log = home.join("tmux-invocations.log");
    stub(&home, "tmux", &format!("echo \"$@\" >> {}", log.display()));

    let out = command(&home, &["-y", "--no-agents", "--no-tmux-probe"])
        .env("PATH", only(&home.join("bin")))
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let invocations = fs::read_to_string(&log).unwrap_or_else(|_| {
        panic!("the stub was never invoked at all, so this proves nothing about the probe")
    });
    // A positive control: the stub really did run (for the ambient
    // `display-message` lookup), so its absence below is not just a PATH
    // that never reached tmux in the first place.
    assert!(
        invocations.contains("display-message"),
        "the stub was not exercised, so the assertion below would pass vacuously:\n{invocations}"
    );
    assert!(
        !invocations.contains("tmux-agent-status-probe-"),
        "--no-tmux-probe did not stop a throwaway server from starting:\n{invocations}"
    );
    // The edit still lands, from the documented default rather than a probed
    // one.
    let written = fs::read_to_string(home.join(".config/tmux/tmux.conf")).expect("the config");
    assert!(written.contains("@agent_status"), "{written}");
}

#[test]
fn a_target_in_a_directory_we_cannot_write_is_handed_back() {
    let home = TempDir::new("cli-unwritable");
    let codex = home.join(".codex");
    fs::create_dir_all(&codex).expect("the directory");
    fs::write(codex.join("hooks.json"), "{}\n").expect("the config");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o555)).expect("chmod");

    let out = register(&home, &["-y", "--agents=codex"]);

    // A refusal the user can act on, not a failure: they are left exactly
    // where they were, holding the block they need.
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("not registered"), "{text}");
    assert!(text.contains("not writable"), "{text}");
    assert_eq!(
        fs::read_to_string(codex.join("hooks.json")).expect("the file"),
        "{}\n"
    );
}

#[test]
fn a_tmux_config_we_cannot_write_is_handed_back_with_the_term() {
    let home = TempDir::new("cli-unwritable-tmux");
    let dir = home.join(".config/tmux");
    fs::create_dir_all(&dir).expect("the directory");
    fs::write(dir.join("tmux.conf"), "set -g status on\n").expect("the config");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("chmod");

    // The snippet goes somewhere writable, so this is about the config file
    // alone: whether a shipped snippet is found beside the binary depends on
    // how the binary under test was laid out, which is not what this is for.
    let snippet = home.join("snippet.conf");
    let out = register(
        &home,
        &[
            "-y",
            "--no-agents",
            "--snippet",
            &snippet.display().to_string(),
        ],
    );

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("not registered"), "{text}");
    assert!(text.contains("not writable"), "{text}");
    // Both steps hand something back, and the format step prints the term.
    assert!(
        text.contains("#{?@agent_status, #{@agent_status},}"),
        "{text}"
    );
}

#[test]
fn a_tmux_edit_that_does_not_land_is_restored_like_any_other() {
    let home = TempDir::new("cli-tmux-truncate");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    let config = home.join(".config/tmux/tmux.conf");
    fs::write(
        &config,
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    )
    .expect("the config");
    let before = fs::read_to_string(&config).expect("the config");

    let out = command(&home, &["-y", "--tmux-format"])
        .env("TMUX_AGENT_STATUS_TEST_FAULT", "truncate")
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert_eq!(
        fs::read_to_string(&config).expect("the config"),
        before,
        "the config was not restored"
    );
    assert!(stdout(&out).contains("restored from"), "{}", stdout(&out));
}

#[test]
fn a_snippet_we_cannot_write_is_handed_back_rather_than_failing() {
    let home = TempDir::new("cli-snippet-unwritable");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    fs::write(home.join(".config/tmux/tmux.conf"), "set -g status on\n").expect("the config");
    let locked = home.join("locked");
    fs::create_dir_all(&locked).expect("the directory");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).expect("chmod");

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-hook",
            "--snippet",
            &locked.join("snippet.conf").display().to_string(),
        ],
    );

    // Nothing was written, and the user is told why while they still have a
    // config that works.
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(stdout(&out).contains("not writable"), "{}", stdout(&out));
    assert!(
        !fs::read_to_string(home.join(".config/tmux/tmux.conf"))
            .expect("the config")
            .contains("source-file"),
        "a source line was written for a snippet that could not be"
    );
}

#[test]
fn a_value_that_cannot_be_requoted_is_handed_back_with_the_term() {
    let home = TempDir::new("cli-unquotable");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    let config = home.join(".config/tmux/tmux.conf");
    // A bare value carrying a backslash: single quotes would make the escape
    // literal and change what the user meant.
    fs::write(&config, "set -g window-status-format back\\ slash\n").expect("the config");
    let before = fs::read_to_string(&config).expect("the config");

    let out = register(&home, &["-y", "--tmux-format"]);

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("cannot be requoted safely"), "{text}");
    assert!(
        text.contains("#{?@agent_status, #{@agent_status},}"),
        "{text}"
    );
    let after = fs::read_to_string(&config).expect("the config");
    // The line that was handed back is untouched, to the byte.
    assert!(
        after.starts_with(before.trim_end()),
        "the refused line was rewritten:\n{after}"
    );
    // And the *other* option, which nothing assigned, still gets its line:
    // one being unrewritable is no reason to leave the other bare.
    assert!(
        after.contains("set -g window-status-current-format"),
        "{after}"
    );
}

#[test]
fn a_default_that_cannot_be_quoted_is_handed_back_for_either_shape() {
    // Both the config that assigns neither option and the one that assigns
    // only one fall back on this tmux's own default, and both must hand it
    // back rather than write a line tmux would discard.
    for existing in [
        "set -g status on\n",
        "set -g window-status-format '#I:#W'\n",
    ] {
        let home = TempDir::new("cli-odd-default");
        fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
        let config = home.join(".config/tmux/tmux.conf");
        fs::write(&config, existing).expect("the config");

        let out = command(&home, &["-y", "--tmux-format"])
            .env("TMUX_AGENT_STATUS_TEST_FAULT", "odd-default")
            .output()
            .expect("the binary runs");

        assert_eq!(code(&out), 0, "{}", stdout(&out));
        assert!(
            stdout(&out).contains("cannot be quoted safely"),
            "{existing:?}: {}",
            stdout(&out)
        );
    }
}

#[test]
fn a_config_we_cannot_write_is_handed_back_whichever_shape_it_has() {
    // The same refusal has to reach both ways of planning a format edit: the
    // one that splices a line, and the one that adds a line for an option
    // nothing assigns.
    for existing in [
        "set -g window-status-format '#I:#W'\n",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    ] {
        let home = TempDir::new("cli-unwritable-shapes");
        let dir = home.join(".config/tmux");
        fs::create_dir_all(&dir).expect("the directory");
        fs::write(dir.join("tmux.conf"), existing).expect("the config");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("chmod");

        let out = register(&home, &["-y", "--tmux-format"]);

        assert_eq!(code(&out), 0, "{}", stdout(&out));
        assert!(
            stdout(&out).contains("not writable"),
            "{existing:?}: {}",
            stdout(&out)
        );
    }
}

/// The stages of the fault switch that stand for something no test can
/// arrange: a tmux that stops answering, an edit it parses but that moves
/// something else, a default it cannot quote, a file that moves under us, a
/// reload it refuses.
#[test]
fn every_fault_stage_reaches_the_branch_it_stands_for() {
    for (stage, expected, in_config) in [
        // tmux stopped answering between the write and the check: the edit
        // stands, and the summary says it was not checked.
        ("no-dump", 0, true),
        // An edit tmux parses that moves something else: rolled back.
        ("bad-value", 1, false),
        // A default that cannot be quoted: handed back, nothing written.
        ("odd-default", 0, false),
        // The file moved between the plan and the write: nothing written.
        ("rebuild", 1, false),
        // The reload was refused: the edit stands and the run says so.
        ("bad-reload", 0, true),
    ] {
        let home = TempDir::new(&format!("cli-fault-{stage}"));
        fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
        let config = home.join(".config/tmux/tmux.conf");
        fs::write(&config, "set -g status-left 'LEFT'\n").expect("the config");

        let out = command(&home, &["-y", "--no-agents"])
            .env("TMUX_AGENT_STATUS_TEST_FAULT", stage)
            .output()
            .expect("the binary runs");

        assert_eq!(code(&out), expected, "{stage}: {}", stdout(&out));
        let written = fs::read_to_string(&config).expect("the config");
        assert_eq!(
            written.contains("@agent_status"),
            in_config,
            "{stage}: the config came out wrong:\n{written}"
        );
    }
}

#[test]
fn a_flag_given_without_its_value_is_a_usage_error_naming_it() {
    for flag in [
        "--agents",
        "--claude-route",
        "--marketplace",
        "--tmux-config",
        "--snippet",
    ] {
        let home = TempDir::new("cli-no-value");
        // The flag last, so there is nothing for it to take.
        let out = register(&home, &["--dry-run", flag]);

        match flag {
            // A bare `--agents` is the documented way to say "only the
            // agents", so it is the one flag that means something on its own.
            "--agents" => assert_eq!(code(&out), 0, "{}", stderr(&out)),
            _ => {
                assert_eq!(code(&out), 2, "{flag}: {}", stdout(&out));
                assert!(
                    stderr(&out).contains(flag),
                    "{flag} is not named: {}",
                    stderr(&out)
                );
            }
        }
    }
}

#[test]
fn the_agents_flag_given_twice_is_a_usage_error() {
    // The bare form is taken first, so the second one is left for the value
    // lookup with nothing to take.
    let home = TempDir::new("cli-agents-twice");
    let out = register(&home, &["--dry-run", "--agents", "--agents"]);

    assert_eq!(code(&out), 2, "{}", stdout(&out));
    assert!(stderr(&out).contains("--agents"), "{}", stderr(&out));
}

/// `$PREFIX` set to nothing has to mean what leaving it unset means. Without
/// the filter at the edge, an empty value becomes a root of `/share`, which is
/// not a search path anyone asked for.
#[test]
fn an_empty_prefix_runs_exactly_as_an_unset_one_does() {
    let home = TempDir::new("cli-empty-prefix");
    let empty = command(&home, &["--dry-run", "--tmux-hook"])
        .env("PREFIX", "")
        .output()
        .expect("the binary runs");
    let unset = command(&home, &["--dry-run", "--tmux-hook"])
        .env_remove("PREFIX")
        .output()
        .expect("the binary runs");

    assert_eq!(code(&empty), 0, "{}", stderr(&empty));
    assert_eq!(stdout(&empty), stdout(&unset));
    assert_eq!(stderr(&empty), stderr(&unset));
}

#[test]
fn a_stray_argument_is_a_usage_error() {
    let home = TempDir::new("cli-stray");
    let out = register(&home, &["--dry-run", "nonsense"]);

    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("nonsense"), "{}", stderr(&out));
}

#[test]
fn with_no_home_there_is_nowhere_to_register_to() {
    let home = TempDir::new("cli-no-home");
    let out = command(&home, &["--dry-run"])
        .env_remove("HOME")
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 2, "{}", stdout(&out));
    assert!(stderr(&out).contains("$HOME"), "{}", stderr(&out));
}

#[test]
fn an_xdg_config_home_elsewhere_is_where_the_config_goes() {
    let home = TempDir::new("cli-xdg");
    let elsewhere = home.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("the directory");

    let out = command(&home, &["-y", "--tmux-hook"])
        .env("XDG_CONFIG_HOME", &elsewhere)
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(
        elsewhere.join("tmux/tmux.conf").is_file(),
        "the config did not follow $XDG_CONFIG_HOME: {}",
        stdout(&out)
    );
    assert!(!home.join(".config/tmux/tmux.conf").exists());
}

#[test]
fn a_file_we_cannot_read_is_reported_rather_than_guessed_at() {
    let home = TempDir::new("cli-unreadable");
    fs::create_dir_all(home.join(".codex")).expect("the directory");
    let target = home.join(".codex/hooks.json");
    fs::write(&target, "{}\n").expect("the config");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).expect("chmod");

    let out = register(&home, &["-y", "--agents=codex"]);

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(stdout(&out).contains("not registered"), "{}", stdout(&out));
}

#[test]
fn with_no_tmux_an_existing_config_is_still_edited_unchecked() {
    let home = TempDir::new("cli-no-tmux-config");
    let empty = home.join("bin");
    fs::create_dir_all(&empty).expect("the bin directory");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    let config = home.join(".config/tmux/tmux.conf");
    fs::write(&config, "set -g window-status-format '#I:#W'\n").expect("the config");

    let out = command(&home, &["-y", "--no-agents"])
        .env("PATH", only(&empty))
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let written = fs::read_to_string(&config).expect("the config");
    assert!(written.contains("@agent_status"), "{written}");
}

#[test]
fn a_read_that_fails_after_the_write_is_reported() {
    let home = TempDir::new("cli-read-back");
    fs::create_dir_all(home.join(".codex")).expect("the directory");
    let target = home.join(".codex/hooks.json");
    fs::write(&target, "{\"theirs\": 1}\n").expect("the config");

    let out = command(&home, &["-y", "--agents=codex"])
        .env("TMUX_AGENT_STATUS_TEST_FAULT", "read-back")
        .output()
        .expect("the binary runs");

    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert!(stdout(&out).contains("read-back"), "{}", stdout(&out));
}

#[test]
fn the_help_text_documents_the_subcommand_and_its_flags() {
    let out = Command::new(support::BIN)
        .arg("--help")
        .output()
        .expect("the binary runs");
    let text = stdout(&out);

    assert!(text.contains("tmux-agent-status register"), "{text}");
    for flag in [
        "--agents",
        "--tmux-hook",
        "--tmux-format",
        "--no-agents",
        "--dry-run",
        "--tmux-config",
        "--snippet",
        "--claude-route",
        "--marketplace",
        "--no-tmux-probe",
    ] {
        assert!(text.contains(flag), "{flag} is not documented: {text}");
    }
    // Every valid `--agents=` name is listed, because the error that rejects a
    // typo points at this list.
    for name in ["codex", "cursor", "mistral-vibe", "claude-code"] {
        assert!(text.contains(name), "{name} is not listed: {text}");
    }
}

// A format line spread over continuations is still a line the splice can
// rewrite. Matching the joined logical line against one physical line never
// succeeded, so the step failed with "the config moved under us" on a file
// nothing had touched - and when only one of the two options wrapped, the run
// left the term in exactly one of them, which is the arrangement that makes
// the glyph vanish the moment the window becomes current.
#[test]
fn a_format_line_split_over_continuations_is_spliced_like_any_other() {
    let home = TempDir::new("cli-continuation");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    let config = home.join(".config/tmux/tmux.conf");
    fs::write(
        &config,
        "set -g status on\n\
         set -g window-status-format \\\n  '#I:#W'\n\
         set -g window-status-current-format '#I:#W'\n",
    )
    .expect("the config");

    let out = register(&home, &["-y", "--tmux-format"]);

    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    assert!(!stdout(&out).contains("moved under us"), "{}", stdout(&out));
    let after = fs::read_to_string(&config).expect("the config");
    // Both options, because a term in only one of them is the arrangement this
    // whole step exists to avoid.
    for option in ["window-status-format", "window-status-current-format"] {
        let assigned = after
            .lines()
            .find(|line| line.contains(&format!("set -g {option} ")))
            .unwrap_or_else(|| panic!("{option} is not assigned:\n{after}"));
        assert!(
            assigned.contains("#{?@agent_status, #{@agent_status},}"),
            "{option} has no term:\n{after}"
        );
    }
    // The run collapses the continuation into the one line it wrote, which is
    // the formatting change the confirmation discloses.
    assert!(!after.contains('\\'), "a continuation survived:\n{after}");
}

// A relative `source-file` is tmux's to resolve against the directory the
// server was started in, which is `$HOME` for a server started from a login
// shell and the cwd the probe runs with. Resolving it from beside the config
// instead picked a winner out of a file tmux never read, and then spliced the
// term into a line tmux discards.
#[test]
fn a_relative_source_is_followed_from_home_and_the_guess_is_reported() {
    let home = TempDir::new("cli-relative-source");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    let config = home.join(".config/tmux/tmux.conf");
    fs::write(
        &config,
        "set -g window-status-format '#I:#W'\n\
         set -g window-status-current-format '#I:#W'\n\
         source-file fragment.conf\n",
    )
    .expect("the config");
    // Beside the config as well as in $HOME, so that reading the wrong one is
    // a failure rather than an accident of which file exists.
    let beside = home.join(".config/tmux/fragment.conf");
    fs::write(&beside, "set -g window-status-format '#I:#W-beside'\n").expect("the decoy");
    let from_home = home.join("fragment.conf");
    fs::write(&from_home, "set -g window-status-format '#I:#W-home'\n").expect("the fragment");

    let out = register(&home, &["-y", "--tmux-format"]);

    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    // The guess is disclosed before anything is written.
    assert!(stdout(&out).contains("relative path"), "{}", stdout(&out));
    // The winning assignment is the one in the fragment tmux would have read,
    // and it is the one that gained the term.
    let edited = fs::read_to_string(&from_home).expect("the fragment");
    assert!(
        edited.contains("#{?@agent_status, #{@agent_status},}"),
        "the fragment $HOME holds was not the one edited:\n{edited}"
    );
    assert_eq!(
        fs::read_to_string(&beside).expect("the decoy"),
        "set -g window-status-format '#I:#W-beside'\n",
        "the fragment beside the config was edited"
    );
}

// A dry run that cannot plan a step still exits 0. Nothing ran, so nothing
// failed, and exit 1 told a caller that a file had been touched and put back -
// the one thing a dry run certainly did not do.
#[test]
fn a_dry_run_exits_zero_even_when_a_step_cannot_be_planned() {
    let home = TempDir::new("cli-dry-run-unplannable");
    let before = home.entries();

    // The plugin route asked for by name, with no `claude` to take it.
    let out = command(
        &home,
        &["--dry-run", "--agents=claude-code", "--claude-route=plugin"],
    )
    .env("PATH", only(&home.join("nothing")))
    .output()
    .expect("the binary runs");

    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    // The step it could not plan is still part of the plan it printed.
    assert!(text.contains("is not on PATH"), "{text}");
    assert!(
        text.contains("--dry-run: nothing above was written."),
        "{text}"
    );
    assert_eq!(home.entries(), before, "a dry run created something");
}

// A snippet path holding both a `'` and something double quotes would expand
// cannot be written into a `source-file` line at all: a stray second argument
// or an unterminated quote makes tmux abandon the whole config file. Handed
// back before anything is written, which leaves the user where they started.
#[test]
fn a_snippet_path_no_quoting_can_carry_is_handed_back() {
    let home = TempDir::new("cli-unspellable-snippet");
    fs::create_dir_all(home.join(".config/tmux")).expect("the directory");
    let config = home.join(".config/tmux/tmux.conf");
    fs::write(&config, "set -g status on\n").expect("the config");
    let snippet = home.join("it's $HOME/tmux-agent-status.conf");

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-hook",
            "--snippet",
            &snippet.display().to_string(),
        ],
    );

    // Not a failure: a refusal the user can act on leaves them no worse off.
    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("cannot be written into a tmux config line"),
        "{text}"
    );
    assert!(text.contains("--snippet"), "{text}");
    assert_eq!(
        fs::read_to_string(&config).expect("the config"),
        "set -g status on\n",
        "the config was edited anyway"
    );
}
