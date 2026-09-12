//! The only module that knows whether there is a terminal.
//!
//! `-y` and `--dry-run` are answered here, so nothing else in `install`
//! branches on interactivity: every other module takes an answer and acts on
//! it. That is also why this is the one file `just coverage` skips - exercising
//! it means driving a terminal, and a pty harness would prove that `dialoguer`
//! works rather than that we do. Everything worth asserting about a run's
//! decisions lives in the modules this one feeds.

use std::io::{self, IsTerminal, Write};

use dialoguer::{Confirm, Editor, MultiSelect};

use super::write::{Ask, Warning};

/// A question that cannot be answered, which is a usage error rather than a
/// guess.
///
/// A hook that silently picks defaults on a CI box is how a config gets edited
/// by something nobody asked.
#[derive(Debug)]
pub struct NoTerminal;

impl std::fmt::Display for NoTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "install needs to ask a question and there is no terminal to ask it on.\n\
             Pass -y to take the recommended answer to every question, or --dry-run \
             to see what it would do.",
        )
    }
}

impl std::error::Error for NoTerminal {}

/// How this run answers its questions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answers {
    /// A terminal, and a human at it.
    Interactive,
    /// `-y`: the recommended answer to everything, said out loud as it goes.
    Recommended,
    /// `--dry-run`: nothing is asked, because nothing is written.
    DryRun,
}

/// Asks the questions, and writes what the run has to say.
pub struct Prompt {
    answers: Answers,
}

impl Prompt {
    /// Decide how this run will answer, or refuse to start.
    pub fn new(yes: bool, dry_run: bool) -> Result<Prompt, NoTerminal> {
        let answers = match (dry_run, yes) {
            (true, _) => Answers::DryRun,
            (false, true) => Answers::Recommended,
            (false, false) if io::stdin().is_terminal() => Answers::Interactive,
            (false, false) => return Err(NoTerminal),
        };
        Ok(Prompt { answers })
    }

    pub fn answers(&self) -> Answers {
        self.answers
    }

    pub fn is_dry_run(&self) -> bool {
        self.answers == Answers::DryRun
    }

    /// Say something the user needs to read. Plain stdout: this is the output
    /// of the command, not a diagnostic.
    pub fn say(&self, line: impl AsRef<str>) {
        let _ = writeln!(io::stdout(), "{}", line.as_ref());
    }

    /// Ask a yes or no question.
    ///
    /// `recommended` is what `-y` answers, and what a bare return takes.
    pub fn confirm(&self, question: &str, recommended: bool) -> bool {
        match self.answers {
            // A dry run performs no write, so every question is moot; taking
            // the recommended answer is what makes the printed plan the plan
            // that a real `-y` run would carry out.
            Answers::DryRun => recommended,
            Answers::Recommended => {
                self.say(format!("{question} [{}, -y]", yes_or_no(recommended)));
                recommended
            }
            Answers::Interactive => Confirm::new()
                .with_prompt(question)
                .default(recommended)
                .interact()
                .unwrap_or(false),
        }
    }

    /// Choose from the agent list.
    ///
    /// Every agent appears, preselected or not, so nothing is ever installed
    /// without having been shown.
    pub fn choose(&self, question: &str, rows: &[(String, bool)]) -> Vec<usize> {
        let preselected = || {
            rows.iter()
                .enumerate()
                .filter(|(_, (_, on))| *on)
                .map(|(index, _)| index)
                .collect()
        };
        match self.answers {
            Answers::DryRun => preselected(),
            Answers::Recommended => {
                self.say(question);
                for (label, on) in rows {
                    self.say(format!("  [{}] {label}", if *on { 'x' } else { ' ' }));
                }
                preselected()
            }
            Answers::Interactive => {
                let labels: Vec<&str> = rows.iter().map(|(label, _)| label.as_str()).collect();
                let checked: Vec<bool> = rows.iter().map(|(_, on)| *on).collect();
                MultiSelect::new()
                    .with_prompt(question)
                    .items(&labels)
                    .defaults(&checked)
                    .interact()
                    .unwrap_or_else(|_| preselected())
            }
        }
    }

    /// Hand a value to `$EDITOR` and take back what comes out.
    ///
    /// Editing a 300-character format string in a one-line prompt is hostile,
    /// which is the whole reason this exists. `None` means the user changed
    /// nothing, or there was no editor to open.
    pub fn edit(&self, value: &str) -> Option<String> {
        match self.answers {
            Answers::Interactive => Editor::new().edit(value).ok().flatten(),
            // Nobody is there to edit anything.
            _ => None,
        }
    }
}

/// A warning is a question, and `-y` proceeds on every one and says so.
impl Ask for Prompt {
    fn warn(&self, warning: &Warning) -> bool {
        self.say(warning.message());
        self.confirm("Go ahead anyway?", true)
    }
}

fn yes_or_no(answer: bool) -> &'static str {
    match answer {
        true => "yes",
        false => "no",
    }
}
