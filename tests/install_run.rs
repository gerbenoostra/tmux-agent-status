//! The run itself, driven by a scripted person.
//!
//! `install` asks questions, and most of what it does next turns on the
//! answers. Driving it through a real terminal would test `dialoguer`; driving
//! it with `-y` only ever answers yes. So these supply an `Interaction` of
//! their own and assert what the run does when somebody says no, edits the
//! proposed line, or picks a different set of agents.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tmux_agent_status::install::prompt::Interaction;
use tmux_agent_status::install::write::{Ask, Warning};
use tmux_agent_status::install::{self, Home, Options, Outcome, Step, agents};

mod support;

use support::tempdir::TempDir;

/// Somebody at the keyboard, with their mind already made up.
struct Script {
    /// What every `confirm` gets back. `None` takes the recommendation.
    answer: Option<bool>,
    /// What `edit` hands back, if anything.
    edited: Option<String>,
    /// Which rows `choose` picks. `None` takes the preselected ones.
    chosen: Option<Vec<usize>>,
    said: Mutex<Vec<String>>,
    asked: Mutex<Vec<String>>,
}

impl Script {
    fn saying_yes() -> Script {
        Script {
            answer: Some(true),
            edited: None,
            chosen: None,
            said: Mutex::new(Vec::new()),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn saying_no() -> Script {
        Script {
            answer: Some(false),
            ..Script::saying_yes()
        }
    }

    fn output(&self) -> String {
        self.said.lock().expect("not poisoned").join("\n")
    }

    fn questions(&self) -> String {
        self.asked.lock().expect("not poisoned").join("\n")
    }
}

impl Interaction for Script {
    fn say(&self, line: &str) {
        self.said
            .lock()
            .expect("not poisoned")
            .push(line.to_owned());
    }

    fn confirm(&self, question: &str, recommended: bool) -> bool {
        self.asked
            .lock()
            .expect("not poisoned")
            .push(question.to_owned());
        self.answer.unwrap_or(recommended)
    }

    fn choose(&self, question: &str, rows: &[(String, bool)]) -> Vec<usize> {
        self.asked
            .lock()
            .expect("not poisoned")
            .push(question.to_owned());
        for (label, _) in rows {
            self.say(label);
        }
        self.chosen.clone().unwrap_or_else(|| {
            rows.iter()
                .enumerate()
                .filter(|(_, (_, on))| *on)
                .map(|(index, _)| index)
                .collect()
        })
    }

    fn edit(&self, _: &str) -> Option<String> {
        self.edited.clone()
    }

    fn is_dry_run(&self) -> bool {
        false
    }
}

impl Ask for Script {
    fn warn(&self, warning: &Warning) -> bool {
        self.say(&warning.message());
        self.confirm("Go ahead anyway?", true)
    }
}

fn options(home: &TempDir, steps: install::Steps) -> Options {
    Options {
        steps,
        agents: None,
        claude_route: agents::ClaudeRoute::Settings,
        marketplace: None,
        tmux_config: None,
        snippet: None,
        probe: true,
        home: Home {
            home: home.path().to_path_buf(),
            xdg_config: None,
        },
        exe: None,
    }
}

fn only_agents(home: &TempDir, names: &[&str]) -> Options {
    Options {
        agents: Some(names.iter().map(|name| agent(name)).collect()),
        ..options(
            home,
            install::select(&[Step::Agents], &[]).expect("a valid selection"),
        )
    }
}

fn agent(name: &str) -> &'static agents::Agent {
    agents::by_name(name).expect("the agent is in the table")
}

/// Both tmux steps, and nothing else.
fn tmux_steps() -> install::Steps {
    install::select(&[Step::TmuxHook, Step::TmuxFormat], &[]).expect("a valid selection")
}

#[test]
fn saying_no_to_a_write_leaves_the_file_alone_and_is_not_a_failure() {
    let dir = TempDir::new("run-declined");
    fs::create_dir_all(dir.join(".codex")).expect("the directory");
    let target = dir.write(".codex/hooks.json", "{\"theirs\": 1}\n");
    let script = Script::saying_no();

    let report = install::run(&only_agents(&dir, &["codex"]), &script);

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::NotInstalled));
    // A refusal the user chose is not an error.
    assert_eq!(report.exit_code(), 0);
    assert_eq!(
        fs::read_to_string(&target).expect("the file"),
        "{\"theirs\": 1}\n"
    );
    assert!(
        script.questions().contains("Write "),
        "nobody was asked: {}",
        script.questions()
    );
}

