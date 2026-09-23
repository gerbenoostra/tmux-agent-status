//! `tmux-agent-status register`: write the hooks and configs `docs/register.md` documents.
//!
//! Everything under here is the one subcommand that touches a user's files, and
//! only when a human types it. The hook commands are unchanged and still write
//! nothing but two tmux options and a bell. The contract that licences the
//! writing - resolve symlinks, lock, back up, never truncate, verify, restore -
//! is stated in CONTRIBUTING.md, and `write` is where it lives.

pub mod agents;
pub mod format;
pub mod probe;
pub mod prompt;
pub mod tmux_conf;
pub mod write;

use std::fmt;
use std::path::{Path, PathBuf};

/// The comment that opens a block this tool manages.
///
/// Markers are what make the future `unregister` a deletion rather than a second
/// parse, and they are the same two lines in tmux config and in TOML, because
/// both take `#` comments.
pub const MARKER_START: &str = "# >>> tmux-agent-status >>>";

/// The comment that closes one.
pub const MARKER_END: &str = "# <<< tmux-agent-status <<<";

/// Append a marked block, whatever the file already ends with.
///
/// Two mechanical details that are easy to get wrong, and so are written down:
/// the block is preceded by a newline when the file does not already end in
/// one, or the marker lands on the tail of the user's last line; and the block
/// ends in a newline of its own.
pub fn append_marked(text: &str, body: &str) -> String {
    let separator = match text.is_empty() || text.ends_with('\n') {
        true => "",
        false => "\n",
    };
    let body = body.strip_suffix('\n').unwrap_or(body);
    format!("{text}{separator}{MARKER_START}\n{body}\n{MARKER_END}\n")
}

/// Whether the text already carries a block this tool manages.
pub fn has_marked_block(text: &str) -> bool {
    text.lines().any(|line| line.trim() == MARKER_START)
}

/// Where the user's configuration lives.
///
/// Read from the environment once, at the edge, and passed down: the tests
/// point this at a temp directory, and a tool that cannot be redirected cannot
/// be tested. Never `getpwuid`, for the same reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`, which is not always `$HOME/.config` and is not
    /// always set.
    pub xdg_config: Option<PathBuf>,
}

impl Home {
    pub fn from_env() -> Option<Home> {
        Some(Home {
            home: PathBuf::from(std::env::var_os("HOME")?),
            xdg_config: std::env::var_os("XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        })
    }

    /// A path under `$HOME`.
    pub fn join(&self, tail: &str) -> PathBuf {
        self.home.join(tail)
    }

    /// A path under the config directory, whichever one that is.
    ///
    /// `$XDG_CONFIG_HOME` when it is set, and `$HOME/.config` otherwise, which
    /// is the default the specification gives and the one tmux documents.
    pub fn config(&self, tail: &str) -> PathBuf {
        match &self.xdg_config {
            Some(dir) => dir.join(tail),
            None => self.home.join(".config").join(tail),
        }
    }
}

/// One of the three things `register` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Agents,
    TmuxHook,
    TmuxFormat,
}

impl Step {
    pub const ALL: [Step; 3] = [Step::Agents, Step::TmuxHook, Step::TmuxFormat];

    pub fn flag(self) -> &'static str {
        match self {
            Step::Agents => "--agents",
            Step::TmuxHook => "--tmux-hook",
            Step::TmuxFormat => "--tmux-format",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::Agents => "agent hooks",
            Step::TmuxHook => "the tmux source-file line",
            Step::TmuxFormat => "the tmux format term",
        }
    }
}

/// Which steps this run performs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steps {
    pub agents: bool,
    pub tmux_hook: bool,
    pub tmux_format: bool,
}

impl Steps {
    pub fn has(self, step: Step) -> bool {
        match step {
            Step::Agents => self.agents,
            Step::TmuxHook => self.tmux_hook,
            Step::TmuxFormat => self.tmux_format,
        }
    }
}

/// Work out which steps run, from what the flags asked for.
///
/// `--agents=codex,cursor` selects the agents step *and* narrows it; a bare
/// `--agents` selects the step and leaves the choice to detection. That is what
/// makes `--agents` alone mean "only the agents", without a second "only" flag.
pub fn select(positive: &[Step], negative: &[Step]) -> Result<Steps, String> {
    if !positive.is_empty() && !negative.is_empty() {
        return Err(format!(
            "{} and {} cannot be given together: name the steps to run, or the steps to skip",
            positive[0].flag(),
            negative[0].flag().replacen("--", "--no-", 1)
        ));
    }
    let chosen = |step: Step| match (positive.is_empty(), negative.is_empty()) {
        // Nothing said: all three.
        (true, true) => true,
        // Only positives: exactly those.
        (false, _) => positive.contains(&step),
        // Only negatives: all three minus those.
        (true, false) => !negative.contains(&step),
    };
    Ok(Steps {
        agents: chosen(Step::Agents),
        tmux_hook: chosen(Step::TmuxHook),
        tmux_format: chosen(Step::TmuxFormat),
    })
}

/// Everything the run was asked to do.
pub struct Options {
    pub steps: Steps,
    /// The agents named on the command line. `None` leaves it to detection.
    ///
    /// A name that is valid but undetected registers anyway: naming an agent
    /// explicitly is a stronger signal than the absence of its config
    /// directory, and registering hooks before the agent is a legitimate order
    /// to do things in.
    pub agents: Option<Vec<&'static agents::Agent>>,
    pub claude_route: agents::ClaudeRoute,
    /// The Claude Code CLI to use. The command line fills this from `PATH`;
    /// `None` means there is none, which is the only thing that falls through
    /// to the settings merge. A test points it at a stub.
    pub claude: Option<agents::Claude>,
    pub marketplace: Option<String>,
    pub tmux_config: Option<PathBuf>,
    pub snippet: Option<PathBuf>,
    /// Whether to let tmux mark our homework. `--no-tmux-probe` downgrades the
    /// tmux steps to the parser's own word.
    pub probe: bool,
    pub home: Home,
    pub exe: Option<PathBuf>,
    /// `$PREFIX`, read at the edge like `$HOME`, and searched for a shipped
    /// snippet.
    pub prefix: Option<PathBuf>,
}

/// What one target's write would be, before anything is written.
///
/// Built by the plan phase from the bytes on disk and a pure function, which is
/// what makes `--dry-run` free and what stops a run from leaving two of three
/// steps applied because the third asked a question the user did not like.
pub struct Change {
    pub what: String,
    pub path: PathBuf,
    /// What the file would become, for the confirmation to show.
    ///
    /// A preview, and only that. The bytes actually written are built by
    /// `rebuild` from what is on disk *inside the lock*, because two steps can
    /// target the same file and the second one's view of it would otherwise be
    /// the view from before the first one wrote.
    pub preview: String,
    pub rebuild: Rebuild,
    /// Whether the bytes are a complete document in this file's own language.
    pub parses: fn(&str) -> bool,
    /// Whether the file existed, which is "created" rather than "edited".
    pub creating: bool,
    /// Anything the confirmation should say out loud.
    pub notes: Vec<String>,
    /// The semantic check, for the steps that have one.
    pub verify: Option<TmuxVerify>,
    /// Whether planning already said what this change is and showed its
    /// before/after, so `confirm` skips repeating that recap - true only for
    /// the tmux-format splice, which asks its own edit question during
    /// planning and would otherwise say the same thing twice. The write
    /// question itself still names the change either way (see `confirm`),
    /// since several changes can share one target file.
    pub announced: bool,
}

/// How a target's new contents are built from whatever is on disk.
///
/// An enum rather than a closure so that a plan can be printed, compared and
/// tested. Each variant is a pure function of the current bytes, which is what
/// the safe write's step 5 asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rebuild {
    /// The whole file is ours: replace it.
    Whole(String),
    /// Merge an agent's entries in.
    Agent(&'static agents::Agent),
    /// Append the marked `source-file` block.
    SourceBlock(PathBuf),
    /// Replace one logical line with the spliced version of itself.
    ///
    /// The line is found again by its own text rather than by the number the
    /// plan recorded. Two things move a number: a logical line spans as many
    /// physical lines as it has continuations, and the *other* format target
    /// writes the same file first and may have collapsed such a run above this
    /// one. Matching the text answers the question the plan actually asks -
    /// is the line we read still there - at every line number it can be at.
    FormatLine {
        /// Where the plan found it, for the message when it has gone.
        reported: usize,
        /// The logical line as it was when the plan was made. Gone, and
        /// nothing is written: the bytes are not the ones we read.
        was: String,
        with: String,
    },
    /// Append a marked block setting both format options, for a config that
    /// assigns neither.
    FormatPair(String),
    /// Append a marked block setting one option, for a config that assigns the
    /// other and leaves this one on tmux's default.
    FormatOne { option: String, line: String },
    /// Wrap entries the user placed by hand in markers, changing nothing else.
    Adopt(&'static agents::Agent),
}

impl Rebuild {
    /// Build the new contents, inside the lock, from the bytes on disk.
    pub fn apply(&self, current: &str) -> Result<write::Plan, String> {
        match self {
            Rebuild::Whole(contents) => Ok(match current == contents {
                true => write::Plan::AlreadyRegistered,
                false => write::Plan::Write(contents.clone()),
            }),
            Rebuild::Agent(agent) => agent.merge(current).map_err(|why| why.to_string()),
            Rebuild::SourceBlock(snippet) => match tmux_conf::sources_snippet(current) {
                true => Ok(write::Plan::AlreadyRegistered),
                false => tmux_conf::with_source_block(current, snippet)
                    .map(write::Plan::Write)
                    .ok_or_else(|| unspellable(snippet)),
            },
            Rebuild::FormatLine {
                reported,
                was,
                with,
            } => {
                // The last one carrying those bytes, for the same reason the
                // plan chose the last assignment: that is the one tmux ends up
                // honouring. Gone entirely, and nothing is written, because the
                // cost of being wrong is a line tmux discards.
                let Some(line) = format::logical_lines(current)
                    .into_iter()
                    .rfind(|line| &line.text == was)
                else {
                    return Err(format!(
                        "the config moved under us: line {} is no longer the one we read",
                        reported + 1
                    ));
                };
                Ok(write::Plan::Write(tmux_conf::replace_lines(
                    current, line.first, line.last, with,
                )))
            }
            Rebuild::Adopt(agent) => Ok(match has_marked_block(current) {
                true => write::Plan::AlreadyRegistered,
                false => write::Plan::Write(agent.adopt(current)),
            }),
            Rebuild::FormatPair(block) => Ok(match format::references_agent_status(current) {
                true => write::Plan::AlreadyRegistered,
                false => write::Plan::Write(append_marked(current, block)),
            }),
            // Per option, not per file: by the time this runs, the *other*
            // option may already have been spliced, and asking whether the
            // file mentions `@agent_status` anywhere would read that as this
            // option being done too.
            Rebuild::FormatOne { option, line } => {
                Ok(match already_carries_the_term(current, option) {
                    true => write::Plan::AlreadyRegistered,
                    false => write::Plan::Write(append_marked(current, line)),
                })
            }
        }
    }
}

/// Whether some config text already assigns `option` a value carrying the term.
fn already_carries_the_term(text: &str, option: &str) -> bool {
    format::logical_lines(text)
        .iter()
        .filter_map(|line| format::parse(&line.text).line())
        .any(|line| line.option == option && line.already_registered())
}

/// One planned piece of work.
pub enum Action {
    Write(Box<Change>),
    /// The hooks are there, but not in a block we manage. Offered as an
    /// adoption - rewrap them in markers, changing no behaviour - because the
    /// alternative is a second copy of every hook.
    Adopt(Box<Change>),
    /// The Claude Code plugin, which is commands rather than a file.
    Plugin {
        claude: agents::Claude,
        marketplace: String,
    },
    /// Already done. Nothing is written and no backup is taken.
    AlreadyRegistered,
    /// Cannot be done here; this is what to do by hand. Not a failure: a
    /// refusal the user can act on leaves them no worse off than before they
    /// ran anything.
    Manual(String),
    /// Detection or planning failed. This one is a failure.
    Failed(String),
}

/// A planned action, and which step asked for it.
pub struct Planned {
    pub step: Step,
    pub what: String,
    pub action: Action,
}

/// Work that survived the confirmation, and so is actually going to happen.
///
/// A type of its own rather than a filtered list of `Planned`, so that `apply`
/// cannot be handed something already settled and has no arm for it.
enum Work {
    Write {
        step: Step,
        what: String,
        change: Box<Change>,
    },
    Plugin {
        step: Step,
        what: String,
        claude: agents::Claude,
        marketplace: String,
    },
}

/// How a step came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Registered,
    AlreadyRegistered,
    /// Nothing was written and the user knows what to do. Exit code 0: a
    /// refusal the user chose is not an error.
    NotRegistered,
    Failed,
}

