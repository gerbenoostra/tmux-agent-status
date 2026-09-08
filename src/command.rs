//! What each command does: the policy that joins the pure rollup to the tmux calls.
//!
//! Every command returns `io::Result<()>` so tmux I/O failures can be handled by
//! the caller. The CLI's `hook()` wrapper turns those failures into a silent exit
//! 0, because a hook must never break the agent that called it.

use std::io;

use crate::bell;
use crate::rollup::rollup;
use crate::state::State;
use crate::tmux;

/// `tmux-agent-status set <state>`: write the pane's state and recompute the window.
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
    let mut panes = tmux::pane_statuses(&pane)?;
    if !state.is_sticky() && panes.iter().any(|status| status.window_watched) {
        // The window is already on screen, so writing the glyph would only be
        // read by the person who is looking at it anyway: clear instead, the
        // way focusing the window would. This pane is passed as superseded
        // because its own old state is over - a `done` that left `working` in
        // place would strand a 🤖 no later focus event ever clears.
        return clear_statuses(&pane, &mut panes, Some(&pane));
    }
    tmux::set_pane_status(&pane, state.name())?;
    recompute_after_set(&pane, &panes, state)
}

/// `tmux-agent-status clear-window [<pane>]`: drop the non-sticky states of every
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
    let mut panes = tmux::pane_statuses(&pane)?;
    clear_statuses(&pane, &mut panes, None)
}

/// Drop the states that seeing the window drops, then recompute.
///
/// `superseded` is the pane an event just arrived on, whose old state goes
/// whatever it was. Every other pane keeps anything unset, sticky or
/// unrecognised.
fn clear_statuses(
    target: &str,
    panes: &mut [tmux::PaneStatus],
    superseded: Option<&str>,
) -> io::Result<()> {
    for pane_status in panes.iter_mut() {
        if clears(pane_status, superseded) {
            tmux::clear_pane_status(&pane_status.pane)?;
            pane_status.status.clear();
        }
    }
    recompute(target, panes)
}

/// Whether seeing the window drops this pane's state.
fn clears(pane: &tmux::PaneStatus, superseded: Option<&str>) -> bool {
    if pane.status.is_empty() {
        return false;
    }
    if superseded == Some(pane.pane.as_str()) {
        return true;
    }
    pane.status
        .parse::<State>()
        .is_ok_and(|state| !state.is_sticky())
}

fn recompute_after_set(target: &str, panes: &[tmux::PaneStatus], state: State) -> io::Result<()> {
    let states: Vec<Option<State>> = panes
        .iter()
        .map(|pane| {
            if pane.pane == target {
                Some(state)
            } else {
                pane.status.parse::<State>().ok()
            }
        })
        .collect();
    write_rollup(target, &states)
}

/// Reduce the window's panes to one glyph, or to no option at all.
fn recompute(target: &str, panes: &[tmux::PaneStatus]) -> io::Result<()> {
    let states: Vec<Option<State>> = panes
        .iter()
        .map(|pane| pane.status.parse::<State>().ok())
        .collect();
    write_rollup(target, &states)
}

fn write_rollup(target: &str, states: &[Option<State>]) -> io::Result<()> {
    match rollup(states) {
        Some(state) => tmux::set_window_status(target, state.icon()),
        None => tmux::clear_window_status(target),
    }
}
