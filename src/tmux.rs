//! The only impure module: the tmux calls.
//!
//! It knows the option names and the commands that read and write them,
//! and nothing about ranks, glyphs or the transition table: every value it
//! writes is a format from `formats`, chosen by `command`.
//!
//! The two public options are `@agent_pane_status` (per pane) and
//! `@agent_status` (per window rollup). The other pane-local options are the
//! internal layers the public state is projected from; tmux option inheritance
//! makes a pane with no status read back as the window's value, so the rollup
//! can never share a name with a pane option.

use std::io;
use std::process::{Command, Stdio};

/// Per pane: the projected public state. Never referenced by the format
/// string, except as the legacy scalar an un-migrated pane still holds.
pub const PANE_OPTION: &str = "@agent_pane_status";

/// Per window, the rollup. The only thing the format string reads.
pub const WINDOW_OPTION: &str = "@agent_status";

/// Per pane: the parent turn's phase - `working`, `stopped` or `settling`.
pub const PANE_ROOT: &str = "@agent_pane_root";

/// Per pane: unacknowledged attention - `waiting` or `error`.
pub const PANE_ATTENTION: &str = "@agent_pane_attention";

/// Per pane: `pending` records a clean stop nobody has seen yet.
pub const PANE_COMPLETION: &str = "@agent_pane_completion";

/// Per pane: the tracked-work ledger, `,token,...,` or unset.
pub const PANE_WORK: &str = "@agent_pane_work";

/// Per pane: the hex-encoded host session lifecycle events must match.
pub const PANE_HOST_SESSION: &str = "@agent_pane_host_session";

/// Per pane: `1` once the layered state has been initialised.
pub const PANE_MODEL: &str = "@agent_pane_model";

/// A pane id as tmux prints it: `%` and a number.
///
/// Only ever parsed from tmux's own output, never taken from the caller, which
/// is what lets it be spliced into a nested command string without quoting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneId(String);