/// What the run did, and what to exit with.
#[derive(Default)]
pub struct Report {
    pub lines: Vec<String>,
    outcomes: Vec<(Step, Outcome)>,
    /// Whether this run wrote nothing whatever it found.
    dry_run: bool,
}

impl Report {
    fn record(&mut self, step: Step, outcome: Outcome) {
        self.outcomes.push((step, outcome));
    }

    pub fn outcome(&self, step: Step) -> Option<Outcome> {
        self.outcomes
            .iter()
            .filter(|(which, _)| *which == step)
            .map(|(_, outcome)| *outcome)
            .reduce(worst)
    }

    /// 0 when every requested step is registered or was already; 1 when at
    /// least one failed, with every file it touched back the way it was.
    ///
    /// A dry run is always 0. Nothing ran, so nothing failed: what it printed
    /// is a plan, and a step it could not plan is part of the plan rather than
    /// a step that went wrong. The outcomes are still recorded, because the
    /// printed plan is built from them.
    pub fn exit_code(&self) -> u8 {
        match !self.dry_run
            && self
                .outcomes
                .iter()
                .any(|(_, outcome)| *outcome == Outcome::Failed)
        {
            true => 1,
            false => 0,
        }
    }
}

/// The outcome a step reports when its targets disagree.
fn worst(a: Outcome, b: Outcome) -> Outcome {
    let rank = |outcome: Outcome| match outcome {
        Outcome::Failed => 3,
        Outcome::NotRegistered => 2,
        Outcome::Registered => 1,
        Outcome::AlreadyRegistered => 0,
    };
    match rank(a) >= rank(b) {
        true => a,
        false => b,
    }
}

/// Everything `register` is allowed to move in a tmux config.
///
/// One list for both tmux steps rather than one each, because they edit the
/// same file in sequence: the format step's baseline is taken before the hook
/// step has written, so it sees the hooks appear as well. The assertion that
/// matters is unchanged - *nothing `register` is not responsible for*
/// moved - and an abandoned config still shows up instantly, because it reverts
/// everything else the user set.
///
/// Hook names are compared with any `[index]` stripped: tmux reports an unset
/// hook under its bare name and a set one under `name[index]`, so one edit
/// shows up as two changes under two spellings of the same thing.
fn ours_to_change() -> Vec<String> {
    let mut names: Vec<String> = format::OPTIONS.iter().map(|o| (*o).to_owned()).collect();
    names.extend(HOOKS.iter().map(|hook| (*hook).to_owned()));
    names.push(FOCUS_EVENTS.to_owned());
    names
}

/// What the edit was for, and so what the probe has to find afterwards.
///
/// "Nothing unexpected moved" is only half the question, and on its own it is
/// the half an edit that does nothing passes perfectly: a line spliced where a
/// later assignment overrides it - an `if-shell` below it, a fragment sourced
/// after it - leaves the file edited, the run reporting success, and no glyph
/// anywhere. That is the worst outcome this module has, so the probe is asked
/// for the other half directly.
pub enum Landed {
    /// The hook step: every hook is registered afterwards, which is what says
    /// the sourced snippet actually ran, and the snippet turns `focus-events`
    /// on, because one that lands the hooks but not the option silently loses
    /// the one hook that sees the terminal.
    ///
    /// The option is read from the snippet, not from tmux: a
    /// `set -g focus-events off` after the source-file line is how a user
    /// declines it, so tmux reading it as off is a choice as often as a fault.
    ///
    /// Only a snippet that exists and sets no hooks reaches this. One that is
    /// missing is refused earlier and better: tmux will not read a config that
    /// sources a file that is not there, so `probe::check` fails first and
    /// names the path it could not find.
    Hooks { snippet: PathBuf },
    /// The format step: these options carry the term afterwards. Per option,
    /// because the two are written one at a time and the second has not been
    /// written yet when the first is checked.
    Term(Vec<String>),
}

impl Landed {
    /// Whether what this edit was for is in the dump tmux just gave us.
    fn found(&self, dumped: &probe::Dump) -> Result<(), String> {
        match self {
            Landed::Hooks { snippet } => {
                if let Some(missing) = HOOKS.iter().find(|hook| {
                    !dumped.any_value(hook, |set| set.contains(agents::COMMAND_PREFIX))
                }) {
                    return Err(format!(
                        "the config sources our snippet, but tmux read the config back without \
                         the {missing} hook: the file it points at does not set it."
                    ));
                }
                match std::fs::read_to_string(snippet)
                    .is_ok_and(|text| tmux_conf::turns_on_focus_events(&text))
                {
                    true => Ok(()),
                    false => Err(format!(
                        "the config sources our snippet, but {} does not turn {FOCUS_EVENTS} \
                         on, so pane-focus-in never sees the terminal regain focus.",
                        snippet.display()
                    )),
                }
            }
            // What is reported is what was observed. A later assignment is the
            // likeliest reason tmux does not read the term back, and it is not
            // the only one - a window-local `setw` at config-load time, a
            // `%if` the walk parsed as a plain line - so the message names the
            // symptom and leaves the cause to the config in front of the user.
            Landed::Term(options) => match options
                .iter()
                .find(|option| !dumped.any_value(option, format::references_agent_status))
            {
                None => Ok(()),
                Some(option) => Err(format!(
                    "the edit landed, but tmux still reads {option} without the term, so the \
                     assignment we edited is not the one it honours.\n  \
                     Add {} to the assignment that does, by hand.",
                    format::TERM
                )),
            },
        }
    }
}

/// The semantic check for a tmux step: start a throwaway server on the config
/// tmux actually loads, and assert that the only things that moved are the ones
/// we meant to move.
pub struct TmuxVerify {
    /// The config tmux loads, which is not always the file we edited: the
    /// winning format line can live in a sourced fragment, and probing a
    /// fragment on its own tests a config the user does not have.
    pub entry: PathBuf,
    pub baseline: probe::Dump,
    /// The option and hook names allowed to differ.
    pub expected: Vec<String>,
    /// What this edit was for, which is the half of the question that
    /// "nothing unexpected moved" cannot answer.
    pub landed: Landed,
}

impl write::Verify for TmuxVerify {
    fn verify(&self, _: &Path) -> Result<(), String> {
        // First: can tmux read it at all. `source-file` names the file, the
        // line and the reason, which is the only useful error tmux ever gives
        // about a config.
        if let Some(Err(complaint)) = probe::check(&self.entry) {
            return Err(format!("tmux will not read the edited config: {complaint}"));
        }
        // Then: does it mean what we meant. A bare value containing a space
        // parses perfectly well and is discarded in silence, so parsing is not
        // the whole question.
        let dumped = match faulty("no-dump") {
            // A tmux that stopped answering between the write and the check.
            true => None,
            false => probe::dump(&self.entry),
        };
        let Some(candidate) = dumped else {
            // No tmux to ask. The edit stands unchecked.
            return Ok(());
        };
        let moved = match faulty("bad-value") {
            // An edit tmux parses perfectly well that moves something else.
            true => vec![probe::Change {
                name: "status-left".to_owned(),
                before: None,
                after: Some("CHANGED".to_owned()),
            }],
            false => probe::changes(&self.baseline, &candidate),
        };
        let unexpected: Vec<String> = moved
            .into_iter()
            .map(|change| change.name)
            .filter(|name| {
                !self
                    .expected
                    .iter()
                    .any(|ours| ours == probe::without_index(name))
            })
            .collect();
        if !unexpected.is_empty() {
            // Everything reverting at once is what an abandoned config looks
            // like, and it costs the user their whole configuration rather
            // than just our glyph.
            return Err(format!(
                "tmux read the edited config back differently than we meant: {} \
                 also changed. A config tmux cannot parse is abandoned whole.",
                unexpected.join(", ")
            ));
        }
        // And the other half: that what the edit was *for* is there. An edit
        // that changed nothing at all passes every check above.
        self.landed.found(&candidate)
    }
}

