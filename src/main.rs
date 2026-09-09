//! Argument dispatch and exit codes. All behaviour lives in the library.

use std::env;
use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

use tmux_agent_status::command;
use tmux_agent_status::notify::{self, Action};
use tmux_agent_status::state::State;

/// A wrong invocation: a bug in the caller's hook config, and so a loud one.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let pane = match extract_pane(&mut args) {
        Ok(pane) => pane,
        Err(err) => return usage_error(&err),
    };
    let pane_ref = pane.as_deref();

    match args.as_slice() {
        [cmd, state] if cmd == "set" => match state.parse::<State>() {
            Ok(state) => run_hook(|| command::set(state, pane_ref)),
            Err(err) => usage_error(&err.to_string()),
        },
        [cmd, ..] if cmd == "set" => usage_error("set requires a state"),
        [cmd] if cmd == "reset" => run_hook(|| command::reset(pane_ref)),
        [cmd] if cmd == "finish" => run_hook(|| command::finish(pane_ref)),
        [cmd] if cmd == "clear-window" => run_hook(|| command::clear_window(pane_ref)),
        [cmd, positional] if cmd == "clear-window" => {
            run_hook(|| command::clear_window(pane_ref.or(Some(positional.as_str()))))
        }
        [cmd, ..] if cmd == "notify" => match run_notify(args, pane_ref) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => usage_error(&err),
        },
        // Written for humans on stdout, so `--help | less` works. The hook
        // commands themselves never write to stdout at all.
        [cmd] if cmd == "--help" || cmd == "-h" => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        [cmd] if cmd == "--version" || cmd == "-V" => {
            println!("{}", version());
            ExitCode::SUCCESS
        }
        [] => usage_error("no command given"),
        _ => usage_error(&format!("unexpected arguments: {}", args.join(" "))),
    }
}

/// Pull `--pane <id>` out of the argument list if present.
///
/// Returns an error when `--pane` is the last argument with no value.
fn extract_pane(args: &mut Vec<String>) -> Result<Option<String>, String> {
    let Some(pos) = args.iter().position(|arg| arg == "--pane") else {
        return Ok(None);
    };
    args.remove(pos);
    if pos >= args.len() {
        return Err("--pane requires a value".into());
    }
    Ok(Some(args.remove(pos)))
}

fn extract_agent(args: &mut Vec<String>) -> Result<Option<String>, String> {
    let Some(pos) = args.iter().position(|arg| arg == "--agent") else {
        return Ok(None);
    };
    args.remove(pos);
    if pos >= args.len() {
        return Err("--agent requires a value".into());
    }
    Ok(Some(args.remove(pos)))
}

fn extract_stdin(args: &mut Vec<String>) -> bool {
    let Some(pos) = args.iter().position(|arg| arg == "--stdin") else {
        return false;
    };
    args.remove(pos);
    true
}

/// Run the shape-B `notify` subcommand: parse the JSON payload and call the
/// matching hook command.
///
/// Errors are usage errors because they mean the hook line itself is wrong. An
/// unrecognised payload is not an error: it is silently dropped so upstream
/// changes do not break the agent.
fn run_notify(mut args: Vec<String>, pane: Option<&str>) -> Result<(), String> {
    let from_stdin = extract_stdin(&mut args);
    let agent = extract_agent(&mut args)?.ok_or("notify requires --agent")?;

    if is_disabled() {
        return Ok(());
    }

    let payload = if from_stdin {
        if std::io::stdin().is_terminal() {
            debug("notify: --stdin with a terminal is a no-op");
            return Ok(());
        }
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("cannot read stdin: {e}"))?;
        buf
    } else {
        match args.as_slice() {
            [_, payload] => payload.clone(),
            [cmd] if cmd == "notify" => {
                return Err("notify requires a payload or --stdin".into());
            }
            _ => return Err(format!("unexpected arguments: {}", args.join(" "))),
        }
    };

    match notify::dispatch(&agent, &payload) {
        Some(Action::Set(state)) => hook(command::set(state, pane)),
        Some(Action::Reset) => hook(command::reset(pane)),
        Some(Action::Finish) => hook(command::finish(pane)),
        None => {
            debug(&format!("notify: dropped payload for {agent}"));
            ExitCode::SUCCESS
        }
    };
    Ok(())
}

/// A hook must never break the agent that called it.
///
/// No tmux in the environment, a server that has exited, a tmux that is not on
/// `PATH`: all of them exit 0 and write nothing. The invocation was right; the
/// world simply had no tmux in it.
///
/// This is the intentional boundary between `command.rs` (which propagates tmux
/// I/O failures as `io::Result`) and the CLI (which decides that hook commands
/// are allowed to fail silently). Any future command that is not a hook should
/// route its errors differently rather than passing through `hook()`.
fn hook(result: io::Result<()>) -> ExitCode {
    let _ = result;
    ExitCode::SUCCESS
}

/// Run a hook command unless the user opted out of every write.
///
/// `TMUX_AGENT_STATUS_DISABLED=1` turns every write into a silent no-op.
fn run_hook(f: impl FnOnce() -> io::Result<()>) -> ExitCode {
    if is_disabled() {
        return ExitCode::SUCCESS;
    }
    hook(f())
}

fn is_disabled() -> bool {
    env::var_os("TMUX_AGENT_STATUS_DISABLED").is_some_and(|v| !v.is_empty())
}

/// Log a diagnostic to stderr when `TMUX_AGENT_STATUS_DEBUG=1` is set.
///
/// Never used on a hot path that agents call repeatedly; reserved for shape B
/// `notify` dropping an unrecognised event.
#[allow(dead_code)]
fn debug(message: &str) {
    if env::var_os("TMUX_AGENT_STATUS_DEBUG").is_some_and(|v| v == "1") {
        eprintln!("tmux-agent-status: {message}");
    }
}

fn usage_error(message: &str) -> ExitCode {
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
  tmux-agent-status set <state> [--pane <id>]
                              write this pane's state and recompute the window
  tmux-agent-status reset [--pane <id>]
                              clear this pane's state and recompute the window
  tmux-agent-status finish [--pane <id>]
                              silently resolve this pane's session to done
  tmux-agent-status clear-window [<pane>] [--pane <id>]
                              clear the non-sticky states of every pane of that
                              pane's window, defaulting to $TMUX_PANE
  tmux-agent-status notify --agent <name> [<payload>]
                              map a JSON payload from a shape-B agent
  tmux-agent-status notify --agent <name> --stdin
                              read the JSON payload from stdin
  tmux-agent-status --version      version, and the executable that is actually running
  tmux-agent-status --help         this text

The pane is resolved in this order: --pane, $TMUX_AGENT_STATUS_PANE, $TMUX_PANE.

Set `TMUX_AGENT_STATUS_DISABLED=1` to turn every write into a no-op. Set
`TMUX_AGENT_STATUS_DEBUG=1` to log dropped events to stderr.

states: {}
",
        states.join(", ")
    )
}

fn version() -> String {
    // The dev loop deliberately shadows the installed binary through PATH, and
    // a shadow you cannot see is a shadow that wastes an afternoon.
    let exe = env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or("<unknown>".to_owned());
    format!(
        "{} {}\nrunning from {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        exe
    )
}