#[test]
fn saying_yes_installs_and_says_where_it_landed() {
    let dir = TempDir::new("run-accepted");
    let script = Script::saying_yes();

    let report = install::run(&only_agents(&dir, &["kiro"]), &script);

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::Installed));
    assert_eq!(report.exit_code(), 0);
    assert!(dir.join(".kiro/hooks/tmux-agent-status.json").is_file());
    assert!(script.output().contains("created"), "{}", script.output());
}

#[test]
fn the_agent_list_shows_every_agent_and_installs_what_was_ticked() {
    let dir = TempDir::new("run-choose");
    // Nothing is detected in this home, so nothing is preselected - and the
    // list still shows all of them, because an agent is never installed
    // without appearing in it.
    let script = Script {
        chosen: Some(vec![
            agents::AGENTS
                .iter()
                .position(|agent| agent.name == "kiro")
                .expect("kiro is in the table"),
        ]),
        ..Script::saying_yes()
    };

    let report = install::run(
        &options(&dir, install::select(&[Step::Agents], &[]).expect("valid")),
        &script,
    );

    assert_eq!(report.exit_code(), 0);
    for agent in agents::AGENTS {
        assert!(
            script.output().contains(agent.label),
            "{} was not listed: {}",
            agent.label,
            script.output()
        );
    }
    assert!(dir.join(".kiro/hooks/tmux-agent-status.json").is_file());
    // And nothing that was not ticked.
    assert!(!dir.join(".codex/hooks.json").exists());
    assert!(!dir.join(".grok/hooks/tmux-agent-status.json").exists());
}

#[test]
fn choosing_nothing_installs_nothing() {
    let dir = TempDir::new("run-choose-none");
    let script = Script {
        chosen: Some(Vec::new()),
        ..Script::saying_yes()
    };

    let report = install::run(
        &options(&dir, install::select(&[Step::Agents], &[]).expect("valid")),
        &script,
    );

    assert_eq!(report.exit_code(), 0);
    assert_eq!(report.outcome(Step::Agents), None);
    assert!(dir.entries().is_empty(), "{:?}", dir.entries());
}

#[test]
fn a_target_that_resolves_elsewhere_says_so_before_it_is_written() {
    let dir = TempDir::new("run-symlink");
    // The `mkOutOfStoreSymlink` shape: the path the user knows is a link into
    // a checkout they maintain, and the edit lands in the checkout.
    let checkout = dir.write("dotfiles/hooks.json", "{}\n");
    fs::create_dir_all(dir.join(".codex")).expect("the directory");
    std::os::unix::fs::symlink(&checkout, dir.join(".codex/hooks.json")).expect("the link");
    let script = Script::saying_yes();

    install::run(&only_agents(&dir, &["codex"]), &script);

    let output = script.output();
    assert!(output.contains("resolves to"), "{output}");
    assert!(output.contains("dotfiles/hooks.json"), "{output}");
    assert!(
        fs::symlink_metadata(dir.join(".codex/hooks.json"))
            .expect("the link")
            .is_symlink(),
        "the link was replaced"
    );
    assert!(
        fs::read_to_string(&checkout)
            .expect("the file")
            .contains("tmux-agent-status")
    );
}

#[test]
fn a_target_inside_a_git_repository_is_pointed_out() {
    // For a user whose other agent configs are versioned, an unmanaged write is
    // state that quietly does not exist on their next machine. Reporting, never
    // policy.
    let dir = TempDir::new("run-git");
    fs::create_dir_all(dir.join(".codex/.git")).expect("the directory");
    let script = Script::saying_yes();

    install::run(&only_agents(&dir, &["codex"]), &script);

    assert!(
        script.output().contains("git repository"),
        "{}",
        script.output()
    );
}

