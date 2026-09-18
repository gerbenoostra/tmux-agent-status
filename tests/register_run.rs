//! The run itself, driven by a scripted person.
//!
//! `register` asks questions, and most of what it does next turns on the
//! answers. Driving it through a real terminal would test `dialoguer`; driving
//! it with `-y` only ever answers yes. So these supply an `Interaction` of
//! their own and assert what the run does when somebody says no, edits the
//! proposed line, or picks a different set of agents.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tmux_agent_status::register::prompt::Interaction;
use tmux_agent_status::register::write::{Ask, Warning};
use tmux_agent_status::register::{self, Home, Options, Outcome, Step, agents};

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
    /// A question holding this gets `false` whatever `answer` says, for the
    /// runs where somebody accepts one write and declines another, or turns
    /// the proposed line down to get at the manual edit.
    declining: Option<String>,
    /// Everything said and asked (via `confirm` or `choose`), in the order it
    /// happened. `output()`, `questions()` and `log()` are views over this,
    /// so there is one place events get recorded.
    events: Mutex<Vec<Event>>,
}

/// One thing the run said or asked, tagged by which it was rather than by a
/// string prefix - a said line that happened to start with "ASK: " cannot be
/// misfiled as a question.
enum Event {
    Said(String),
    Asked(String),
}

impl Event {
    fn line(&self) -> String {
        match self {
            Event::Said(line) => format!("SAY: {line}"),
            Event::Asked(question) => format!("ASK: {question}"),
        }
    }
}

impl Script {
    fn saying_yes() -> Script {
        Script {
            answer: Some(true),
            edited: None,
            chosen: None,
            declining: None,
            events: Mutex::new(Vec::new()),
        }
    }

    /// Yes to everything but the one question naming `what`.
    fn saying_yes_but_not_to(what: &str) -> Script {
        Script {
            declining: Some(what.to_owned()),
            ..Script::saying_yes()
        }
    }

    fn saying_no() -> Script {
        Script {
            answer: Some(false),
            ..Script::saying_yes()
        }
    }

