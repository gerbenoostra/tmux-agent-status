//! Argument dispatch and exit codes. All behaviour lives in the library.

use std::env;
use std::io;
use std::process::ExitCode;

use tmux_agent_status::command;
use tmux_agent_status::state::State;

/// A wrong invocation: a bug in the caller's hook config, and so a loud one.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.as_slice() {
        [cmd, state] if cmd == "set" => match state.parse::<State>() {
            Ok(state) => hook(command::set(state)),
            Err(err) => usage_error(&err.to_string()),
        },
        [cmd, ..] if cmd == "set" => usage_error("set requires a state"),
        [cmd] if cmd == "clear-window" => hook(command::clear_window(None)),
        [cmd, pane] if cmd == "clear-window" => hook(command::clear_window(Some(pane.as_str()))),
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
  tmux-agent-status set <state>    write this pane's state and recompute the window
  tmux-agent-status clear-window [<pane>]
                              clear the non-sticky states of every pane of that
                              pane's window, defaulting to $TMUX_PANE
  tmux-agent-status --version      version, and the executable that is actually running
  tmux-agent-status --help         this text

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