#[test]
fn devins_project_file_is_mentioned_and_never_written() {
    let dir = TempDir::new("run-devin");
    let script = Script::saying_yes();

    install::run(&only_agents(&dir, &["devin"]), &script);

    assert!(
        script.output().contains("project scope"),
        "{}",
        script.output()
    );
    assert!(dir.join(".config/devin/config.json").is_file());
    assert!(!dir.join(".devin").exists(), "a project file was written");
}

#[test]
fn hooks_already_there_without_markers_are_adopted_rather_than_repeated() {
    let dir = TempDir::new("run-adopt");
    // Verified as a real setup: a user who copied the shipped drop-in by hand
    // before this subcommand existed.
    let vibe = agent("mistral-vibe");
    let target = dir.write(".vibe/hooks.toml", vibe.contents);
    let script = Script::saying_yes();

    let report = install::run(&only_agents(&dir, &["mistral-vibe"]), &script);

    assert_eq!(
        report.outcome(Step::Agents),
        Some(Outcome::AlreadyInstalled)
    );
    assert_eq!(
        fs::read_to_string(&target).expect("the file"),
        vibe.contents,
        "an already-installed file was rewritten"
    );
}

#[test]
fn a_file_that_cannot_be_merged_into_hands_the_block_back() {
    let dir = TempDir::new("run-manual");
    let target = dir.write(".codex/hooks.json", "not json at all\n");
    let script = Script::saying_yes();

    let report = install::run(&only_agents(&dir, &["codex"]), &script);

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::NotInstalled));
    assert_eq!(report.exit_code(), 0);
    let output = script.output();
    assert!(output.contains("by hand"), "{output}");
    assert!(output.contains("tmux-agent-status"), "{output}");
    assert_eq!(
        fs::read_to_string(&target).expect("the file"),
        "not json at all\n"
    );
}

#[test]
fn a_config_tmux_will_not_read_is_refused_before_anything_is_written() {
    let dir = TempDir::new("run-pre-broken");
    let config = dir.write(
        ".config/tmux/tmux.conf",
        "set -g status-left 'LEFT' stray-argument\n",
    );
    let before = fs::read_to_string(&config).expect("the config");
    let script = Script::saying_yes();

    let report = install::run(&options(&dir, tmux_steps()), &script);

    let output = script.output();
    assert_eq!(report.outcome(Step::TmuxHook), Some(Outcome::NotInstalled));
    assert_eq!(
        report.outcome(Step::TmuxFormat),
        Some(Outcome::NotInstalled)
    );
    assert_eq!(report.exit_code(), 0);
    assert!(output.contains("tmux will not read"), "{output}");
    assert!(output.contains("Fixing that comes first"), "{output}");
    assert_eq!(fs::read_to_string(&config).expect("the config"), before);
}

#[test]
fn a_line_the_parser_will_not_touch_is_handed_back_with_the_term() {
    let dir = TempDir::new("run-refused-line");
    // A second command on the line: tmux reads it, we will not rewrite it.
    let config = dir.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format 'x' ; set -g status on\n\
         set -g window-status-current-format 'y'\n",
    );
    let before = fs::read_to_string(&config).expect("the config");
    let script = Script::saying_no();

    let report = install::run(
        &options(
            &dir,
            install::select(&[Step::TmuxFormat], &[]).expect("valid"),
        ),
        &script,
    );

    let output = script.output();
    assert_eq!(
        report.outcome(Step::TmuxFormat),
        Some(Outcome::NotInstalled)
    );
    assert!(output.contains("second command"), "{output}");
    // The term is printed, ready to paste, which is the whole point of having
    // a manual path.
    assert!(
        output.contains("#{?@agent_status, #{@agent_status},}"),
        "{output}"
    );
    assert_eq!(fs::read_to_string(&config).expect("the config"), before);
}

#[test]
fn an_option_nothing_assigns_gets_a_line_of_its_own() {
    let dir = TempDir::new("run-one-line");
    // Only one of the two is set; the other is on tmux's default, and a term in
    // only one of them makes the glyph vanish the moment the window is current.
    let config = dir.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\n",
    );
    let script = Script::saying_yes();

    let report = install::run(
        &options(
            &dir,
            install::select(&[Step::TmuxFormat], &[]).expect("valid"),
        ),
        &script,
    );

    assert_eq!(report.exit_code(), 0);
    let written = fs::read_to_string(&config).expect("the config");
    assert!(
        written.contains("set -g window-status-format '#I:#W#{?@agent_status"),
        "the assigned option was not spliced:\n{written}"
    );
    assert!(
        written.contains("set -g window-status-current-format"),
        "the unassigned option got no line:\n{written}"
    );
    assert!(
        script.output().contains("nothing assigns"),
        "{}",
        script.output()
    );
}

