//! What each command does: the policy that joins the pure rollup to the tmux calls.

use std::io;

use crate::bell;
use crate::rollup::rollup;
use crate::state::State;
use crate::tmux;

/// `agent-status set <state>`: write the pane's state and recompute the window.
///
/// The bell is rung whether or not there is a tmux to write to, because it is a
/// separate channel: it reaches the human through the terminal, which a tmux
/// option can never do.
pub fn set(state: State) -> io::Result<()> {
    if state.rings_bell() {
        bell::ring();
    }
    let Some(pane) = tmux::current_pane() else {
        return Ok(());
    };
    tmux::set_pane_status(&pane, state.name())?;
    recompute(&pane)
}

/// `agent-status clear-window [<pane>]`: drop the non-sticky states of every
/// pane of that pane's window, then recompute.
///
/// Every pane, not just the focused one: all panes of a window are on screen
/// together, so seeing the window is seeing them.
///
/// The pane is an argument because tmux's `run-shell` does not put `TMUX_PANE`
/// in a hook's environment - it does expand formats in the command, so the
/// shipped hook passes `#{pane_id}`. Without one, `$TMUX_PANE` is used, which
/// is what a hand invocation from a pane has.
pub fn clear_window(pane: Option<&str>) -> io::Result<()> {
    let pane = match pane {
        Some(pane) => pane.to_owned(),
        None => match tmux::current_pane() {
            Some(pane) => pane,
            None => return Ok(()),
        },
    };
    for pane_status in tmux::pane_statuses(&pane)? {
        // Anything unset, sticky or unrecognised is left exactly as it is.
        // (A let-chain would read better and needs a newer compiler than the MSRV.)
        if let Ok(state) = pane_status.status.parse::<State>() {
            if !state.is_sticky() {
                tmux::clear_pane_status(&pane_status.pane)?;
            }
        }
    }
    recompute(&pane)
}

/// Reduce the window's panes to one glyph, or to no option at all.
fn recompute(target: &str) -> io::Result<()> {
    let panes = tmux::pane_statuses(target)?;
    let states: Vec<Option<State>> = panes
        .iter()
        .map(|pane| pane.status.parse::<State>().ok())
        .collect();
    match rollup(&states) {
        Some(state) => tmux::set_window_status(target, state.icon()),
        None => tmux::clear_window_status(target),
    }
}