impl PaneId {
    pub(crate) fn parse(text: &str) -> Option<PaneId> {
        let number = text.strip_prefix('%')?;
        let valid = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
        valid.then(|| PaneId(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One tmux command, as its arguments.
pub type Cmd = Vec<String>;

/// The pane the caller is running in, or `None` when there is no tmux to talk to.
///
/// The tmux server sets both variables for every process it starts, so their
/// absence means the caller is not inside tmux. That is not an error.
pub fn current_pane() -> Option<String> {
    std::env::var_os("TMUX")?;
    std::env::var("TMUX_PANE")
        .ok()
        .filter(|pane| !pane.is_empty())
}

/// Resolve the pane to operate on, in priority order.
///
/// 1. An explicit value passed on the command line (`--pane`).
/// 2. The `TMUX_AGENT_STATUS_PANE` environment variable, for agents whose hook
///    format cannot pass an argument.
/// 3. The tmux-provided `$TMUX_PANE` of the caller's pane.
///
/// `None` means the caller is not inside tmux and no override was given, so the
/// command should silently do nothing.
///
/// An empty override is no override. Every per-agent page documents
/// `--pane #{pane_id}` or `--pane "$TMUX_PANE"`, and both expand to nothing
/// outside tmux; tmux reads an empty `-t` as *the current pane*, so an unfiltered
/// empty value paints the glyph on whatever pane the server happens to be on.
pub fn resolve_pane(explicit: Option<&str>) -> Option<String> {
    if let Some(pane) = explicit.filter(|pane| !pane.is_empty()) {
        return Some(pane.to_owned());
    }
    if let Ok(pane) = std::env::var("TMUX_AGENT_STATUS_PANE") {
        if !pane.is_empty() {
            return Some(pane);
        }
    }
    current_pane()
}

/// The pane `target` resolves to.
///
/// This is the one read a command performs, and only the pane's id comes back:
/// no aggregate state is ever read into a write decision. A pane that has
/// closed since its hook fired fails here, which the caller's silent exit
/// turns into a no-op.
pub fn pane(target: &str) -> io::Result<PaneId> {
    let out = run(&[cmd(&["display-message", "-p", "-t", target, "#{pane_id}"])])?;
    out.lines().next().and_then(PaneId::parse).ok_or_else(|| {
        io::Error::other(format!(
            "tmux printed an unexpected answer for {target}: {out}"
        ))
    })
}

/// Set a pane option to what `format` expands to on that pane.
pub fn set_pane_option(pane: &PaneId, option: &str, format: &str) -> Cmd {
    cmd(&[
        "set-option",
        "-p",
        "-F",
        "-t",
        pane.as_str(),
        option,
        format,
    ])
}

/// Unset `option` on `pane` when `condition` expands true on that pane.
///
/// A format can only expand to a value, so a write that clears leaves an empty
/// string; the conditional unset turns that back into an unset option and
/// decides on the value current at the instant it runs, so a write that lands
/// in between is never removed.
pub fn unset_pane_option_if(pane: &PaneId, option: &str, condition: &str) -> Cmd {
    // The nested command does not inherit the `-t` of `if-shell` - verified on
    // 3.6a - so it names the pane again.
    let unset = format!("set-option -p -u -t {} {option}", pane.as_str());
    cmd(&["if-shell", "-F", "-t", pane.as_str(), condition, &unset])
}

/// Unset `option` on `pane` if it holds nothing.
pub fn unset_pane_option_if_empty(pane: &PaneId, option: &str) -> Cmd {
    unset_pane_option_if(pane, option, &format!("#{{?{option},,1}}"))
}

/// Set the glyph of the pane's window to what `format` expands to there.
pub fn set_window_status(pane: &PaneId, format: &str) -> Cmd {
    cmd(&[
        "set-option",
        "-w",
        "-F",
        "-t",
        pane.as_str(),
        WINDOW_OPTION,
        format,
    ])
}

/// Unset the glyph of the pane's window if it holds nothing, so the format term
/// renders nothing and a window without an agent carries no option at all.
pub fn unset_window_status_if_empty(pane: &PaneId) -> Cmd {
    let unset = format!("set-option -w -u -t {} {WINDOW_OPTION}", pane.as_str());
    cmd(&[
        "if-shell",
        "-F",
        "-t",
        pane.as_str(),
        &format!("#{{?{WINDOW_OPTION},,1}}"),
        &unset,
    ])
}

/// Print `1` when the pane's tracked-work ledger is empty, nothing otherwise.
///
/// Appended as the last command of a queue so the verdict reflects the writes
/// the same serialized run just performed; the caller reads its stdout.
pub fn work_verdict(pane: &PaneId) -> Cmd {
    let verdict = format!("#{{?{PANE_WORK},,1}}");
    cmd(&["display-message", "-p", "-t", pane.as_str(), &verdict])
}

/// Run `commands` as one tmux invocation and return what they printed.
///
/// One invocation is one command queue on the server, and an error stops the
/// rest of it.
///
/// Never called with an empty list, which `tmux` would read as `new-session`:
/// every command ends by recomputing the window glyph, so every list has at
/// least those two commands in it. A guard against it would be a branch no
/// integration test can reach, and unreachable branches are what the coverage
/// gate exists to keep out.
pub fn run(commands: &[Cmd]) -> io::Result<String> {
    let mut args: Vec<&str> = Vec::new();
    for command in commands {
        if !args.is_empty() {
            args.push(";");
        }
        args.extend(command.iter().map(String::as_str));
    }
    tmux(&args)
}

fn cmd(args: &[&str]) -> Cmd {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn tmux(args: &[&str]) -> io::Result<String> {
    let out = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "tmux {} exited with {}",
            args.join(" "),
            out.status
        )));
    }
    String::from_utf8(out.stdout).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> PaneId {
        PaneId::parse(text).expect("a valid pane id")
    }

    #[test]
    fn an_empty_explicit_pane_is_no_pane() {
        // Whatever the environment holds, an empty `--pane` must never reach
        // tmux: `-t ""` resolves to the current pane rather than failing.
        assert_ne!(resolve_pane(Some("")), Some(String::new()));
        assert_eq!(resolve_pane(Some("%7")), Some("%7".to_owned()));
    }

    #[test]
    fn a_pane_id_is_a_percent_sign_and_a_number() {
        assert_eq!(id("%12").as_str(), "%12");
        for text in ["", "%", "12", "%1a", "% 1", "%1;", "t:0.1"] {
            assert_eq!(PaneId::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn no_argument_ends_in_a_command_separator() {
        // tmux splits a command list on an argument that ends in `;`.
        let pane = id("%3");
        for command in [
            set_pane_option(&pane, PANE_OPTION, "#{x}"),
            unset_pane_option_if_empty(&pane, PANE_OPTION),
            unset_pane_option_if(&pane, PANE_WORK, "#{==:#{@agent_pane_work},}"),
            set_window_status(&pane, "#{x}"),
            unset_window_status_if_empty(&pane),
            work_verdict(&pane),
        ] {
            assert!(command.iter().all(|arg| !arg.ends_with(';')), "{command:?}");
        }
    }

    #[test]
    fn option_names_cannot_break_a_format() {
        // The option names are spliced into formats and nested command
        // strings; a `;` or `,` in one would inject a command or break an
        // argument list.
        for option in [
            PANE_OPTION,
            WINDOW_OPTION,
            PANE_ROOT,
            PANE_ATTENTION,
            PANE_COMPLETION,
            PANE_WORK,
            PANE_HOST_SESSION,
            PANE_MODEL,
        ] {
            assert!(!option.contains([';', ',', '#', '{', '}', ' ']), "{option}");
        }
    }
}