/// A tmux config file is complete when it has anything in it at all.
///
/// tmux has no notion of a truncated config - every prefix of a valid one is
/// also valid - so emptiness is the only truncation this can see, and it is the
/// one that matters: a write that lost everything.
fn not_empty(text: &str) -> bool {
    !text.is_empty()
}

/// The test-only fault switch, shared with `write` and unstable in the same
/// way: it names the branches a test cannot otherwise reach, because a branch
/// no test can reach is a branch nobody has read.
///
/// Every stage here stands for something a real machine does and a test cannot
/// arrange: a quoting bug that produces a config tmux throws away whole, a tmux
/// whose own default cannot be quoted, a file that moves between the plan and
/// the write, a tmux that stops answering mid-run.
fn faulty(stage: &str) -> bool {
    std::env::var("TMUX_AGENT_STATUS_TEST_FAULT")
        .is_ok_and(|value| value.split(',').any(|named| named == stage))
}

/// What to say about `/etc/tmux.conf`, which is never chosen.
///
/// It needs root and it registers the tool for every user of the machine, which
/// is not what anyone typing this command meant. So it is reported, with the
/// way to ask for it on purpose.
fn system_wide_note(listed: Option<&str>, exists: impl Fn(&Path) -> bool) -> String {
    listed
        .map(tmux_conf::candidates)
        .unwrap_or_default()
        .iter()
        .filter(|candidate| tmux_conf::is_system_wide(candidate) && exists(candidate))
        .map(|candidate| {
            format!(
                "{} exists and is not being touched: it needs root, and it would register \
                 this for every user of the machine.\nPass --tmux-config if that really \
                 was the intent.\n",
                candidate.display()
            )
        })
        .collect()
}

/// What to say about a config that sources a relative path.
///
/// Which assignment of an option wins is decided by the order tmux executes
/// them in, and a relative `source-file` is resolved against the working
/// directory of whatever started the server. We follow it from `$HOME`, which
/// is where the probe puts its own cwd, so the walk and the check that marks
/// its homework agree - but a server started from somewhere else read a
/// different file, and may have a different winner. Reported, because the one
/// moment the user can act on that is while reading the plan.
fn ordering_notes(relative: &[String]) -> Vec<String> {
    match relative.is_empty() {
        true => Vec::new(),
        false => vec![format!(
            "this config sources {} by a relative path, which tmux resolves against the \
             directory the server was started in; it is read here from $HOME, so which \
             assignment wins can differ if yours was started elsewhere",
            relative.join(", ")
        )],
    }
}

/// A message indented under the step it belongs to, continuation lines included.
///
/// The steps print their refusals under a heading, and a message that indents
/// only its first line reads as two entries rather than one.
fn indented(message: &dyn fmt::Display) -> String {
    format!("    {}", message.to_string().replace('\n', "\n    "))
}

/// What to say about a snippet path no tmux quoting can carry.
///
/// A path holding both a `'` and one of the characters double quotes give a
/// meaning to. Rare, and the answer is to move the snippet rather than to
/// write a line that makes tmux abandon the config file it sits in.
fn unspellable(snippet: &Path) -> String {
    format!(
        "{} cannot be written into a tmux config line: it holds both a single quote \
         and something double quotes would expand.\n    Pass --snippet with a path \
         that holds neither.",
        snippet.display()
    )
}

/// The hooks the shipped snippet registers.
///
/// Named here as well as in the snippet because this is the list the probe
/// checks actually arrived: a `source-file` line pointing at something that
/// sets none of them is a line that does nothing.
const HOOKS: [&str; 3] = [
    "session-window-changed",
    "window-pane-changed",
    "pane-focus-in",
];

/// What the user is told about the one global option the snippet sets, because
/// nothing else in it shows the user what the snippet contains. It carries the
/// opt-out itself: the snippet is rewritten whole and verified on every run, so
/// only a line in the user's own config, after the source-file line, survives.
const FOCUS_EVENTS_NOTE: &str = "the snippet turns on the global tmux option focus-events, so \
that returning to the terminal clears the focused pane; to decline it, add `set -g focus-events \
off` to your own tmux config after the source-file line";

/// The option the shipped snippet turns on: without it `pane-focus-in` fires on
/// attach and never when the terminal regains focus.
const FOCUS_EVENTS: &str = "focus-events";

/// Run the whole thing.
///
/// Every step is the same five phases - detect, plan, confirm, apply, verify -
/// and nothing writes until every phase-3 answer is in. That is what makes
/// `--dry-run` free, and what stops a run from leaving two of three steps
/// applied because the third asked a question the user did not like.
///
/// A step that fails does not stop the run: the remaining steps still apply,
/// and the summary says which of the three landed. No step reads another's
/// output, so a failure cannot corrupt what follows. The glyph does need all
/// three to appear, but a run that does what it can and says exactly what it
/// did not beats one that abandons work it was able to finish.
pub fn run(options: &Options, prompt: &dyn prompt::Interaction) -> Report {
    let mut report = Report::default();

    // Phases 1 and 2, for every step, before a single byte is written.
    let mut planned: Vec<Planned> = Vec::new();
    if options.steps.agents {
        planned.extend(plan_agents(options, prompt));
    }
    let tmux = match options.steps.tmux_hook || options.steps.tmux_format {
        true => Some(TmuxPlan::detect(options, prompt)),
        false => None,
    };
    if let Some(tmux) = &tmux {
        if options.steps.tmux_hook {
            planned.extend(tmux.plan_hook(options));
        }
        if options.steps.tmux_format {
            planned.extend(tmux.plan_format(options, prompt));
        }
    }

    // Phase 3. Every question, and then no more questions.
    if tmux.is_none() {
        prompt.say("This is what register would do:");
    }
    let approved = confirm(planned, prompt, &mut report);

    if prompt.is_dry_run() {
        prompt.say("\n--dry-run: nothing above was written.");
        report.dry_run = true;
        return report;
    }

    // Phases 4 and 5.
    apply(approved, prompt, &mut report);
    summarise(&report, prompt, options);
    if let Some(tmux) = &tmux {
        if report.exit_code() == 0 {
            offer_reload(tmux.config.path(), probe::config_files().as_deref(), prompt);
        }
    }
    report
}

/// Show every planned change and take the answers, in one pass.
///
/// What comes back is the work to do, not indices into what was planned:
/// everything settled here - already registered, handed back, refused - is
/// settled, and `apply` never sees it again.
fn confirm(
    planned: Vec<Planned>,
    prompt: &dyn prompt::Interaction,
    report: &mut Report,
) -> Vec<Work> {
    let mut approved = Vec::new();
    for item in planned {
        let (step, what) = (item.step, item.what);
        match item.action {
            Action::AlreadyRegistered => {
                prompt.say(&format!("  {what} - already registered"));
                report.record(step, Outcome::AlreadyRegistered);
            }
            Action::Manual(advice) => {
                prompt.say(&format!("  {what} - not registered\n{advice}"));
                report.record(step, Outcome::NotRegistered);
            }
            Action::Failed(why) => {
                prompt.say(&format!("  {what} - failed\n    {why}"));
                report.record(step, Outcome::Failed);
            }
            Action::Plugin {
                marketplace,
                claude,
            } => {
                prompt.say(&format!("  {what} - install the Claude Code plugin"));
                prompt.say("    This clones the marketplace from GitHub:");
                for command in claude.plugin_commands(&marketplace) {
                    prompt.say(&format!("      {}", command.join(" ")));
                }
                match prompt.confirm(&format!("Install {what}?"), true) {
                    true => approved.push(Work::Plugin {
                        step,
                        what,
                        claude,
                        marketplace,
                    }),
                    false => report.record(step, Outcome::NotRegistered),
                }
            }
            Action::Adopt(change) => {
                prompt.say(&format!(
                    "  {what} - already registered, but not in a block we manage"
                ));
                for note in &change.notes {
                    prompt.say(&format!("    {note}"));
                }
                match prompt.confirm(
                    &format!("Adopt the hooks in {}?", change.path.display()),
                    true,
                ) {
                    true => approved.push(Work::Write { step, what, change }),
                    // Declining leaves the file untouched and the step reports
                    // success, because the hooks *are* registered.
                    false => report.record(step, Outcome::AlreadyRegistered),
                }
            }
            Action::Write(change) => {
                if !change.announced {
                    prompt.say(&format!(
                        "  {what} - {} {}",
                        match change.creating {
                            true => "create",
                            false => "edit",
                        },
                        change.path.display()
                    ));
                    for note in &change.notes {
                        prompt.say(&format!("    {note}"));
                    }
                } else {
                    debug_assert!(
                        change.notes.is_empty(),
                        "an announced change's notes are never printed"
                    );
                }
                // Named every time, announced or not: when several changes
                // touch the same file, a bare "Write <path>?" repeats
                // verbatim and cannot be told apart, and an announced
                // change's header scrolled away during planning, above every
                // other step's own block.
                match prompt.confirm(&format!("Write {what} - {}?", change.path.display()), true) {
                    true => approved.push(Work::Write { step, what, change }),
                    false => report.record(step, Outcome::NotRegistered),
                }
            }
        }
    }
    approved
}

/// Phases 4 and 5: the safe write, per target, in the order the list was built
/// - agents, then the tmux hook, then the tmux format.
fn apply(approved: Vec<Work>, prompt: &dyn prompt::Interaction, report: &mut Report) {
    for item in approved {
        match item {
            Work::Plugin {
                step,
                what,
                claude,
                marketplace,
            } => match claude.install_plugin(&marketplace) {
                Ok(()) => {
                    report.lines.push(format!("{what}: plugin installed"));
                    report.record(step, Outcome::Registered);
                }
                // The routes are chosen by what is available, not by what
                // worked. Falling back here would edit `~/.claude/settings.json`
                // on a machine where the user was promised it would not be, as
                // the silent consequence of a network blip.
                Err(why) => {
                    prompt.say(&format!(
                        "{what}: failed\n{why}\n  To merge into ~/.claude/settings.json \
                         instead, re-run with --claude-route=settings."
                    ));
                    report.record(step, Outcome::Failed);
                }
            },
            Work::Write { step, what, change } => {
                let outcome = write_one(&what, *change, prompt, report);
                report.record(step, outcome);
            }
        }
    }
}

