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
        &format!("#{{pane_id}}\t#{{{PANE_OPTION}}}"),
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
    let (pane, status) = line.split_once('\t').ok_or_else(|| {
        io::Error::other(format!(
            "tmux list-panes printed an unexpected line: {line}"
        ))
    })?;
    Ok(PaneStatus {
        pane: pane.to_owned(),
        status: status.to_owned(),
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
