//! `install` as a user runs it: a real process, a real `$HOME`, real files.
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
fn install(home: &TempDir, args: &[&str]) -> Output {
    command(home, args).output().expect("the binary runs")
}

fn command(home: &TempDir, args: &[&str]) -> Command {
    let mut cmd = Command::new(support::BIN);
    cmd.arg("install")
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
    let out = install(&home, &[]);

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

    let out = install(&home, &["--dry-run", "--agents=codex"]);

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("This is what install would do:"), "{text}");
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

    let out = install(&home, &["--dry-run", "--no-agents"]);

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
    let out = install(&home, &["--dry-run", "--agents=cursur"]);

    assert_eq!(code(&out), 2);
    let message = stderr(&out);
    assert!(message.contains("unknown agent `cursur`"), "{message}");
    assert!(message.contains("cursor"), "{message}");
}

#[test]
fn mixing_positive_and_negative_step_flags_is_a_usage_error() {
    let home = TempDir::new("cli-mixed-steps");
    let out = install(&home, &["--dry-run", "--agents=codex", "--no-tmux-hook"]);

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
    let out = install(&home, &["--dry-run", "--claude-route=sideways"]);

    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("valid routes are"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_real_run_installs_an_agent_and_a_second_run_writes_nothing() {
    let home = TempDir::new("cli-idempotent");
    let target = home.join(".codex/hooks.json");

    let first = install(&home, &["-y", "--agents=codex"]);
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
    let second = install(&home, &["-y", "--agents=codex"]);
    assert_eq!(code(&second), 0, "{}", stderr(&second));
    assert!(
        stdout(&second).contains("already installed"),
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

    let out = install(&home, &["-y", "--agents=codex"]);

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
        stdout(&out).contains("already installed"),
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

// 22. No tmux on `PATH`: everything degrades, the run still installs what it
// can, and the summary says what could not be checked.
#[test]
fn with_no_tmux_at_all_the_run_still_installs_what_it_can() {
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
    // Installing the config before installing tmux is a legitimate order to do
    // things in, so the config is still written - and the run says plainly
    // that nothing could be checked against a live tmux.
    assert!(
        home.join(".config/tmux/tmux.conf").is_file(),
        "the tmux config was not created: {text}"
    );
    assert!(
        text.contains("not be checked against a live tmux"),
        "the run did not say what it could not check: {text}"
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

    let out = install(&home, &["-y", "--agents=codex"]);

    // A refusal the user can act on is not a failure: the step reports "not
    // installed, here is what to do" and the run exits 0.
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("not installed"), "{text}");
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

// 20. Exit codes for a mixed run: one target already installed, one applied,
// one failed. The run does what it can and says exactly what it did not.
#[test]
fn a_mixed_run_finishes_the_work_it_can_and_exits_one() {
    let home = TempDir::new("cli-mixed-outcomes");
    // Already installed: a Kiro file byte-identical to the shipped one.
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
    assert!(text.contains("already installed"), "{text}");
    // The step that could be done was not abandoned because another failed.
    assert!(
        home.join(".grok/hooks/tmux-agent-status.json").is_file(),
        "the work that could be finished was not: {text}"
    );
    assert_eq!(
        fs::read_to_string(&kiro).expect("the file"),
        shipped,
        "an already-installed file was rewritten"
    );
    // And the failure is loud, retryable, and names the way out.
    assert!(text.contains("--claude-route=settings"), "{text}");
    assert!(!home.join(".claude/settings.json").exists(), "{text}");
}

#[test]
fn the_help_text_documents_the_subcommand_and_its_flags() {
    let out = Command::new(support::BIN)
        .arg("--help")
        .output()
        .expect("the binary runs");
    let text = stdout(&out);

    assert!(text.contains("tmux-agent-status install"), "{text}");
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