fn write_one(
    what: &str,
    change: Change,
    prompt: &dyn prompt::Interaction,
    report: &mut Report,
) -> Outcome {
    let verify = change.verify.as_ref().map(|v| v as &dyn write::Verify);
    let writer = write::SafeWrite {
        path: &change.path,
        ask: prompt.as_ask(),
        parses: change.parses,
        verify,
        faults: write::Faults::from_env(),
    };
    let mut refused = None;
    let outcome = writer.apply(|current| match rebuild(&change.rebuild, current) {
        Ok(plan) => plan,
        Err(why) => {
            refused = Some(why);
            write::Plan::AlreadyRegistered
        }
    });
    if let Some(why) = refused {
        prompt.say(&format!("{what}: failed\n  {why}"));
        return Outcome::Failed;
    }
    match outcome {
        Ok(written) => {
            report.lines.push(summary_line(what, &written));
            outcome_of(written.outcome)
        }
        Err(error) => {
            prompt.say(&format!("{what}: failed\n  {error}"));
            Outcome::Failed
        }
    }
}

/// Build the new contents, with the fault switch standing in for a file that
/// moved between the plan and the write.
fn rebuild(rebuild: &Rebuild, current: &str) -> Result<write::Plan, String> {
    match faulty("rebuild") {
        true => Err("the file moved under us".to_owned()),
        false => rebuild.apply(current),
    }
}

/// What a completed write means for the step that asked for it.
///
/// A write can come back as "already registered" even when the plan said
/// otherwise: the plan reads the file before the confirmation, and the merge
/// runs again inside the lock against whatever is there by then.
fn outcome_of(written: write::Outcome) -> Outcome {
    match written {
        write::Outcome::AlreadyRegistered => Outcome::AlreadyRegistered,
        write::Outcome::Created | write::Outcome::Edited => Outcome::Registered,
    }
}

fn summary_line(what: &str, written: &write::Written) -> String {
    let did = match written.outcome {
        write::Outcome::Created => "created",
        write::Outcome::Edited => "edited",
        write::Outcome::AlreadyRegistered => "already registered",
    };
    let backup = written
        .backup
        .as_ref()
        .map(|path| format!(", backup {}", path.display()))
        .unwrap_or_default();
    format!("{what}: {did} {}{backup}", written.resolved.display())
}

/// What the run did, grouped by where it landed.
///
/// A dotfiles-managed machine does not have one answer to "where does this edit
/// go": verified on a real home-manager setup, some targets resolve into a
/// versioned checkout and others into unmanaged `$HOME`, at the same time. For
/// a user whose other agent configs *are* versioned, an unmanaged write is
/// state that quietly does not exist on their next machine, and the one moment
/// they can act on that is while reading this. Reporting, never policy.
fn summarise(report: &Report, prompt: &dyn prompt::Interaction, options: &Options) {
    if report.lines.is_empty() {
        return;
    }
    prompt.say("\nWhat changed:");
    for line in &report.lines {
        prompt.say(&format!("  {line}"));
    }
    if let Some(line) = where_they_landed(&report.lines) {
        prompt.say(&line);
    }
    if options.steps.agents {
        prompt.say("\nAgents load their hooks at startup: restart any running session.");
    }
}

/// What to say about where the run's writes ended up, or nothing to say.
///
/// "Some of those" is false when it was all of them, and a user whose configs
/// are *all* versioned is the one who most wants to hear which sentence it is.
/// Lines that name no path at all - the plugin route installs commands rather
/// than a file - are not counted either way.
fn where_they_landed(lines: &[String]) -> Option<String> {
    let landed: Vec<&Path> = lines.iter().filter_map(|line| path_in_line(line)).collect();
    let versioned = landed
        .iter()
        .filter(|path| git_repo_of(path).is_some())
        .count();
    match (versioned, landed.len()) {
        (0, _) => None,
        (versioned, all) if versioned == all => {
            Some("\nEvery one of those landed in a git repository.".to_owned())
        }
        _ => Some(
            "\nSome of those landed in a git repository; the rest live only in $HOME.".to_owned(),
        ),
    }
}

/// The path a summary line names, which is its first absolute word.
///
/// A line that names two - the target and its backup - is about the target,
/// which `summary_line` prints first, and prints a comma after when a backup
/// follows it. The comma is not part of the path.
fn path_in_line(line: &str) -> Option<&Path> {
    line.split_whitespace()
        .find(|word| word.starts_with('/'))
        .map(|word| Path::new(word.trim_end_matches(',')))
}

/// Plan the agent step: choose the agents, then work out each one's write.
fn plan_agents(options: &Options, prompt: &dyn prompt::Interaction) -> Vec<Planned> {
    chosen_agents(options, prompt)
        .into_iter()
        .map(|agent| plan_agent(agent, options))
        .collect()
}

/// Which agents to register for.
///
/// Named on the command line wins outright. Otherwise every agent is listed,
/// preselected when either signal hits, so nothing is ever registered without
/// having been shown.
fn chosen_agents(
    options: &Options,
    prompt: &dyn prompt::Interaction,
) -> Vec<&'static agents::Agent> {
    if let Some(named) = &options.agents {
        return named.clone();
    }
    let rows: Vec<(String, bool)> = agents::AGENTS
        .iter()
        .map(|agent| {
            let found = agent.detect(&options.home);
            (
                format!("{} ({})", agent.label, found.why()),
                found.preselected(),
            )
        })
        .collect();
    prompt
        .choose("Which agents should get hooks?", &rows)
        .into_iter()
        .filter_map(|index| agents::AGENTS.get(index))
        .collect()
}

fn plan_agent(agent: &'static agents::Agent, options: &Options) -> Planned {
    Planned {
        step: Step::Agents,
        what: agent.label.to_owned(),
        action: plan_agent_action(agent, options),
    }
}

fn plan_agent_action(agent: &'static agents::Agent, options: &Options) -> Action {
    if agent.delivery == agents::Delivery::Plugin
        && options.claude_route != agents::ClaudeRoute::Settings
    {
        match &options.claude {
            Some(claude) => return plan_plugin(claude, options),
            // The plugin route was never available, so this is not a failure:
            // fall through to the merge the plan names as the fallback.
            None if options.claude_route == agents::ClaudeRoute::Plugin => {
                return Action::Failed(
                    "--claude-route=plugin was asked for, but `claude` is not on PATH".to_owned(),
                );
            }
            None => {}
        }
    }
    plan_merge(agent, options)
}

fn plan_plugin(claude: &agents::Claude, options: &Options) -> Action {
    let marketplace = options
        .marketplace
        .clone()
        .unwrap_or_else(agents::default_marketplace);
    match claude.plugin_installed() {
        // Idempotency stops here: it must not go on to check where the
        // marketplace points, and must never re-add it to "correct" it.
        Some(true) => Action::AlreadyRegistered,
        Some(false) => Action::Plugin {
            claude: claude.clone(),
            marketplace,
        },
        None => Action::Failed(
            "`claude plugin list --json` could not be read.\n  \
             To merge into ~/.claude/settings.json instead, re-run with \
             --claude-route=settings."
                .to_owned(),
        ),
    }
}

fn plan_merge(agent: &'static agents::Agent, options: &Options) -> Action {
    let target = agent.target(&options.home);
    let seen = match write::inspect(&target, &write::Faults::from_env()) {
        Ok(seen) => seen,
        Err(error) => return Action::Manual(indented(&error)),
    };
    match agent.merge(&seen.contents) {
        // Verified as a real setup: a user who copied the shipped drop-in by
        // hand before this subcommand existed has the hooks and no markers.
        // Only a marked block is ours to rewrite or, later, to remove.
        Ok(write::Plan::AlreadyRegistered) if agent.present_unmarked(&seen.contents) => {
            Action::Adopt(Box::new(Change {
                what: agent.label.to_owned(),
                preview: agent.adopt(&seen.contents),
                rebuild: Rebuild::Adopt(agent),
                parses: agent.parses(),
                creating: false,
                notes: agent_notes(agent, &seen),
                verify: None,
                path: seen.resolved,
                announced: false,
            }))
        }
        Ok(write::Plan::AlreadyRegistered) => Action::AlreadyRegistered,
        Ok(write::Plan::Write(preview)) => Action::Write(Box::new(Change {
            what: agent.label.to_owned(),
            preview,
            rebuild: Rebuild::Agent(agent),
            parses: agent.parses(),
            creating: !seen.exists,
            notes: agent_notes(agent, &seen),
            verify: None,
            path: seen.resolved,
            announced: false,
        })),
        // The step does not fail and nothing is written: the user is left
        // exactly where they were, holding the block they need.
        Err(why) => Action::Manual(format!(
            "    {why}\n    Add this to {} by hand:\n{}",
            seen.resolved.display(),
            indent(agent.contents)
        )),
    }
}

fn agent_notes(agent: &'static agents::Agent, seen: &write::Inspection) -> Vec<String> {
    let mut notes = Vec::new();
    if seen.resolved != seen.named {
        notes.push(format!(
            "{} resolves to {}",
            seen.named.display(),
            seen.resolved.display()
        ));
    }
    if let Some(repo) = git_repo_of(&seen.resolved) {
        notes.push(format!(
            "that is inside the git repository at {}",
            repo.display()
        ));
    }
    // A line merely mentioning us that is neither an entry we manage nor one
    // we would replace - a comment, or a wrapper of the user's own - is a
    // warning to review, never a silent skip.
    if agent.present_unmarked(&seen.contents) {
        notes.push(
            "these hooks are already there, without markers. Adopting them wraps them in a \
             block this tool manages and changes nothing about what they do; declining leaves \
             the file exactly as it is, and the hooks are registered either way."
                .to_owned(),
        );
    }
    if agent.name == "devin" {
        notes.push(format!(
            "project scope lives in {} and is not written here",
            agents::devin_project_file().display()
        ));
    }
    notes
}