#[test]
fn an_edited_line_is_taken_when_it_still_carries_the_term() {
    let dir = TempDir::new("run-edit");
    let config = dir.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    let script = Script {
        edited: Some(
            "set -g window-status-format 'EDITED#{?@agent_status, #{@agent_status},}'".to_owned(),
        ),
        ..Script::saying_yes()
    };

    install::run(
        &options(
            &dir,
            install::select(&[Step::TmuxFormat], &[]).expect("valid"),
        ),
        &script,
    );

    let written = fs::read_to_string(&config).expect("the config");
    assert!(
        written.contains("EDITED"),
        "the edit was discarded:\n{written}"
    );
}

#[test]
fn an_edit_that_drops_the_term_is_refused_and_the_proposal_stands() {
    let dir = TempDir::new("run-edit-bad");
    let config = dir.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    let script = Script {
        edited: Some("set -g window-status-format 'NO TERM HERE'".to_owned()),
        ..Script::saying_yes()
    };

    install::run(
        &options(
            &dir,
            install::select(&[Step::TmuxFormat], &[]).expect("valid"),
        ),
        &script,
    );

    let written = fs::read_to_string(&config).expect("the config");
    assert!(!written.contains("NO TERM HERE"), "{written}");
    assert!(written.contains("@agent_status"), "{written}");
    assert!(
        script.output().contains("no longer references"),
        "{}",
        script.output()
    );
}

#[test]
fn a_run_with_no_steps_does_nothing_at_all() {
    let dir = TempDir::new("run-nothing");
    let script = Script::saying_yes();

    let report = install::run(
        &options(
            &dir,
            install::Steps {
                agents: false,
                tmux_hook: false,
                tmux_format: false,
            },
        ),
        &script,
    );

    assert_eq!(report.exit_code(), 0);
    assert!(dir.entries().is_empty(), "{:?}", dir.entries());
}

#[test]
fn the_snippet_is_written_when_none_is_installed_and_then_sourced() {
    let dir = TempDir::new("run-snippet");
    dir.write(".config/tmux/tmux.conf", "set -g status on\n");
    let script = Script::saying_yes();

    let report = install::run(
        &Options {
            snippet: Some(dir.join(".config/tmux/tmux-agent-status.conf")),
            ..options(
                &dir,
                install::select(&[Step::TmuxHook], &[]).expect("valid"),
            )
        },
        &script,
    );

    assert_eq!(report.exit_code(), 0);
    let snippet = dir.join(".config/tmux/tmux-agent-status.conf");
    assert!(snippet.is_file(), "{}", script.output());
    assert!(
        fs::read_to_string(&snippet)
            .expect("the snippet")
            .contains("set-hook -g 'session-window-changed[50]'")
    );
    assert!(
        fs::read_to_string(dir.join(".config/tmux/tmux.conf"))
            .expect("the config")
            .contains("source-file")
    );
}

#[test]
fn a_config_that_does_not_exist_yet_is_created_where_tmux_documents() {
    let dir = TempDir::new("run-create-config");
    let script = Script::saying_yes();

    let report = install::run(&options(&dir, tmux_steps()), &script);

    assert_eq!(report.exit_code(), 0);
    let config = dir.join(".config/tmux/tmux.conf");
    assert!(config.is_file(), "{}", script.output());
    let written = fs::read_to_string(&config).expect("the config");
    assert!(written.contains("@agent_status"), "{written}");
    assert!(written.contains("source-file"), "{written}");
    // The hook step created the file, so it took no backup; the format step
    // then edited what the hook step had left, so it took exactly one.
    let backups: Vec<String> = leftovers(dir.path())
        .into_iter()
        .filter(|name| name.contains(".bak-"))
        .collect();
    assert_eq!(backups.len(), 1, "{backups:?}");
}

fn leftovers(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            found.extend(leftovers(&path));
        } else {
            found.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found
}
