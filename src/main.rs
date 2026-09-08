//! Argument dispatch and exit codes. All behaviour lives in the library.

use std::env;
use std::io;
use std::process::ExitCode;

use agent_status::command;
use agent_status::state::State;

/// A wrong invocation: a bug in the caller's hook config, and so a loud one.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        ["set", state] => match state.parse::<State>() {
            Ok(state) => hook(command::set(state)),
            Err(err) => usage_error(&err.to_string()),
        },
        ["clear-window"] => hook(command::clear_window(None)),
        ["clear-window", pane] => hook(command::clear_window(Some(pane))),
        // Written for humans on stdout, so `--help | less` works. The hook
        // commands themselves never write to stdout at all.
        ["--help" | "-h"] => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        ["--version" | "-V"] => {
            println!("{}", version());
            ExitCode::SUCCESS
        }
        [] => usage_error("no command given"),
        _ => usage_error(&format!("unexpected arguments: {}", args.join(" "))),
    }
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

fn usage_error(message: &str) -> ExitCode {
    eprintln!("agent-status: {message}");
    eprint!("{}", help());
    ExitCode::from(USAGE_ERROR)
}

fn help() -> String {
    let states: Vec<&str> = State::ALL.iter().map(|state| state.name()).collect();
    format!(
        "\
agent-status - agent lifecycle events as one glyph on the tmux window entry

usage:
  agent-status set <state>    write this pane's state and recompute the window
  agent-status clear-window [<pane>]
                              clear the non-sticky states of every pane of that
                              pane's window, defaulting to $TMUX_PANE
  agent-status --version      version, and the executable that is actually running
  agent-status --help         this text

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
        .unwrap_or_else(|_| "<unknown>".to_owned());
    format!(
        "{} {}\nrunning from {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        exe
    )
}