fn indent(text: &str) -> String {
    text.lines().map(|line| format!("      {line}\n")).collect()
}

/// What the tmux steps found out before either of them planned anything.
struct TmuxPlan {
    config: tmux_conf::Choice,
    /// The dump of the config as it stands, or `None` when there is no tmux.
    baseline: Option<probe::Dump>,
    /// What tmux said about the config as it stands, when it would not read
    /// it. Set means the user's config was already broken before we arrived.
    pre_broken: Option<String>,
}

impl TmuxPlan {
    fn detect(options: &Options, prompt: &dyn prompt::Interaction) -> TmuxPlan {
        let config = tmux_conf::discover_config(options.tmux_config.as_deref(), &options.home);

        // Said as part of the plan's own heading rather than on a line of its
        // own, so that there is no line to print when there is nothing to say.
        prompt.say(&format!(
            "{}This is what register would do:",
            system_wide_note(probe::config_files().as_deref(), |path| path.exists()) // coverage: off: needs a real tmux with a present /etc/tmux.conf to reach
        ));

        // The baseline is what makes the probe honest, and it is taken before
        // anything is written.
        let baseline = match options.probe && config.path().exists() {
            true => probe::dump(config.path()),
            false => None,
        };
        let pre_broken = match options.probe && config.path().exists() {
            true => probe::check(config.path()).and_then(Result::err),
            false => None,
        };

        TmuxPlan {
            config,
            baseline,
            pre_broken,
        }
    }

    /// The refusal both tmux steps share when the config was already broken.
    ///
    /// Editing it would produce a registration nobody could validate, and the user
    /// would reasonably blame the tool that touched the file last for a
    /// breakage it inherited.
    fn refusal(&self) -> Option<Action> {
        self.pre_broken.as_ref().map(|complaint| {
            Action::Manual(
                [
                    format!(
                        "    tmux will not read {} as it stands:",
                        self.config.path().display()
                    ),
                    format!("      {complaint}"),
                    "    tmux discards a config it cannot parse *whole*, so nothing in that"
                        .to_owned(),
                    "    file is in effect right now. Fixing that comes first.".to_owned(),
                ]
                .join("\n"),
            )
        })
    }

    fn plan_hook(&self, options: &Options) -> Vec<Planned> {
        if let Some(refusal) = self.refusal() {
            return vec![Planned {
                step: Step::TmuxHook,
                what: Step::TmuxHook.title().to_owned(),
                action: refusal,
            }];
        }
        // The copy the config already sources is the one tmux reads, whatever
        // this run would otherwise have discovered. An install route that left
        // a copy of its own - which is every one but a package manager - leaves
        // an upgrade nothing else to notice: the source-file line is already
        // there, so without this the run reports the step done and the hooks
        // stay the ones the old copy sets.
        let installed = self
            .contents()
            .and_then(|text| tmux_conf::sourced_snippet(&text, &options.home));
        let snippet = match &installed {
            Some(Ok(sourced)) => tmux_conf::Choice::Existing(sourced.path.clone()),
            // The line is there either way, so the source-file step only has
            // to see that it is; the file it names is the snippet step's
            // question.
            Some(Err(_)) | None => tmux_conf::discover_snippet(
                options.snippet.as_deref(),
                options.exe.as_deref(),
                self.config.path(),
                &options.home,
                options.prefix.as_deref(),
            ),
        };
        let mut planned = Vec::new();
        // The option is said once: by the snippet step when this run writes the
        // snippet, and by the source-file step when it points at one that
        // exists. A config that already sources one never reaches the second
        // half of that sentence: the line is there, so the source-file step
        // says nothing either way.
        let disclosed = matches!(snippet, tmux_conf::Choice::Create(_));

        // The snippet first: a source-file line pointing at nothing is worse
        // than no line at all.
        match &installed {
            Some(Ok(sourced)) => planned.push(Planned {
                step: Step::TmuxHook,
                what: "the tmux snippet".to_owned(),
                action: self.plan_installed_snippet(sourced, options.snippet.as_deref()),
            }),
            Some(Err(argument)) => planned.push(Planned {
                step: Step::TmuxHook,
                what: "the tmux snippet".to_owned(),
                action: Action::Manual(indented(&format!(
                    "the source-file line in {} names {argument}, and a variable in it has no \
                     value here, so the file tmux reads is not known to this run.\n\
                     tmux expands it from the environment its server started with. Check that \
                     file is this version's snippet, or spell the path out and run this again.",
                    self.config.path().display()
                ))),
            }),
            None => {
                if let tmux_conf::Choice::Create(path) = &snippet {
                    planned.push(Planned {
                        step: Step::TmuxHook,
                        what: "the tmux snippet".to_owned(),
                        // Inspected like every other target, so that somewhere we
                        // cannot write is handed back while nothing has been written,
                        // rather than failing halfway through the step.
                        action: match write::inspect(path, &write::Faults::from_env()) {
                            Err(error) => Action::Manual(indented(&error)),
                            Ok(seen) => Action::Write(Box::new(Change {
                                what: "the tmux snippet".to_owned(),
                                preview: tmux_conf::SNIPPET.to_owned(),
                                rebuild: Rebuild::Whole(tmux_conf::SNIPPET.to_owned()),
                                parses: not_empty,
                                creating: !seen.exists,
                                notes: vec![
                                    "no shipped copy was found, so one is written here".to_owned(),
                                    FOCUS_EVENTS_NOTE.to_owned(),
                                ],
                                verify: None,
                                path: seen.resolved,
                                announced: false,
                            })),
                        },
                    });
                }
            }
        }

        // A source-file line pointing at nothing is worse than no line at all,
        // so the line only goes in if the thing it points at will be there.
        // A config that already sources one is past that question: the line is
        // there, whatever this run could or could not do to the file it names.
        let snippet_refused = installed.is_none()
            && planned
                .iter()
                .any(|item| matches!(item.action, Action::Manual(_)));
        planned.push(Planned {
            step: Step::TmuxHook,
            what: Step::TmuxHook.title().to_owned(),
            action: match snippet_refused {
                true => Action::Manual(
                    "    not adding a source-file line, because the snippet it would point \
                     at could not be written."
                        .to_owned(),
                ),
                false => self.plan_source_line(snippet.path(), disclosed),
            },
        });
        planned
    }

    /// What to say about a sourced snippet that is out of date and that this
    /// run cannot write.
    ///
    /// The refusal on its own leaves the user where they started: the hooks
    /// tmux runs are an older copy's, and "read-only" does not say what to do
    /// about that. Both ways out are named, because which one is right depends
    /// on who owns the file - a package manager's copy is upgraded through the
    /// package, and a path of your own is rewritten by the next run.
    ///
    /// The second way is spelled as an edit to the source-file line rather than
    /// as a flag, because that is what actually works: the line names the file
    /// tmux reads, and this run writes whatever it names.
    fn stale_and_unwritable(&self, snippet: &Path, error: &dyn fmt::Display) -> String {
        indented(&format!(
            "{error}\n\
             It is also not the snippet this version ships, so the hooks tmux runs are the \
             ones that copy sets.\n\
             Either update whatever provides {}, or point the source-file line in {} at a \
             path of your own and run this again: register writes this version's snippet \
             wherever that line points.",
            snippet.display(),
            self.config.path().display()
        ))
    }

    /// The tmux config's bytes, or `None` when it cannot be read.
    ///
    /// Read rather than remembered, for the reason `Change::rebuild` exists:
    /// the file is the only thing that knows what is in it.
    fn contents(&self) -> Option<String> {
        write::inspect(self.config.path(), &write::Faults::from_env())
            .ok()
            .map(|seen| seen.contents)
    }

    /// Bring the snippet the config already sources up to this version.
    ///
    /// The whole file is ours, so the comparison is the whole file: anything
    /// but our exact bytes is an older copy, a hand-edited one, or one from an
    /// install route that ships something else - and all three answer the same
    /// question, which is whether the hooks tmux runs are the hooks this build
    /// means to register. Identical bytes are the common case and ask nothing.
    ///
    /// A copy a package manager owns is read-only, so `inspect` refuses it and
    /// names the path; that is the whole of the report the user needs, because
    /// the fix is to upgrade the package rather than to write over its store.
    ///
    /// Two things about the path itself are said out loud rather than assumed,
    /// because both mean the file written may not be the file the user had in
    /// mind: a `--snippet` naming somewhere else, and an argument tmux resolved
    /// against a working directory this process cannot know.
    fn plan_installed_snippet(
        &self,
        sourced: &tmux_conf::Sourced,
        explicit: Option<&Path>,
    ) -> Action {
        let seen = match write::inspect(&sourced.path, &write::Faults::from_env()) {
            Ok(seen) => seen,
            // A copy we cannot write is only a problem if it is the wrong one.
            // A package manager's copy that already matches this build is the
            // ordinary, healthy case - tmux runs the right hooks either way -
            // and reporting a read-only directory there would be a complaint
            // about nothing.
            Err(error) => {
                let current = std::fs::read_to_string(&sourced.path).unwrap_or_default();
                return match current == tmux_conf::SNIPPET {
                    true => Action::AlreadyRegistered,
                    false => Action::Manual(self.stale_and_unwritable(&sourced.path, &error)),
                };
            }
        };
        if seen.contents == tmux_conf::SNIPPET {
            // Nothing is written anywhere, so neither disclosure has anything
            // to be about: the file tmux reads is already this version's.
            return Action::AlreadyRegistered;
        }
        let mut notes = vec![
            "the copy this config sources is not the one this version ships, so it is \
             replaced whole; your own tmux config is untouched"
                .to_owned(),
        ];
        if let Some(explicit) = explicit.filter(|path| *path != sourced.path) {
            notes.push(format!(
                "--snippet names {}, but this config sources {}, which is the file tmux \
                 reads and so the one replaced",
                explicit.display(),
                sourced.path.display()
            ));
        }
        if sourced.relative {
            notes.push(format!(
                "the source-file line names a relative path, which tmux resolves against the \
                 directory its server was started in; this run resolved it against $HOME, to \
                 {}, and a server started elsewhere reads a different file",
                sourced.path.display()
            ));
        }
        notes.push(FOCUS_EVENTS_NOTE.to_owned());
        Action::Write(Box::new(Change {
            what: "the tmux snippet".to_owned(),
            preview: tmux_conf::SNIPPET.to_owned(),
            rebuild: Rebuild::Whole(tmux_conf::SNIPPET.to_owned()),
            parses: not_empty,
            creating: !seen.exists,
            notes,
            verify: self.verification(Landed::Hooks {
                snippet: sourced.path.clone(),
            }),
            path: seen.resolved,
            announced: false,
        }))
    }

