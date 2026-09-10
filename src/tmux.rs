//! The only impure module: the tmux calls.
//!
//! It knows the two option names and how to invoke tmux, and nothing about
//! ranks, glyphs, stickiness or when to write what.

use std::io;
use std::process::{Command, Stdio};

use crate::state::State;

/// Per pane, written from `$TMUX_PANE`. Never referenced by the format string.
const PANE_OPTION: &str = "@agent_pane_status";

/// Per window, the rollup. The only thing the format string reads.
const WINDOW_OPTION: &str = "@agent_status";

/// A pane and whatever `@agent_pane_status` holds for it.
///
/// The raw string is parsed to a `State` once, at the boundary. An empty or
/// unrecognised value is `None`, so an externally set invalid string cannot
/// corrupt the rollup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneStatus {
    pub pane: String,
    pub status: Option<State>,
}

/// A window's panes, and whether anyone is looking at it.
///
/// Being watched is a fact about the window, not about any one pane, which is
/// why it is read once here rather than carried on every `PaneStatus`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// Whether the window is on screen: the current window of a session a
    /// client is attached to. tmux calls a detached session's current window
    /// active, but nobody is looking at it, so the client count is part of the
    /// answer - otherwise a turn that ends while you are detached is cleared
    /// before you ever get to see it.
    pub watched: bool,
    pub panes: Vec<PaneStatus>,
}

/// One `list-panes` line: a pane's status, plus the window answer that every
/// pane of the window repeats.
#[derive(Debug)]
struct PaneLine {
    status: PaneStatus,
    watched: bool,
}

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

/// The status of every pane of `target`'s window, and whether it is watched.
///
/// A pane target resolves to the window that holds it, which is how one
/// `$TMUX_PANE` addresses all of its siblings.
pub fn window(target: &str) -> io::Result<Window> {
    let out = tmux(&[
        "list-panes",
        "-t",
        target,
        "-F",
        &format!("#{{pane_id}}\t#{{{PANE_OPTION}}}\t#{{window_active}}\t#{{session_attached}}"),
    ])?;
    let lines: Vec<PaneLine> = out
        .lines()
        .map(parse_pane_line)
        .collect::<io::Result<_>>()?;
    // All panes of a window are on screen together, so they all answer the same
    // and the first is the answer.
    let watched = lines.first().is_some_and(|line| line.watched);
    let panes = lines.into_iter().map(|line| line.status).collect();
    Ok(Window { watched, panes })
}

/// Write a pane's status.
pub fn set_pane_status(pane: &str, value: &str) -> io::Result<()> {
    tmux(&["set-option", "-p", "-t", pane, PANE_OPTION, value]).map(drop)
}

/// Unset a pane's status, so it reads back empty rather than inheriting.
pub fn clear_pane_status(pane: &str) -> io::Result<()> {
    tmux(&["set-option", "-p", "-u", "-t", pane, PANE_OPTION]).map(drop)
}

/// Write the rollup on `target`'s window.
pub fn set_window_status(target: &str, value: &str) -> io::Result<()> {
    tmux(&["set-option", "-w", "-t", target, WINDOW_OPTION, value]).map(drop)
}

/// Unset the rollup, so the format term renders nothing at all.
pub fn clear_window_status(target: &str) -> io::Result<()> {
    tmux(&["set-option", "-w", "-u", "-t", target, WINDOW_OPTION]).map(drop)
}

fn parse_pane_line(line: &str) -> io::Result<PaneLine> {
    parse_fields(line).ok_or_else(|| {
        io::Error::other(format!(
            "tmux list-panes printed an unexpected line: {line}"
        ))
    })
}

/// `pane<TAB>status<TAB>window_active<TAB>session_attached`.
///
/// The status is whatever the user's option holds and may contain tabs itself,
/// so the pane is taken from the left and the two flags from the right.
fn parse_fields(line: &str) -> Option<PaneLine> {
    let (pane, rest) = line.split_once('\t')?;
    let (rest, attached) = rest.rsplit_once('\t')?;
    let (status, active) = rest.rsplit_once('\t')?;
    Some(PaneLine {
        status: PaneStatus {
            pane: pane.to_owned(),
            status: status.parse::<State>().ok(),
        },
        watched: is_watched(active, attached),
    })
}

/// Whether tmux says the window is on screen, read leniently.
///
/// The tabs are ours, so a line missing one is a tmux that ignored the format
/// and an error worth raising. The values are tmux's, and nothing reports the
/// error to anyone - a hook exits 0 whatever happens - so a value this cannot
/// read must not take the tool out of service. It means "not watched", which
/// costs the immediate clear and nothing else: a glyph you clear by looking.
fn is_watched(active: &str, attached: &str) -> bool {
    // `session_attached` counts clients; it is not a flag.
    active == "1" && attached.parse::<u32>().is_ok_and(|clients| clients > 0)
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

    #[test]
    fn an_empty_explicit_pane_is_no_pane() {
        // Whatever the environment holds, an empty `--pane` must never reach
        // tmux: `-t ""` resolves to the current pane rather than failing.
        assert_ne!(resolve_pane(Some("")), Some(String::new()));
        assert_eq!(resolve_pane(Some("%7")), Some("%7".to_owned()));
    }

    #[test]
    fn parse_pane_line_splits_on_tab() {
        let line = parse_pane_line("%0\tdone\t1\t1").unwrap();
        assert_eq!(line.status.pane, "%0");
        assert_eq!(line.status.status, Some(State::Done));
        assert!(line.watched);
    }

    #[test]
    fn parse_pane_line_allows_empty_status() {
        let line = parse_pane_line("%0\t\t0\t1").unwrap();
        assert_eq!(line.status.pane, "%0");
        assert_eq!(line.status.status, None);
        assert!(!line.watched);
    }

    #[test]
    fn parse_pane_line_keeps_extra_tabs_in_status() {
        // Extra tabs in the status field make the value unrecognisable, but the
        // parser must still split the line and not panic.
        let line = parse_pane_line("%0\twaiting\textra\t0\t1").unwrap();
        assert_eq!(line.status.pane, "%0");
        assert_eq!(line.status.status, None);
        assert!(!line.watched);
    }

    #[test]
    fn the_current_window_of_a_detached_session_is_not_watched() {
        assert!(!parse_pane_line("%0\tdone\t1\t0").unwrap().watched);
    }

    #[test]
    fn more_than_one_client_still_counts_as_watched() {
        // `session_attached` is a client count, so a second client must not
        // parse as "not a flag" and take the window off screen.
        assert!(parse_pane_line("%0\tdone\t1\t2").unwrap().watched);
    }

    #[test]
    fn a_flag_that_cannot_be_read_means_not_watched() {
        // A tmux whose flags this cannot read must still set states: nobody
        // ever sees the error, so an unreadable flag costs the immediate clear
        // and not the tool.
        for line in ["%0\tdone\tyes\t1", "%0\tdone\t1\tmany", "%0\tdone\t\t"] {
            let parsed = parse_pane_line(line).unwrap();
            assert_eq!(parsed.status.status, Some(State::Done), "line {line:?}");
            assert!(!parsed.watched, "line {line:?}");
        }
    }

    #[test]
    fn parse_pane_line_errors_on_a_line_that_is_not_ours() {
        // The tabs are ours; a line without them is not a line we asked for.
        for line in ["badline", "%0\tdone", "%0\tdone\t1"] {
            let err = parse_pane_line(line).unwrap_err();
            assert!(err.to_string().contains(line), "line {line:?}");
        }
    }
}
