//! Argument dispatch and exit codes. All behaviour lives in the library.

use std::fmt;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use pico_args::Arguments;
use tmux_agent_status::command;
use tmux_agent_status::install;
use tmux_agent_status::notify;
use tmux_agent_status::state::{State, UnknownState};

/// A wrong invocation: a bug in the caller's hook config, and so a loud one.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => usage_error(&err),
    }
}

#[derive(Debug)]
enum MainError {
    Usage(String),
}

impl fmt::Display for MainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MainError::Usage(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for MainError {}

impl From<&str> for MainError {
    fn from(message: &str) -> Self {
        MainError::Usage(message.to_owned())
    }
}

impl From<String> for MainError {
    fn from(message: String) -> Self {
        MainError::Usage(message)
    }
}

impl From<pico_args::Error> for MainError {
    fn from(error: pico_args::Error) -> Self {
        MainError::Usage(error.to_string())
    }
}

impl From<UnknownState> for MainError {
    fn from(error: UnknownState) -> Self {
        MainError::Usage(error.to_string())
    }
}

fn run() -> Result<ExitCode, MainError> {
    let mut pargs = Arguments::from_vec(std::env::args_os().skip(1).collect());

    if pargs.contains(["-h", "--help"]) {
        print!("{}", help());
        return Ok(ExitCode::SUCCESS);
    }
    if pargs.contains(["-V", "--version"]) {
        println!("{}", version());
        return Ok(ExitCode::SUCCESS);
    }

    let subcommand = pargs
        .subcommand()?
        .ok_or(MainError::from("no command given"))?;

    match subcommand.as_str() {
        "set" => run_set(pargs),
        "start" => run_start(pargs),
        "reset" => run_reset(pargs),
        "finish" => run_finish(pargs),
        "clear-window" => run_clear_window(pargs),
        "notify" => run_notify(pargs),
        "install" => run_install(pargs),
        _ => {
            let free = free_strings(pargs)?;
            Err(unexpected_arguments(&subcommand, free))
        }
    }
}

fn run_set(mut pargs: Arguments) -> Result<ExitCode, MainError> {
    let pane = pane_value(&mut pargs)?;
    let json = pargs.contains("--json");
    let free = free_strings(pargs)?;

    if free.is_empty() {
        return Err(MainError::from("set requires a state"));
    }
    if free.len() > 1 {
        return Err(unexpected_arguments("set", free));
    }

    let state = free[0].parse::<State>()?;
    let pane = pane.as_deref();
    Ok(run_hook(|| command::set(state, pane), json))
}

fn run_start(mut pargs: Arguments) -> Result<ExitCode, MainError> {
    let pane = pane_value(&mut pargs)?;
    let json = pargs.contains("--json");
    reject_extra_with_prefix(pargs, "start")?;
    let pane = pane.as_deref();
    Ok(run_hook(|| command::start(pane), json))
}

fn run_reset(mut pargs: Arguments) -> Result<ExitCode, MainError> {
    let pane = pane_value(&mut pargs)?;
    let json = pargs.contains("--json");
    reject_extra_with_prefix(pargs, "reset")?;
    let pane = pane.as_deref();
    Ok(run_hook(|| command::reset(pane), json))
}

fn run_finish(mut pargs: Arguments) -> Result<ExitCode, MainError> {
    let pane = pane_value(&mut pargs)?;
    let json = pargs.contains("--json");
    reject_extra_with_prefix(pargs, "finish")?;
    let pane = pane.as_deref();
    Ok(run_hook(|| command::finish(pane), json))
}

fn run_clear_window(mut pargs: Arguments) -> Result<ExitCode, MainError> {
    // The pane is accepted as an optional positional argument because tmux
    // hooks pass `#{pane_id}`, which expands to "" when no pane is available.
    // `--pane ""` is treated as absent; a positional lets the same hook line
    // work without `--pane` having to parse an empty value.
    let pane_flag = pane_value(&mut pargs)?;
    let json = pargs.contains("--json");
    let free = free_strings(pargs)?;

    if free.len() > 1 {
        return Err(unexpected_arguments("clear-window", free));
    }

    let positional = free.first().map(|s| s.as_str());
    let pane = pane_flag.as_deref().or(positional);
    Ok(run_hook(|| command::clear_window(pane), json))
}

fn run_notify(mut pargs: Arguments) -> Result<ExitCode, MainError> {
    let from_stdin = pargs.contains("--stdin");
    let agent = pargs
        .opt_value_from_fn("--agent", |s: &str| Ok::<_, &'static str>(s.to_owned()))
        .map_err(|_| MainError::from("--agent requires a value"))?
        .ok_or(MainError::from("notify requires --agent"))?;
    let pane = pane_value(&mut pargs)?;
    let json = pargs.contains("--json");
    let free = free_strings(pargs)?;

    let payload = if from_stdin {
        if !free.is_empty() {
            return Err(unexpected_arguments("notify", free));
        }
        if std::io::stdin().is_terminal() {
            debug("notify: --stdin with a terminal is a no-op");
            if json {
                let _ = writeln!(io::stdout(), "{{}}");
            }
            return Ok(ExitCode::SUCCESS);
        }
        let mut buf = String::new();
        // A read failure here is not a hook-config error: stdin was promised
        // but could not be consumed. Treat it as an empty, unrecognised payload
        // and exit 0 so the agent is not blocked.
        let _ = std::io::stdin().read_to_string(&mut buf);
        buf
    } else {
        match free.as_slice() {
            [] => return Err(MainError::from("notify requires a payload or --stdin")),
            [payload] => payload.clone(),
            _ => return Err(unexpected_arguments("notify", free)),
        }
    };

    // Checked after the payload is consumed, not before: the agent is writing
    // into a pipe, and a payload larger than the pipe buffer would block that
    // write and then take an EPIPE on our exit. Being disabled must be
    // invisible to the agent, which means draining what it sent us.
    if is_disabled() {
        if json {
            let _ = writeln!(io::stdout(), "{{}}");
        }
        return Ok(ExitCode::SUCCESS);
    }

    match notify::dispatch(&agent, &payload) {
        Some(state) => {
            hook(command::set(state, pane.as_deref()));
        }
        None => {
            debug(&format!("notify: dropped payload for {agent}"));
        }
    };
    if json {
        let _ = writeln!(io::stdout(), "{{}}");
    }
    Ok(ExitCode::SUCCESS)
}

/// `tmux-agent-status install`.
///
/// Deliberately not routed through `run_hook`: a hook exits 0 whatever happens,
/// which is right for something an agent calls on every turn and worthless for
/// an installer. This one reports what it did and exits accordingly.
fn run_install(mut pargs: Arguments) -> Result<ExitCode, MainError> {
    let yes = pargs.contains(["-y", "--yes"]);
    let dry_run = pargs.contains("--dry-run");
    let probe = !pargs.contains("--no-tmux-probe");

    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let named = opt_value(&mut pargs, "--agents")?;
    // `--agents=codex,cursor` selects the step *and* narrows it; a bare
    // `--agents` selects the step and leaves the choice to detection.
    if named.is_some() || pargs.contains("--agents") {
        positive.push(install::Step::Agents);
    }
    if pargs.contains("--tmux-hook") {
        positive.push(install::Step::TmuxHook);
    }
    if pargs.contains("--tmux-format") {
        positive.push(install::Step::TmuxFormat);
    }
    if pargs.contains("--no-agents") {
        negative.push(install::Step::Agents);
    }
    if pargs.contains("--no-tmux-hook") {
        negative.push(install::Step::TmuxHook);
    }
    if pargs.contains("--no-tmux-format") {
        negative.push(install::Step::TmuxFormat);
    }
    let steps = install::select(&positive, &negative).map_err(MainError::Usage)?;

    let claude_route = match opt_value(&mut pargs, "--claude-route")?.as_deref() {
        None | Some("auto") => install::agents::ClaudeRoute::Auto,
        Some("plugin") => install::agents::ClaudeRoute::Plugin,
        Some("settings") => install::agents::ClaudeRoute::Settings,
        Some(other) => {
            return Err(MainError::from(format!(
                "unknown --claude-route `{other}`: valid routes are auto, plugin, settings"
            )));
        }
    };
    let marketplace = opt_value(&mut pargs, "--marketplace")?;
    let tmux_config = opt_value(&mut pargs, "--tmux-config")?.map(PathBuf::from);
    let snippet = opt_value(&mut pargs, "--snippet")?.map(PathBuf::from);
    reject_extra_with_prefix(pargs, "install")?;

    let agents = named.map(|names| parse_agents(&names)).transpose()?;
    let home = install::Home::from_env()
        .ok_or(MainError::from("install needs $HOME and there is none"))?;

    // A question with no terminal and no `-y` is a usage error rather than a
    // guess: a tool that edits configs unattended is a tool nobody asked to
    // run.
    let prompt = install::prompt::Prompt::new(yes, dry_run)
        .map_err(|error| MainError::Usage(error.to_string()))?;

    let options = install::Options {
        steps,
        agents,
        claude_route,
        marketplace,
        tmux_config,
        snippet,
        probe,
        home,
        exe: std::env::current_exe().ok(),
    };
    Ok(ExitCode::from(install::run(&options, &prompt).exit_code()))
}

/// Resolve the names `--agents=` gave.
///
/// A name that is not in the table is a usage error listing the valid names: a
/// typo'd `--agents=cursur` must never be read as "install nothing,
/// successfully".
fn parse_agents(names: &str) -> Result<Vec<&'static install::agents::Agent>, MainError> {
    names
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            install::agents::by_name(name).ok_or_else(|| {
                MainError::Usage(format!(
                    "unknown agent `{name}`: valid names are {}",
                    install::agents::names().join(", ")
                ))
            })
        })
        .collect()
}