    fn plan_source_line(&self, snippet: &Path, disclosed: bool) -> Action {
        let seen = match write::inspect(self.config.path(), &write::Faults::from_env()) {
            Ok(seen) => seen,
            Err(error) => return Action::Manual(indented(&error)),
        };
        if tmux_conf::sources_snippet(&seen.contents) {
            return Action::AlreadyRegistered;
        }
        // A path no quoting can carry is handed back before anything is
        // written: a `source-file` line tmux cannot read costs the user the
        // whole config file, not just our glyph.
        let Some(preview) = tmux_conf::with_source_block(&seen.contents, snippet) else {
            return Action::Manual(format!("    {}", unspellable(snippet)));
        };
        Action::Write(Box::new(Change {
            what: Step::TmuxHook.title().to_owned(),
            preview,
            rebuild: Rebuild::SourceBlock(snippet.to_path_buf()),
            parses: not_empty,
            creating: !seen.exists,
            notes: match disclosed {
                true => Vec::new(),
                false => vec![FOCUS_EVENTS_NOTE.to_owned()],
            },
            verify: self.verification(Landed::Hooks {
                snippet: snippet.to_path_buf(),
            }),
            path: seen.resolved,
            announced: false,
        }))
    }

    /// Plan the format step, per option.
    ///
    /// Both options need the term - a term in only one of them makes the glyph
    /// vanish the moment the window becomes current - and a config may set
    /// them on different lines, in different files, or not at all. So the
    /// winning assignment is found *per option*, not once for the file: taking
    /// the last assignment overall edits whichever of the two happens to come
    /// second and silently leaves the other bare.
    fn plan_format(&self, options: &Options, prompt: &dyn prompt::Interaction) -> Vec<Planned> {
        if let Some(refusal) = self.refusal() {
            return vec![Planned {
                step: Step::TmuxFormat,
                what: Step::TmuxFormat.title().to_owned(),
                action: refusal,
            }];
        }
        let found = tmux_conf::walk(self.config.path());
        let winners: Vec<Option<&tmux_conf::Assignment>> = format::OPTIONS
            .iter()
            .map(|option| {
                found
                    .assignments
                    .iter()
                    .rfind(|found| found.option() == Some(option))
            })
            .collect();
        let ordering = ordering_notes(&found.relative_sources);

        // Neither is assigned: the user is on tmux's compiled-in default, and
        // one marked block sets both.
        if winners.iter().all(Option::is_none) {
            return vec![Planned {
                step: Step::TmuxFormat,
                what: Step::TmuxFormat.title().to_owned(),
                action: self.plan_new_pair(options, &ordering),
            }];
        }

        format::OPTIONS
            .iter()
            .zip(winners)
            .map(|(option, winner)| Planned {
                step: Step::TmuxFormat,
                what: format!("the term in {option}"),
                action: match winner {
                    Some(winner) => self.plan_splice(option, winner, prompt, &ordering),
                    // One is assigned and the other is not, so the bare one
                    // gets a line of its own spliced from tmux's default.
                    None => self.plan_one_line(option, options, &ordering),
                },
            })
            .collect()
    }

    /// The default this tmux would use, asked rather than remembered, with the
    /// term already in it.
    ///
    /// `None` when the value cannot be quoted safely, which sends the step to
    /// the manual path like any other value this tool will not write.
    fn spliced_default(&self, options: &Options, option: &str) -> Option<String> {
        let found = match options.probe {
            true => probe::compiled_in_default(),
            false => None,
        }
        .unwrap_or_else(|| format::FALLBACK_DEFAULT.to_owned());
        let found = match faulty("odd-default") {
            // A default no quoting can carry. No tmux produces one today; a
            // future one might, and the step must hand it back rather than
            // write a line tmux discards.
            true => "it's $a `b` \\c".to_owned(),
            false => found,
        };
        format::assignment(option, &format::splice(&found))
    }

    /// Append a line for an option nothing assigns.
    fn plan_one_line(&self, option: &str, options: &Options, ordering: &[String]) -> Action {
        let Some(line) = self.spliced_default(options, option) else {
            return self.manual_term("    this tmux's default format cannot be quoted safely");
        };
        let seen = match write::inspect(self.config.path(), &write::Faults::from_env()) {
            Ok(seen) => seen,
            Err(error) => return self.manual_term(&indented(&error)),
        };
        Action::Write(Box::new(Change {
            what: format!("the term in {option}"),
            preview: append_marked(&seen.contents, &line),
            rebuild: Rebuild::FormatOne {
                option: option.to_owned(),
                line: line.clone(),
            },
            parses: not_empty,
            creating: !seen.exists,
            notes: {
                let mut notes = vec![format!("nothing assigns {option}, so a line is added")];
                notes.push(format!("  {}", line.trim_end()));
                notes.extend(ordering.iter().cloned());
                notes
            },
            verify: self.verification(TmuxPlan::term_in(option)),
            path: seen.resolved,
            announced: false,
        }))
    }

    /// Splice the term into the winning assignment of one option.
    ///
    /// The last assignment in tmux's own order is the one that wins, and so the
    /// one to edit. Editing any earlier one produces a line tmux discards:
    /// a successful-looking registration with no glyph and nothing in the diff.
    fn plan_splice(
        &self,
        option: &str,
        last: &tmux_conf::Assignment,
        prompt: &dyn prompt::Interaction,
        ordering: &[String],
    ) -> Action {
        let Some(line) = last.candidate.clone().line() else {
            let why = last
                .candidate
                .clone()
                .refusal()
                .map(format::Refusal::reason)
                .unwrap_or("the line cannot be read");
            return self.manual_term(&format!(
                "    {} line {}: {why}",
                last.file.display(),
                last.line.first + 1
            ));
        };
        if line.already_registered() {
            // Wherever the user put the term, they put it there on purpose.
            return Action::AlreadyRegistered;
        }
        let Some(rewritten) = line.spliced_line() else {
            return self.manual_term(&format!(
                "    {} line {}: the value cannot be requoted safely",
                last.file.display(),
                last.line.first + 1
            ));
        };
        // Checked before anything is announced or asked: a file we cannot
        // write makes both the announcement and the edit question moot, and
        // the winning line lives in a file we may not write. Editing an
        // earlier one would produce a line tmux discards, so the step
        // reports rather than editing a loser.
        let seen = match write::inspect(&last.file, &write::Faults::from_env()) {
            Ok(seen) => seen,
            Err(error) => return self.manual_term(&indented(&error)),
        };

        // The full before/after, shown once, right where the edit question is
        // asked, naming the same path `confirm` and the writer use. `confirm`
        // only asks the bare write question for this change - see
        // `announced` below - so this is the only place any of it is said.
        prompt.say(&format!(
            "  the term in {option} - edit {}",
            seen.resolved.display()
        ));
        prompt.say(&format!("      before: {}", last.line.text));
        prompt.say(&format!("      after:  {rewritten}"));
        for note in ordering {
            prompt.say(&format!("    {note}"));
        }
        let proposed = rewritten;
        let rewritten = self.offer_edit(proposed.clone(), prompt);
        if rewritten != proposed {
            prompt.say(&format!("      after your edit: {rewritten}"));
        }
        Action::Write(Box::new(Change {
            what: format!("the term in {option}"),
            preview: tmux_conf::replace_lines(
                &seen.contents,
                last.line.first,
                last.line.last,
                &rewritten,
            ),
            rebuild: Rebuild::FormatLine {
                reported: last.line.first,
                was: last.line.text.clone(),
                // The fault hands the writer a deliberately malformed splice -
                // a stray argument after the value, which is what a quoting
                // bug produces. tmux abandons the whole config over it, and
                // the probe is the only thing that can see that.
                with: match faulty("bad-splice") {
                    true => format!("{rewritten} stray-argument"),
                    false => rewritten.clone(),
                },
            },
            parses: not_empty,
            creating: false,
            // Nothing left for `confirm` to say: the header, before/after and
            // ordering hint were all said above, next to the edit question.
            notes: Vec::new(),
            verify: self.verification(TmuxPlan::term_in(option)),
            path: seen.resolved,
            announced: true,
        }))
    }

    /// No format line at all: the user is on tmux's compiled-in default.
    fn plan_new_pair(&self, options: &Options, ordering: &[String]) -> Action {
        // Asked rather than remembered: the default has changed between tmux
        // versions and the one in *this* tmux is the only one that is right.
        let block: Option<String> = format::OPTIONS
            .iter()
            .map(|option| self.spliced_default(options, option))
            .collect();
        let Some(block) = block else {
            return self.manual_term("    this tmux's default format cannot be quoted safely");
        };
        let seen = match write::inspect(self.config.path(), &write::Faults::from_env()) {
            Ok(seen) => seen,
            Err(error) => return self.manual_term(&indented(&error)),
        };
        Action::Write(Box::new(Change {
            what: Step::TmuxFormat.title().to_owned(),
            preview: append_marked(&seen.contents, &block),
            rebuild: Rebuild::FormatPair(block.clone()),
            parses: not_empty,
            creating: !seen.exists,
            notes: {
                let mut notes = vec![
                    "no format line was found, so a new pair is written for both options"
                        .to_owned(),
                ];
                notes.extend(block.lines().map(|line| format!("  {line}")));
                notes.extend(ordering.iter().cloned());
                notes
            },
            verify: self.verification(Landed::Term(
                format::OPTIONS.iter().map(|o| (*o).to_owned()).collect(),
            )),
            path: seen.resolved,
            announced: false,
        }))
    }

