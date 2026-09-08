//! The only impure module: the tmux calls.
//!
//! It knows the two option names and how to invoke tmux, and nothing about
//! ranks, glyphs, stickiness or when to write what.

use std::io;
use std::process::{Command, Stdio};

/// Per pane, written from `$TMUX_PANE`. Never referenced by the format string.
const PANE_OPTION: &str = "@agent_pane_status";

/// Per window, the rollup. The only thing the format string reads.
const WINDOW_OPTION: &str = "@agent_status";

/// A pane and whatever `@agent_pane_status` holds for it, empty string included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneStatus {
    pub pane: String,
    pub status: String,
    /// Whether this pane's window is on screen: the current window of a session
    /// a client is attached to. tmux calls a detached session's current window
    /// active, but nobody is looking at it, so the client count is part of the
    /// answer - otherwise a turn that ends while you are detached is cleared
    /// before you ever get to see it.
    pub window_watched: bool,
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

/// The status of every pane of `target`'s window.
///
/// A pane target resolves to the window that holds it, which is how one
/// `$TMUX_PANE` addresses all of its siblings.
pub fn pane_statuses(target: &str) -> io::Result<Vec<PaneStatus>> {
    let out = tmux(&[
        "list-panes",
        "-t",
        target,
        "-F",
        &format!("#{{pane_id}}\t#{{{PANE_OPTION}}}\t#{{window_active}}\t#{{session_attached}}"),
    ])?;
    out.lines().map(parse_pane_status).collect()
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

fn parse_pane_status(line: &str) -> io::Result<PaneStatus> {
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
fn parse_fields(line: &str) -> Option<PaneStatus> {
    let (pane, rest) = line.split_once('\t')?;
    let (rest, attached) = rest.rsplit_once('\t')?;
    let (status, active) = rest.rsplit_once('\t')?;
    let active = match active {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    // `session_attached` counts clients; it is not a flag.
    let attached: u32 = attached.parse().ok()?;
    Some(PaneStatus {
        pane: pane.to_owned(),
        status: status.to_owned(),
        window_watched: active && attached > 0,
    })
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
    fn parse_pane_status_splits_on_tab() {
        let status = parse_pane_status("%0\tdone\t1\t1").unwrap();
        assert_eq!(status.pane, "%0");
        assert_eq!(status.status, "done");
        assert!(status.window_watched);
    }

    #[test]
    fn parse_pane_status_allows_empty_status() {
        let status = parse_pane_status("%0\t\t0\t1").unwrap();
        assert_eq!(status.pane, "%0");
        assert_eq!(status.status, "");
        assert!(!status.window_watched);
    }

    #[test]
    fn parse_pane_status_keeps_extra_tabs_in_status() {
        let status = parse_pane_status("%0\twaiting\textra\t0\t1").unwrap();
        assert_eq!(status.pane, "%0");
        assert_eq!(status.status, "waiting\textra");
    }

    #[test]
    fn the_current_window_of_a_detached_session_is_not_watched() {
        let status = parse_pane_status("%0\tdone\t1\t0").unwrap();
        assert!(!status.window_watched);
    }

    #[test]
    fn more_than_one_client_still_counts_as_watched() {
        // `session_attached` is a client count, so a second client must not
        // parse as "not a flag" and take the window off screen.
        let status = parse_pane_status("%0\tdone\t1\t2").unwrap();
        assert!(status.window_watched);
    }

    #[test]
    fn parse_pane_status_errors_without_tab() {
        let err = parse_pane_status("badline").unwrap_err();
        assert!(err.to_string().contains("badline"));
    }

    #[test]
    fn parse_pane_status_errors_on_unreadable_flags() {
        for line in [
            "%0\tdone\tyes\t1",
            "%0\tdone\t1\tmany",
            "%0\tdone\t1",
            "%0\tdone",
        ] {
            let err = parse_pane_status(line).unwrap_err();
            assert!(err.to_string().contains(line), "line {line:?}");
        }
    }
}