fn opt_value(pargs: &mut Arguments, name: &'static str) -> Result<Option<String>, MainError> {
    pargs
        .opt_value_from_fn(name, |s: &str| Ok::<_, &'static str>(s.to_owned()))
        .map_err(|_| MainError::from(format!("{name} requires a value")))
}

fn pane_value(pargs: &mut Arguments) -> Result<Option<String>, MainError> {
    pargs
        .opt_value_from_fn("--pane", |s: &str| Ok::<_, &'static str>(s.to_owned()))
        .map_err(|_| MainError::from("--pane requires a value"))
}

fn free_strings(pargs: Arguments) -> Result<Vec<String>, MainError> {
    pargs
        .finish()
        .into_iter()
        .map(|os| {
            os.into_string()
                .map_err(|_| MainError::from("argument is not valid UTF-8"))
        })
        .collect::<Result<Vec<_>, _>>()
}

fn unexpected_arguments(command: &str, args: Vec<String>) -> MainError {
    let mut all = vec![command.to_string()];
    all.extend(args);
    MainError::from(format!("unexpected arguments: {}", all.join(" ")))
}

fn reject_extra_with_prefix(pargs: Arguments, command: &str) -> Result<(), MainError> {
    let free = free_strings(pargs)?;
    if !free.is_empty() {
        return Err(unexpected_arguments(command, free));
    }
    Ok(())
}

/// A hook must never break the agent that called it.
///
/// No tmux in the environment, a server that has exited, a tmux that is not on
/// `PATH`: all of them exit 0 and write nothing. The invocation was right; the
/// world simply had no tmux in it.
fn hook(result: io::Result<()>) -> ExitCode {
    let _ = result;
    ExitCode::SUCCESS
}

/// Run a hook command unless the user opted out of every write.
///
/// `TMUX_AGENT_STATUS_DISABLED=1` turns every write into a silent no-op.
/// `--json` prints `{}` on stdout when the hook exits successfully, so an
/// agent that parses hook stdout as JSON gets a valid empty object without a
/// wrapper in the command.
fn run_hook(f: impl FnOnce() -> io::Result<()>, json: bool) -> ExitCode {
    if !is_disabled() {
        let _ = f();
    }
    if json {
        let _ = writeln!(io::stdout(), "{{}}");
    }
    ExitCode::SUCCESS
}

fn is_disabled() -> bool {
    flag("TMUX_AGENT_STATUS_DISABLED")
}

/// Log a diagnostic to stderr when `TMUX_AGENT_STATUS_DEBUG` is set.
///
/// Never used on a hot path that agents call repeatedly; reserved for shape B
/// `notify` dropping an unrecognised event.
fn debug(message: &str) {
    if flag("TMUX_AGENT_STATUS_DEBUG") {
        eprintln!("tmux-agent-status: {message}");
    }
}

/// Whether a `TMUX_AGENT_STATUS_*` switch is on.
///
/// Any non-empty value counts, for every switch in the namespace. The
/// documented spelling is `=1`, but a user who writes `=true` means the same
/// thing, and a switch that silently ignores them cannot be told apart from one
/// that had nothing to report.
fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn usage_error(message: &MainError) -> ExitCode {
    eprintln!("tmux-agent-status: {message}");
    eprint!("{}", help());
    ExitCode::from(USAGE_ERROR)
}

fn help() -> String {
    let states: Vec<&str> = State::ALL.iter().map(|state| state.name()).collect();
    format!(
        "\
tmux-agent-status - agent lifecycle events as one glyph on the tmux window entry

usage:
  tmux-agent-status set <state> [--pane <id>] [--json]
                              write this pane's state and recompute the window
  tmux-agent-status start [--pane <id>] [--json]
                              begin a turn: replace whatever this pane holds
                              with working
  tmux-agent-status reset [--pane <id>] [--json]
                              clear this pane's state and recompute the window
  tmux-agent-status finish [--pane <id>] [--json]
                              silently resolve this pane's session to done
  tmux-agent-status clear-window [<pane>] [--pane <id>] [--json]
                              clear the non-sticky states of every pane of that
                              pane's window, defaulting to $TMUX_PANE
  tmux-agent-status notify --agent <name> [<payload>] [--json]
                              map a JSON payload from a shape-B agent
  tmux-agent-status notify --agent <name> --stdin [--json]
                              read the JSON payload from stdin
  tmux-agent-status install [flags]
                              write the agent hooks and tmux configuration the
                              README documents, asking before each change
  tmux-agent-status --version      version, and the executable that is actually running
  tmux-agent-status --help         this text

Use --json with a hook command to print '{{}}' on stdout on success, for agents
that parse hook stdout as JSON.

The pane is resolved in this order: --pane, $TMUX_AGENT_STATUS_PANE, $TMUX_PANE.

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every write into a no-op. Set
`TMUX_AGENT_STATUS_DEBUG=1` to log dropped events to stderr.

install flags:
  --agents[=<name>[,<name>...]]  agent hooks; with names, only those agents
  --tmux-hook                    the source-file line for the shipped snippet
  --tmux-format                  the glyph term in both window status formats
  --no-agents --no-tmux-hook --no-tmux-format
  -y, --yes                      take the recommended answer to every question
  --dry-run                      print the plan and change nothing
  --tmux-config <path>           the config file to edit
  --snippet <path>               where the sourced snippet lives, or should go
  --claude-route <auto|plugin|settings>
  --marketplace <source>         where to install the Claude Code plugin from
  --no-tmux-probe                do not check the edit against a throwaway tmux

With no step flags, install does all three. Positive and negative step flags
cannot be mixed.

agents: {}

states: {}
",
        install::agents::names().join(", "),
        states.join(", ")
    )
}

fn version() -> String {
    // The dev loop deliberately shadows the installed binary through PATH, and
    // a shadow you cannot see is a shadow that wastes an afternoon.
    tmux_agent_status::version::text(std::env::current_exe())
}