    /// Offer to edit the proposed line, and re-check whatever comes back.
    fn offer_edit(&self, proposed: String, prompt: &dyn prompt::Interaction) -> String {
        if !prompt.confirm(
            "Accept the proposed line? Choose 'no' for manual edit",
            true,
        ) {
            if let Some(edited) = prompt.edit(&proposed) {
                let edited = edited.trim_end().to_owned();
                if format::references_agent_status(&edited) {
                    return edited;
                }
                prompt.say("  that no longer references @agent_status; keeping the proposal.");
            }
        }
        proposed
    }

    fn verification(&self, landed: Landed) -> Option<TmuxVerify> {
        Some(TmuxVerify {
            entry: self.config.path().to_path_buf(),
            baseline: self.baseline.clone()?,
            expected: ours_to_change(),
            landed,
        })
    }

    /// The landing check for one option's edit.
    fn term_in(option: &str) -> Landed {
        Landed::Term(vec![option.to_owned()])
    }

    /// The manual path: print the term and what we found, and report the step
    /// as not registered. The run's exit code stays 0, because a refusal the
    /// user chose is not an error - and this is the state they are in today,
    /// so falling into it leaves them no worse off than before they ran
    /// anything.
    fn manual_term(&self, why: &str) -> Action {
        Action::Manual(format!(
            "{why}\n    Add this term to both {} and {}, after the name segment:\n      {}",
            format::OPTIONS[0],
            format::OPTIONS[1],
            format::TERM
        ))
    }
}

/// Offer the reload, which is the same command the user would type.
///
/// Never `set-option`. By this point the probe has already loaded that exact
/// file in a throwaway server and found it sound, so the reload is offered on a
/// file that is known to parse rather than hoped to.
pub fn offer_reload(config: &Path, listed: Option<&str>, prompt: &dyn prompt::Interaction) {
    let Some(listed) = listed else {
        // No running server, so there is nothing to reload into.
        return;
    };
    // Only a config that server would load. Sourcing anything else into it
    // applies settings the user never asked that tmux to have - which is a
    // real hazard with `--tmux-config`, and how a scratch config ends up in
    // somebody's live session.
    let loaded = tmux_conf::candidates(listed)
        .iter()
        .any(|candidate| same_file(candidate, config));
    if !loaded {
        prompt.say(&format!(
            "\nNot offering a reload: the running tmux does not load {}.\n\
             Start a new server, or source it yourself if that is what you meant.",
            config.display()
        ));
        return;
    }
    if prompt.confirm(
        &format!("Reload tmux now (tmux source-file {})?", config.display()),
        true,
    ) {
        let reloaded = match faulty("bad-reload") {
            true => Err(std::io::Error::other("tmux refused the reload")),
            false => probe::reload(config),
        };
        match reloaded {
            Ok(()) => prompt.say("  tmux reloaded."),
            Err(error) => prompt.say(&format!("  {error}")),
        }
    } else {
        prompt.say(&format!(
            "  Not reloaded. Run `tmux source-file {}` when you are ready.",
            config.display()
        ));
    }
}

/// Whether two paths name the same file, following symlinks where they exist.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        // A candidate tmux named that does not exist cannot be the one we
        // edited, because we only edit files we could resolve.
        _ => a == b,
    }
}