    fn output(&self) -> String {
        self.events
            .lock()
            .expect("not poisoned")
            .iter()
            .filter_map(|event| match event {
                Event::Said(line) => Some(line.clone()),
                Event::Asked(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn questions(&self) -> String {
        self.events
            .lock()
            .expect("not poisoned")
            .iter()
            .filter_map(|event| match event {
                Event::Asked(question) => Some(question.clone()),
                Event::Said(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Everything said and asked, in order, as `SAY: `/`ASK: ` lines - for
    /// tests that care which came first rather than just what was said.
    fn log(&self) -> String {
        self.events
            .lock()
            .expect("not poisoned")
            .iter()
            .map(Event::line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Interaction for Script {
    fn as_ask(&self) -> &dyn Ask {
        self
    }

    fn say(&self, line: &str) {
        self.events
            .lock()
            .expect("not poisoned")
            .push(Event::Said(line.to_owned()));
    }

    fn confirm(&self, question: &str, recommended: bool) -> bool {
        self.events
            .lock()
            .expect("not poisoned")
            .push(Event::Asked(question.to_owned()));
        match &self.declining {
            Some(what) if question.contains(what.as_str()) => false,
            _ => self.answer.unwrap_or(recommended),
        }
    }

    fn choose(&self, question: &str, rows: &[(String, bool)]) -> Vec<usize> {
        self.events
            .lock()
            .expect("not poisoned")
            .push(Event::Asked(question.to_owned()));
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

fn options(home: &TempDir, steps: register::Steps) -> Options {
    Options {
        steps,
        agents: None,
        claude_route: agents::ClaudeRoute::Settings,
        claude: None,
        marketplace: None,
        tmux_config: None,
        snippet: None,
        probe: true,
        home: Home {
            home: home.path().to_path_buf(),
            xdg_config: None,
        },
        exe: None,
        prefix: None,
    }
}

fn only_agents(home: &TempDir, names: &[&str]) -> Options {
    Options {
        agents: Some(names.iter().map(|name| agent(name)).collect()),
        ..options(
            home,
            register::select(&[Step::Agents], &[]).expect("a valid selection"),
        )
    }
}

fn agent(name: &str) -> &'static agents::Agent {
    agents::by_name(name).expect("the agent is in the table")
}

/// Both tmux steps, and nothing else.
fn tmux_steps() -> register::Steps {
    register::select(&[Step::TmuxHook, Step::TmuxFormat], &[]).expect("a valid selection")
}

#[test]
fn saying_no_to_a_write_leaves_the_file_alone_and_is_not_a_failure() {
    let dir = TempDir::new("run-declined");
    fs::create_dir_all(dir.join(".codex")).expect("the directory");
    let target = dir.write(".codex/hooks.json", "{\"theirs\": 1}\n");
    let script = Script::saying_no();

    let report = register::run(&only_agents(&dir, &["codex"]), &script);

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::NotRegistered));
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
fn saying_yes_registers_and_says_where_it_landed() {
    let dir = TempDir::new("run-accepted");
    let script = Script::saying_yes();

    let report = register::run(&only_agents(&dir, &["kiro"]), &script);

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::Registered));
    assert_eq!(report.exit_code(), 0);
    assert!(dir.join(".kiro/hooks/tmux-agent-status.json").is_file());
    assert!(script.output().contains("created"), "{}", script.output());
}

#[test]
fn the_agent_list_shows_every_agent_and_registers_what_was_ticked() {
    let dir = TempDir::new("run-choose");
    // Nothing is detected in this home, so nothing is preselected - and the
    // list still shows all of them, because an agent is never registered
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

    let report = register::run(
        &options(&dir, register::select(&[Step::Agents], &[]).expect("valid")),
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
fn choosing_nothing_registers_nothing() {
    let dir = TempDir::new("run-choose-none");
    let script = Script {
        chosen: Some(Vec::new()),
        ..Script::saying_yes()
    };

    let report = register::run(
        &options(&dir, register::select(&[Step::Agents], &[]).expect("valid")),
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

    register::run(&only_agents(&dir, &["codex"]), &script);

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

    register::run(&only_agents(&dir, &["codex"]), &script);

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

    register::run(&only_agents(&dir, &["devin"]), &script);

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
    // before this subcommand existed. Marker-only idempotency would append a
    // second copy of every hook to that file.
    let vibe = agent("mistral-vibe");
    let target = dir.write(".vibe/hooks.toml", vibe.contents);
    let script = Script::saying_yes();

    let report = register::run(&only_agents(&dir, &["mistral-vibe"]), &script);

    assert_eq!(report.exit_code(), 0);
    let after = fs::read_to_string(&target).expect("the file");
    assert!(
        script.output().contains("not in a block we manage"),
        "{}",
        script.output()
    );
    // Adopting wraps what is there and changes nothing about what it does.
    assert!(after.contains("# >>> tmux-agent-status >>>"), "{after}");
    assert_eq!(
        after.matches("tmux-agent-status-pre-tool").count(),
        1,
        "a second copy of the hooks was appended:\n{after}"
    );
    let start = after.find("# >>> tmux-agent-status >>>").unwrap();
    let end = after.find("# <<< tmux-agent-status <<<").unwrap();
    assert!(start < end);
    let between = &after[start..end];
    assert!(
        between.contains("tmux-agent-status-pre-tool"),
        "the markers wrapped an empty block instead of the hooks:\n{after}"
    );
    // And a second run has nothing left to do.
    let again = Script::saying_yes();
    let report = register::run(&only_agents(&dir, &["mistral-vibe"]), &again);
    assert_eq!(
        report.outcome(Step::Agents),
        Some(Outcome::AlreadyRegistered)
    );
    assert_eq!(fs::read_to_string(&target).expect("the file"), after);
}

#[test]
fn declining_an_adoption_leaves_the_file_alone_and_still_reports_success() {
    let dir = TempDir::new("run-adopt-declined");
    let vibe = agent("mistral-vibe");
    let target = dir.write(".vibe/hooks.toml", vibe.contents);
    let script = Script::saying_no();

    let report = register::run(&only_agents(&dir, &["mistral-vibe"]), &script);

    // The hooks *are* registered, so declining to rewrap them is not a failure
    // and not an omission.
    assert_eq!(
        report.outcome(Step::Agents),
        Some(Outcome::AlreadyRegistered)
    );
    assert_eq!(report.exit_code(), 0);
    assert_eq!(
        fs::read_to_string(&target).expect("the file"),
        vibe.contents
    );
}

/// A `claude` that answers `plugin list` with `listed`, and exits `code` for
/// anything else.
fn claude_stub(dir: &TempDir, listed: &str, code: i32) -> agents::Claude {
    let path = dir.join("claude");
    fs::write(
        &path,
        format!("#!/bin/sh\ncase \"$2\" in list) echo '{listed}' ;; *) exit {code} ;; esac\n"),
    )
    .expect("the stub");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    agents::Claude::at(path)
}

fn with_claude(dir: &TempDir, claude: Option<agents::Claude>) -> Options {
    Options {
        claude,
        claude_route: agents::ClaudeRoute::Auto,
        ..only_agents(dir, &["claude-code"])
    }
}

#[test]
fn the_plugin_route_installs_and_leaves_settings_json_alone() {
    let dir = TempDir::new("run-plugin");
    let script = Script::saying_yes();

    let report = register::run(
        &with_claude(&dir, Some(claude_stub(&dir, "[]", 0))),
        &script,
    );

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::Registered));
    assert_eq!(report.exit_code(), 0);
    assert!(
        script.output().contains("plugin installed"),
        "{}",
        script.output()
    );
    // 004's promise: the plugin writes its own bookkeeping, and we go nowhere
    // near the file the user was told we would not touch.
    assert!(!dir.join(".claude/settings.json").exists());
}

#[test]
fn declining_the_plugin_installs_nothing_and_is_not_a_failure() {
    let dir = TempDir::new("run-plugin-declined");
    let script = Script::saying_no();

    let report = register::run(
        &with_claude(&dir, Some(claude_stub(&dir, "[]", 0))),
        &script,
    );

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::NotRegistered));
    assert_eq!(report.exit_code(), 0);
    assert!(!dir.join(".claude/settings.json").exists());
    // The confirmation says what it is about to clone, and from where.
    assert!(
        script
            .output()
            .contains("clones the marketplace from GitHub"),
        "{}",
        script.output()
    );
}

#[test]
fn a_claude_that_cannot_answer_fails_the_step_rather_than_merging() {
    let dir = TempDir::new("run-plugin-mute");
    let script = Script::saying_yes();

    // `plugin list` itself fails, so we cannot know whether it is installed.
    let mute = claude_stub(&dir, "", 3);
    fs::write(dir.join("claude"), "#!/bin/sh\nexit 3\n").expect("the stub");

    let report = register::run(&with_claude(&dir, Some(mute)), &script);

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::Failed));
    assert_eq!(report.exit_code(), 1);
    assert!(
        script.output().contains("--claude-route=settings"),
        "{}",
        script.output()
    );
    assert!(!dir.join(".claude/settings.json").exists());
}

#[test]
fn no_claude_at_all_is_the_one_thing_that_falls_through_to_the_merge() {
    let dir = TempDir::new("run-plugin-absent");
    let script = Script::saying_yes();

    let report = register::run(&with_claude(&dir, None), &script);

    assert_eq!(report.exit_code(), 0);
    assert!(dir.join(".claude/settings.json").is_file());
}

#[test]
fn forcing_the_plugin_route_with_no_claude_fails_rather_than_merging() {
    let dir = TempDir::new("run-plugin-forced");
    let script = Script::saying_yes();

    let report = register::run(
        &Options {
            claude_route: agents::ClaudeRoute::Plugin,
            ..with_claude(&dir, None)
        },
        &script,
    );

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::Failed));
    assert!(!dir.join(".claude/settings.json").exists());
}

#[test]
fn a_file_that_cannot_be_merged_into_hands_the_block_back() {
    let dir = TempDir::new("run-manual");
    let target = dir.write(".codex/hooks.json", "not json at all\n");
    let script = Script::saying_yes();

    let report = register::run(&only_agents(&dir, &["codex"]), &script);

    assert_eq!(report.outcome(Step::Agents), Some(Outcome::NotRegistered));
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

    let report = register::run(&options(&dir, tmux_steps()), &script);

    let output = script.output();
    assert_eq!(report.outcome(Step::TmuxHook), Some(Outcome::NotRegistered));
    assert_eq!(
        report.outcome(Step::TmuxFormat),
        Some(Outcome::NotRegistered)
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

    let report = register::run(
        &options(
            &dir,
            register::select(&[Step::TmuxFormat], &[]).expect("valid"),
        ),
        &script,
    );

    let output = script.output();
    assert_eq!(
        report.outcome(Step::TmuxFormat),
        Some(Outcome::NotRegistered)
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

    let report = register::run(
        &options(
            &dir,
            register::select(&[Step::TmuxFormat], &[]).expect("valid"),
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
    // 'No' to the accept question is how the manual edit is asked for; the
    // write questions after it still get yes.
    let script = Script {
        edited: Some(
            "set -g window-status-format 'EDITED#{?@agent_status, #{@agent_status},}'".to_owned(),
        ),
        ..Script::saying_yes_but_not_to("Accept the proposed line")
    };

    register::run(
        &options(
            &dir,
            register::select(&[Step::TmuxFormat], &[]).expect("valid"),
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
fn an_accepted_edit_is_shown_before_the_write_question() {
    let dir = TempDir::new("run-edit-shown");
    dir.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    let script = Script {
        edited: Some(
            "set -g window-status-format 'EDITED#{?@agent_status, #{@agent_status},}'".to_owned(),
        ),
        ..Script::saying_yes_but_not_to("Accept the proposed line")
    };

    register::run(
        &options(
            &dir,
            register::select(&[Step::TmuxFormat], &[]).expect("valid"),
        ),
        &script,
    );

    let log = script.log();
    let question = log
        .find("ASK: Accept the proposed line? Choose 'no' for manual edit")
        .expect("the edit question was asked");
    let shown = log
        .find("SAY:       after your edit: set -g window-status-format 'EDITED")
        .unwrap_or_else(|| panic!("the edited value was not shown back:\n{log}"));
    let write = log
        .find("ASK: Write")
        .expect("the write question was asked");
    assert!(
        question < shown && shown < write,
        "the edited value should appear between the edit question and the write question:\n{log}"
    );
}

#[test]
fn write_questions_stay_distinguishable_when_several_target_the_same_file() {
    // The hook's source-file line and both format splices all land in the
    // same tmux.conf, so if their write questions were not distinguishable
    // (e.g. every one asking a bare "Write <path>?") a user declining one
    // could not tell which they had just answered.
    let dir = TempDir::new("run-same-file-questions");
    dir.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    let script = Script::saying_yes();

    register::run(&options(&dir, tmux_steps()), &script);

    let questions = script.questions();
    // The snippet, the source-file line and both format splices all ask
    // "Write ...?"; the source-file line and the two splices share the same
    // tmux.conf.
    let asked: Vec<&str> = questions
        .lines()
        .filter(|line| line.starts_with("Write "))
        .collect();
    assert!(
        asked.len() >= 3,
        "expected at least the source-file line and both format splices to ask:\n{asked:?}"
    );
    let distinct: std::collections::HashSet<&&str> = asked.iter().collect();
    assert_eq!(
        distinct.len(),
        asked.len(),
        "two write questions were identical, so they cannot be told apart: {asked:?}"
    );
}

#[test]
fn the_edit_question_names_the_file_it_concerns_before_asking() {
    let dir = TempDir::new("run-edit-context");
    let config = dir.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    let script = Script::saying_yes();

    register::run(
        &options(
            &dir,
            register::select(&[Step::TmuxFormat], &[]).expect("valid"),
        ),
        &script,
    );

    let log = script.log();
    // The full header/before/after/question block, contiguous and in order,
    // for each option in turn - not just the first one found.
    for option in register::format::OPTIONS {
        let block = format!(
            "SAY:   the term in {option} - edit {}\n\
             SAY:       before: set -g {option} '#I:#W'\n\
             SAY:       after:  set -g {option} '#I:#W#{{?@agent_status, #{{@agent_status}},}}'\n\
             ASK: Accept the proposed line? Choose 'no' for manual edit",
            config.display()
        );
        assert!(
            log.contains(&block),
            "the block for {option} was not said as a contiguous unit right before its edit \
             question:\n{log}"
        );
    }
    assert!(
        !log.contains("afterwards tmux is asked to read the edited config back")
            && !log.contains("the edit will not be checked against a live tmux"),
        "the probe disclosure should no longer be said at all:\n{log}"
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
        ..Script::saying_yes_but_not_to("Accept the proposed line")
    };

    register::run(
        &options(
            &dir,
            register::select(&[Step::TmuxFormat], &[]).expect("valid"),
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
fn declining_the_reload_says_how_to_do_it_later() {
    let dir = TempDir::new("run-reload-declined");
    let config = dir.write(".config/tmux/tmux.conf", "set -g status on\n");
    let script = Script::saying_no();

    // The config is one the running server loads, so the offer is made - and
    // declined, which must leave the user knowing what to type.
    register::offer_reload(&config, Some(&config.display().to_string()), &script);

    let output = script.output();
    assert!(output.contains("Not reloaded"), "{output}");
    assert!(output.contains("tmux source-file"), "{output}");
}

#[test]
fn a_config_the_running_tmux_does_not_load_is_never_sourced_into_it() {
    // Sourcing anything else applies settings to a live session that nobody
    // asked that tmux to have, which is a real hazard with --tmux-config.
    let dir = TempDir::new("run-reload-elsewhere");
    let config = dir.write(".config/tmux/tmux.conf", "set -g status on\n");
    let script = Script::saying_yes();

    register::offer_reload(&config, Some("/etc/tmux.conf,~/.tmux.conf"), &script);

    let output = script.output();
    assert!(output.contains("Not offering a reload"), "{output}");
    assert!(script.questions().is_empty(), "{}", script.questions());
}

#[test]
fn with_no_running_tmux_there_is_nothing_to_reload_into() {
    let dir = TempDir::new("run-reload-none");
    let config = dir.write(".config/tmux/tmux.conf", "set -g status on\n");
    let script = Script::saying_yes();

    register::offer_reload(&config, None, &script);

    assert!(script.output().is_empty(), "{}", script.output());
}

#[test]
fn a_run_with_no_steps_does_nothing_at_all() {
    let dir = TempDir::new("run-nothing");
    let script = Script::saying_yes();

    let report = register::run(
        &options(
            &dir,
            register::Steps {
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
fn the_snippet_is_written_when_none_is_present_and_then_sourced() {
    let dir = TempDir::new("run-snippet");
    dir.write(".config/tmux/tmux.conf", "set -g status on\n");
    let script = Script::saying_yes();

    let report = register::run(
        &Options {
            snippet: Some(dir.join(".config/tmux/tmux-agent-status.conf")),
            ..options(
                &dir,
                register::select(&[Step::TmuxHook], &[]).expect("valid"),
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

    let report = register::run(&options(&dir, tmux_steps()), &script);

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

// Declining the snippet and accepting the source-file line leaves a line
// pointing at a file that is not there. tmux will not read a config that
// sources a missing file, so the refusal comes from tmux and names the path -
// which is a better answer than anything the hook check could say, and the
// reason that check never has to describe an absent snippet.
#[test]
fn a_source_line_pointing_at_a_snippet_nobody_wrote_is_refused_by_tmux() {
    let dir = TempDir::new("run-snippet-declined");
    let config = dir.write(".config/tmux/tmux.conf", "set -g status on\n");
    let before = fs::read_to_string(&config).expect("the config");
    let snippet = dir.join(".config/tmux/tmux-agent-status.conf");
    let options = Options {
        snippet: Some(snippet.clone()),
        ..options(
            &dir,
            register::select(&[Step::TmuxHook], &[]).expect("a valid selection"),
        )
    };
    // Yes to the source-file line, no to writing the snippet it points at.
    let script = Script::saying_yes_but_not_to(&snippet.display().to_string());

    let report = register::run(&options, &script);

    let output = script.output();
    assert!(!snippet.exists(), "the snippet was written anyway");
    assert_eq!(report.outcome(Step::TmuxHook), Some(Outcome::Failed));
    assert!(
        output.contains("tmux will not read the edited config"),
        "{output}"
    );
    assert!(
        output.contains(&snippet.display().to_string()),
        "the refusal does not name the file tmux could not find:\n{output}"
    );
    // And the config is back the way it was, because a line that does nothing
    // is not worth leaving in somebody's dotfiles.
    assert_eq!(fs::read_to_string(&config).expect("the config"), before);
}
