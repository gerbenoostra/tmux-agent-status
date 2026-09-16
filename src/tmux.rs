//! The only impure module: the tmux calls.
//!
//! It knows the two option names and the commands that read and write them,
//! and nothing about ranks, glyphs or stickiness: every value it writes is a
//! format from `formats`, chosen by `command`.

use std::io;
use std::process::{Command, Stdio};

/// Per pane, written from `$TMUX_PANE`. Never referenced by the format string.
pub const PANE_OPTION: &str = "@agent_pane_status";

/// Per window, the rollup. The only thing the format string reads.
pub const WINDOW_OPTION: &str = "@agent_status";

/// A pane id as tmux prints it: `%` and a number.
///
/// Only ever parsed from tmux's own output, never taken from the caller, which
/// is what lets it be spliced into a nested command string without quoting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneId(String);

impl PaneId {
    fn parse(text: &str) -> Option<PaneId> {
        let number = text.strip_prefix('%')?;
        let valid = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
        valid.then(|| PaneId(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A pane of the window, and whether its status option holds anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: PaneId,
    pub has_status: bool,
}

/// The window a hook addresses, as one read saw it.
///
/// Only which panes to write is taken from here. Whether a write lands, and
/// what it writes, is decided again by the server when it runs: this read can
/// be stale by then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// The pane the target resolved to.
    pub pane: PaneId,
    /// Every pane of its window, that one included.
    pub panes: Vec<Pane>,
}

impl Window {
    /// Every pane holding a status.
    pub fn panes_with_status(&self) -> impl Iterator<Item = &PaneId> {
        self.panes
            .iter()
            .filter(|pane| pane.has_status)
            .map(|pane| &pane.id)
    }

    /// The panes other than the addressed one holding a status.
    pub fn siblings_with_status(&self) -> impl Iterator<Item = &PaneId> {
        self.panes_with_status().filter(|id| **id != self.pane)
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

/// The pane `target` resolves to and the panes of its window, in one call.
///
/// A pane target resolves to the window that holds it, which is how one
/// `$TMUX_PANE` addresses all of its siblings.
pub fn window(target: &str) -> io::Result<Window> {
    let listing = format!("#{{pane_id}}\t#{{{PANE_OPTION}}}");
    let out = run(&[
        cmd(&["display-message", "-p", "-t", target, "#{pane_id}"]),
        cmd(&["list-panes", "-t", target, "-F", &listing]),
    ])?;
    parse_window(&out).ok_or_else(|| {
        io::Error::other(format!(
            "tmux printed an unexpected description of {target}: {out}"
        ))
    })
}

/// The addressed pane's id on the first line, then `pane<TAB>status` per pane.
///
/// The status is whatever the user's option holds and may contain tabs itself,
/// so the pane is taken from the left. Every line is ours to have asked for, so
/// one that does not parse is a tmux that did not answer the question.
fn parse_window(out: &str) -> Option<Window> {
    let mut lines = out.lines();
    let pane = PaneId::parse(lines.next()?)?;
    let panes = lines
        .map(|line| {
            let (id, status) = line.split_once('\t')?;
            Some(Pane {
                id: PaneId::parse(id)?,
                has_status: !status.is_empty(),
            })
        })
        .collect::<Option<_>>()?;
    Some(Window { pane, panes })
}

/// Set the pane's status to what `format` expands to on that pane.
pub fn set_pane_status(pane: &PaneId, format: &str) -> Cmd {
    cmd(&[
        "set-option",
        "-p",
        "-F",
        "-t",
        pane.as_str(),
        PANE_OPTION,
        format,
    ])
}

/// Unset the pane's status if it holds nothing.
///
/// A format can only expand to a value, so a write that clears leaves an empty
/// string; this turns it back into an unset option, which reads back empty
/// rather than inheriting. It decides on what the pane holds when it runs, so a
/// write that lands in between is never removed.
pub fn unset_pane_status_if_empty(pane: &PaneId) -> Cmd {
    // The nested command does not inherit the `-t` of `if-shell` - verified on
    // 3.6a - so it names the pane again.
    let unset = format!("set-option -p -u -t {} {PANE_OPTION}", pane.as_str());
    cmd(&[
        "if-shell",
        "-F",
        "-t",
        pane.as_str(),
        &format!("#{{?{PANE_OPTION},,1}}"),
        &unset,
    ])
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
    fn parse_window_reads_the_pane_then_every_pane_of_its_window() {
        let window = parse_window("%1\n%0\tdone\n%1\t\n").unwrap();
        assert_eq!(window.pane, id("%1"));
        assert_eq!(
            window.panes,
            [
                Pane {
                    id: id("%0"),
                    has_status: true
                },
                Pane {
                    id: id("%1"),
                    has_status: false
                },
            ]
        );
    }

    #[test]
    fn a_value_with_tabs_is_still_a_status() {
        let window = parse_window("%0\n%0\twaiting\textra\n").unwrap();
        assert!(window.panes[0].has_status);
    }

    #[test]
    fn siblings_leave_out_the_addressed_pane_and_panes_without_a_status() {
        let window = parse_window("%1\n%0\tdone\n%1\tdone\n%2\t\n").unwrap();
        assert_eq!(
            window.panes_with_status().collect::<Vec<_>>(),
            [&id("%0"), &id("%1")]
        );
        assert_eq!(
            window.siblings_with_status().collect::<Vec<_>>(),
            [&id("%0")]
        );
    }

    #[test]
    fn parse_window_rejects_output_it_did_not_ask_for() {
        for out in ["", "badline", "%0\nbadline", "%0\n%x\tdone", "t:0\n%0\t"] {
            assert_eq!(parse_window(out), None, "{out:?}");
        }
    }

    #[test]
    fn no_argument_ends_in_a_command_separator() {
        // tmux splits a command list on an argument that ends in `;`.
        let pane = id("%3");
        for command in [
            set_pane_status(&pane, "#{x}"),
            unset_pane_status_if_empty(&pane),
            set_window_status(&pane, "#{x}"),
            unset_window_status_if_empty(&pane),
        ] {
            assert!(command.iter().all(|arg| !arg.ends_with(';')), "{command:?}");
        }
    }
}