/// The git repository a path sits in, if any.
///
/// Reporting, never policy. For a user whose other agent configs are versioned,
/// a write into unmanaged `$HOME` is state that quietly does not exist on their
/// next machine, and the one moment they can act on that is while reading the
/// summary.
pub fn git_repo_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_block_is_appended_with_its_own_newlines() {
        assert_eq!(
            append_marked("set -g status on\n", "source-file x"),
            "set -g status on\n# >>> tmux-agent-status >>>\nsource-file x\n# <<< tmux-agent-status <<<\n"
        );
    }

    #[test]
    fn a_file_not_ending_in_a_newline_gains_one_first() {
        // Otherwise the marker lands on the tail of the user's last line and
        // takes that line's command with it.
        let out = append_marked("set -g status on", "source-file x");
        assert!(out.starts_with("set -g status on\n# >>>"), "{out}");
    }

    #[test]
    fn an_empty_file_gains_no_leading_blank_line() {
        assert!(append_marked("", "x").starts_with(MARKER_START));
    }

    #[test]
    fn a_body_that_already_ends_in_a_newline_does_not_gain_a_second() {
        assert_eq!(
            append_marked("", "one\ntwo\n"),
            append_marked("", "one\ntwo")
        );
    }

    #[test]
    fn a_marked_block_is_recognised_wherever_it_sits() {
        let text = append_marked("set -g status on\n", "source-file x");
        assert!(has_marked_block(&text));
        assert!(has_marked_block("  # >>> tmux-agent-status >>>  \n"));
        assert!(!has_marked_block("set -g status on\n"));
        assert!(!has_marked_block(
            "# a comment mentioning tmux-agent-status\n"
        ));
    }

    #[test]
    fn every_step_names_its_flag_and_itself() {
        for step in Step::ALL {
            assert!(step.flag().starts_with("--"), "{step:?}");
            assert!(!step.title().is_empty(), "{step:?}");
        }
        assert_eq!(Step::Agents.flag(), "--agents");
        assert_eq!(Step::TmuxHook.flag(), "--tmux-hook");
        assert_eq!(Step::TmuxFormat.flag(), "--tmux-format");
    }

    #[test]
    fn the_step_algebra_is_the_table_the_plan_gives() {
        let all = Steps {
            agents: true,
            tmux_hook: true,
            tmux_format: true,
        };
        // Nothing said: all three.
        assert_eq!(select(&[], &[]).unwrap(), all);
        // One or more positive: exactly those.
        assert_eq!(
            select(&[Step::Agents], &[]).unwrap(),
            Steps {
                tmux_hook: false,
                tmux_format: false,
                ..all
            }
        );
        assert_eq!(
            select(&[Step::TmuxHook, Step::TmuxFormat], &[]).unwrap(),
            Steps {
                agents: false,
                ..all
            }
        );
        // Only negative: all three minus those.
        assert_eq!(
            select(&[], &[Step::Agents]).unwrap(),
            Steps {
                agents: false,
                ..all
            }
        );
        // A positive and a negative: a usage error.
        assert!(select(&[Step::Agents], &[Step::TmuxHook]).is_err());

        for step in Step::ALL {
            assert!(all.has(step));
            assert!(!select(&[], &[step]).unwrap().has(step));
        }
    }

    #[test]
    fn a_mixed_step_error_names_both_flags() {
        let message = select(&[Step::Agents], &[Step::TmuxFormat]).unwrap_err();
        assert!(message.contains("--agents"), "{message}");
        assert!(message.contains("--no-tmux-format"), "{message}");
    }

    #[test]
    fn a_step_reports_the_worst_of_what_its_targets_did() {
        // The agent step has one target per agent, and the summary has to say
        // something true about all of them at once.
        let mut report = Report::default();
        report.record(Step::Agents, Outcome::AlreadyRegistered);
        assert_eq!(
            report.outcome(Step::Agents),
            Some(Outcome::AlreadyRegistered)
        );
        report.record(Step::Agents, Outcome::Registered);
        assert_eq!(report.outcome(Step::Agents), Some(Outcome::Registered));
        report.record(Step::Agents, Outcome::NotRegistered);
        assert_eq!(report.outcome(Step::Agents), Some(Outcome::NotRegistered));
        report.record(Step::Agents, Outcome::Failed);
        assert_eq!(report.outcome(Step::Agents), Some(Outcome::Failed));

        assert_eq!(report.outcome(Step::TmuxHook), None);
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn a_dry_run_exits_zero_even_when_it_could_not_plan_a_step() {
        // Nothing ran, so nothing failed: what a dry run prints is a plan, and
        // a step it could not plan is part of that plan. Exiting 1 told a
        // caller a file had been touched and put back, which is the one thing
        // that certainly did not happen.
        let mut report = Report {
            dry_run: true,
            ..Report::default()
        };
        report.record(Step::Agents, Outcome::Failed);
        assert_eq!(report.exit_code(), 0);
        // And the outcome is still there, because the plan is printed from it.
        assert_eq!(report.outcome(Step::Agents), Some(Outcome::Failed));

        // The same outcomes in a real run are what exit 1 is for.
        report.dry_run = false;
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn a_run_that_registered_or_was_already_registered_exits_zero() {
        let mut report = Report::default();
        for outcome in [
            Outcome::Registered,
            Outcome::AlreadyRegistered,
            // A refusal the user chose is not an error.
            Outcome::NotRegistered,
        ] {
            report.record(Step::TmuxFormat, outcome);
        }
        assert_eq!(report.exit_code(), 0);
        assert_eq!(Report::default().exit_code(), 0);
    }

    #[test]
    fn every_rebuild_is_a_pure_function_of_what_is_on_disk() {
        // Whole: ours, so replaced, then left alone.
        let whole = Rebuild::Whole("ours\n".to_owned());
        assert_eq!(
            whole.apply("theirs\n"),
            Ok(write::Plan::Write("ours\n".to_owned()))
        );
        assert_eq!(whole.apply("ours\n"), Ok(write::Plan::AlreadyRegistered));

        // A snippet path no tmux quoting can carry is a refusal rather than a
        // line that makes tmux abandon the config file it sits in.
        assert!(
            Rebuild::SourceBlock(PathBuf::from("/it's $HOME/tmux-agent-status.conf"))
                .apply("set -g status on\n")
                .is_err()
        );

        // The source block, which must see the file as it is at apply time.
        let block = Rebuild::SourceBlock(PathBuf::from("/x/tmux-agent-status.conf"));
        let out = block
            .apply("set -g status on\n")
            .unwrap()
            .written()
            .unwrap();
        assert!(out.contains("source-file /x/tmux-agent-status.conf"));
        assert_eq!(block.apply(&out), Ok(write::Plan::AlreadyRegistered));

        // The new pair.
        let pair =
            Rebuild::FormatPair("set -g window-status-format 'x#{?@agent_status,y,}'\n".to_owned());
        let out = pair.apply("").unwrap().written().unwrap();
        assert!(has_marked_block(&out));
        assert_eq!(pair.apply(&out), Ok(write::Plan::AlreadyRegistered));

        // An agent merge, which is the agent's own function.
        let codex = Rebuild::Agent(agents::by_name("codex").expect("a row"));
        assert!(codex.apply("").unwrap().written().is_some());
        assert!(codex.apply("not json").is_err());
    }

    #[test]
    fn a_format_line_that_moved_under_us_is_not_rewritten() {
        let rebuild = Rebuild::FormatLine {
            reported: 1,
            was: "set -g window-status-format 'x'".to_owned(),
            with: "set -g window-status-format 'x!'".to_owned(),
        };
        let unchanged = "set -g status on\nset -g window-status-format 'x'\n";
        assert_eq!(
            rebuild.apply(unchanged).unwrap().written().unwrap(),
            "set -g status on\nset -g window-status-format 'x!'\n"
        );

        // Another step wrote the file and the line is gone. Nothing is
        // written, because the cost of being wrong is a line tmux discards in
        // silence.
        assert!(rebuild.apply("set -g status on\n").is_err());
        assert!(rebuild.apply("one\ntwo\nthree\n").is_err());

        // But a line that merely moved is still the line we read. The other
        // format target writes this same file first, and collapsing a run of
        // continuations above this one shifts every number below it.
        assert_eq!(
            rebuild
                .apply("set -g status on\nset -g other 'y'\nset -g window-status-format 'x'\n")
                .unwrap()
                .written()
                .unwrap(),
            "set -g status on\nset -g other 'y'\nset -g window-status-format 'x!'\n"
        );
    }

    #[test]
    fn a_line_spread_over_continuations_is_still_the_line_we_read() {
        // The plan records the *logical* line, which `logical_lines` joins with
        // its backslashes and newlines removed. Matching that against one
        // physical line can never succeed, and the whole continuation path -
        // parsed, spliced, collapsed by `replace_lines` - was unreachable
        // because of it: the step failed with "the config moved under us" on a
        // file nothing had touched.
        let rebuild = Rebuild::FormatLine {
            reported: 1,
            was: "set -g window-status-format   'x'".to_owned(),
            with: "set -g window-status-format 'x!'".to_owned(),
        };
        let wrapped =
            "set -g status on\nset -g window-status-format \\\n  'x'\nset -g status-left 'L'\n";
        assert_eq!(
            rebuild.apply(wrapped).unwrap().written().unwrap(),
            "set -g status on\nset -g window-status-format 'x!'\nset -g status-left 'L'\n"
        );
    }

    #[test]
    fn the_system_config_is_reported_and_never_chosen() {
        // It needs root, and it would register this for every user of the
        // machine, which is not what anyone typing the command meant.
        let note = system_wide_note(Some("/etc/tmux.conf,~/.tmux.conf"), |_| true);
        assert!(note.contains("/etc/tmux.conf"), "{note}");
        assert!(note.contains("--tmux-config"), "{note}");

        // A config file tmux names but that is not there is nothing to say.
        assert!(system_wide_note(Some("/etc/tmux.conf"), |_| false).is_empty());
        // A user config is never reported this way.
        assert!(system_wide_note(Some("~/.tmux.conf"), |_| true).is_empty());
        // And no listing at all says nothing rather than guessing.
        assert!(system_wide_note(None, |_| true).is_empty());
    }

    #[test]
    fn everything_register_may_move_is_named() {
        let ours = ours_to_change();
        for option in format::OPTIONS {
            assert!(ours.contains(&option.to_owned()), "{option}");
        }
        for hook in [
            "session-window-changed",
            "window-pane-changed",
            "pane-focus-in",
        ] {
            assert!(ours.contains(&hook.to_owned()), "{hook}");
        }
        assert!(ours.contains(&"focus-events".to_owned()));
        // And nothing else, because anything else moving is the tell that tmux
        // abandoned the config.
        assert_eq!(ours.len(), 6);
    }

    #[test]
    fn a_write_that_turned_out_to_be_a_no_op_reports_as_one() {
        // The plan reads the file before the confirmation; the merge runs
        // again inside the lock, and by then somebody else may have done it.
        assert_eq!(
            outcome_of(write::Outcome::AlreadyRegistered),
            Outcome::AlreadyRegistered
        );
        assert_eq!(outcome_of(write::Outcome::Created), Outcome::Registered);
        assert_eq!(outcome_of(write::Outcome::Edited), Outcome::Registered);
    }

    #[test]
    fn an_adoption_and_an_added_line_both_know_when_they_are_done() {
        let vibe = agents::by_name("mistral-vibe").expect("a row");
        let adopt = Rebuild::Adopt(vibe);
        let wrapped = adopt.apply(vibe.contents).unwrap().written().unwrap();
        assert!(has_marked_block(&wrapped));
        // Only a marked block is ours, so a second adoption has nothing to do.
        assert_eq!(adopt.apply(&wrapped), Ok(write::Plan::AlreadyRegistered));

        let one = Rebuild::FormatOne {
            option: format::OPTIONS[1].to_owned(),
            line: format!("set -g {} 'x{}'\n", format::OPTIONS[1], format::TERM),
        };
        // The *other* option already carrying the term is not this one being
        // done, which is the whole reason the check is per option.
        let other_done = format!("set -g {} 'x{}'\n", format::OPTIONS[0], format::TERM);
        let out = one.apply(&other_done).unwrap().written().unwrap();
        assert_eq!(one.apply(&out), Ok(write::Plan::AlreadyRegistered));
    }

    #[test]
    fn a_tmux_config_is_complete_when_it_has_anything_in_it() {
        // tmux has no notion of a truncated config, so emptiness is the only
        // truncation this can see - and it is the one that matters.
        assert!(not_empty("set -g status on\n"));
        assert!(!not_empty(""));
    }

    #[test]
    fn a_summary_line_says_what_happened_and_names_the_backup() {
        let written = write::Written {
            resolved: PathBuf::from("/home/u/.tmux.conf"),
            outcome: write::Outcome::Edited,
            backup: Some(PathBuf::from("/home/u/.tmux.conf.bak-19700101T000000Z")),
        };
        let line = summary_line("the term", &written);
        assert!(line.contains("edited"), "{line}");
        assert!(line.contains(".bak-"), "{line}");

        let created = write::Written {
            outcome: write::Outcome::Created,
            backup: None,
            ..written.clone()
        };
        assert!(summary_line("x", &created).contains("created"));
        assert!(!summary_line("x", &created).contains("backup"));

        let already = write::Written {
            outcome: write::Outcome::AlreadyRegistered,
            ..created
        };
        assert!(summary_line("x", &already).contains("already registered"));
    }

    #[test]
    fn the_summary_says_which_of_some_and_all_it_actually_was() {
        let tmp = tempfile::tempdir().expect("a temp directory");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();
        let versioned = format!("Codex CLI: edited {}", repo.join("hooks.json").display());
        let unmanaged = "Cursor: edited /nowhere/at/all/hooks.json".to_owned();
        // A line naming no path: the plugin route installs commands, not a
        // file, and it is neither versioned nor unmanaged.
        let plugin = "Claude Code: plugin installed".to_owned();

        assert_eq!(where_they_landed(&[]), None);
        assert_eq!(where_they_landed(std::slice::from_ref(&unmanaged)), None);
        assert_eq!(where_they_landed(std::slice::from_ref(&plugin)), None);

        let all = where_they_landed(&[versioned.clone(), plugin]).expect("something to say");
        assert!(all.contains("Every one of those"), "{all}");

        let some = where_they_landed(&[versioned, unmanaged]).expect("something to say");
        assert!(some.contains("Some of those"), "{some}");
    }

    #[test]
    fn a_summary_line_names_the_target_before_its_backup() {
        let line = summary_line(
            "the term",
            &write::Written {
                resolved: PathBuf::from("/home/u/.tmux.conf"),
                outcome: write::Outcome::Edited,
                backup: Some(PathBuf::from("/home/u/.tmux.conf.bak-19700101T000000Z")),
            },
        );
        assert_eq!(path_in_line(&line), Some(Path::new("/home/u/.tmux.conf")));
        assert_eq!(path_in_line("nothing absolute here"), None);
    }

    #[test]
    fn a_file_under_a_git_directory_is_versioned_and_the_root_is_not() {
        // Reporting, never policy: for a user whose other agent configs are
        // versioned, an unmanaged write is state that quietly does not exist
        // on their next machine.
        let tmp = tempfile::tempdir().expect("a temp directory");
        let here = tmp.path().join("checkout");
        fs::create_dir_all(&here).unwrap();
        fs::create_dir(here.join(".git")).unwrap();
        assert_eq!(git_repo_of(&here.join("src/register/mod.rs")), Some(here));
        assert_eq!(git_repo_of(Path::new("/")), None);
    }

    #[test]
    fn two_names_for_one_file_are_the_same_file() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert!(same_file(&here.join("src/../src"), &here.join("src")));
        assert!(!same_file(&here.join("src"), &here.join("tests")));
        // Two paths that do not exist are the same only if they are written
        // the same way, which is all there is to go on.
        assert!(same_file(Path::new("/nowhere/x"), Path::new("/nowhere/x")));
        assert!(!same_file(Path::new("/nowhere/x"), Path::new("/nowhere/y")));
    }

    #[test]
    fn an_indented_block_is_readable_where_it_is_printed() {
        assert_eq!(indent("one\ntwo\n"), "      one\n      two\n");
        assert_eq!(indent(""), "");
    }

    #[test]
    fn the_config_directory_follows_xdg_when_it_is_set() {
        let home = Home {
            home: PathBuf::from("/home/u"),
            xdg_config: None,
        };
        assert_eq!(home.join(".tmux.conf"), PathBuf::from("/home/u/.tmux.conf"));
        assert_eq!(
            home.config("tmux/tmux.conf"),
            PathBuf::from("/home/u/.config/tmux/tmux.conf")
        );

        let elsewhere = Home {
            xdg_config: Some(PathBuf::from("/elsewhere")),
            ..home
        };
        assert_eq!(
            elsewhere.config("tmux/tmux.conf"),
            PathBuf::from("/elsewhere/tmux/tmux.conf")
        );
    }
}
