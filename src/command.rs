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
pub fn set(state: State, pane: Option<&str>) -> io::Result<()> {
    if state.rings_bell() {
        bell::ring();
    }
    let Some(pane) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&pane)?;
    write_status(&pane, window, state)
}

/// `tmux-agent-status finish`: silently resolve this pane's session to done.
pub fn finish(pane: Option<&str>) -> io::Result<()> {
    let Some(pane) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&pane)?;
    let has_error = window
        .panes
        .iter()
        .any(|listed| listed.pane == pane && listed.status.parse::<State>() == Ok(State::Error));
    if has_error {
        return Ok(());
    }
    write_status(&pane, window, State::Done)
}

/// `tmux-agent-status reset`: unconditionally drop this pane's session status.
pub fn reset(pane: Option<&str>) -> io::Result<()> {
    let Some(pane) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let mut window = tmux::window(&pane)?;
    for reporter in window.panes.iter_mut().filter(|listed| listed.pane == pane) {
        if !reporter.status.is_empty() {
            tmux::clear_pane_status(&pane)?;
            reporter.status.clear();
        }
    }
    recompute(&pane, &window.panes)
}

/// Write a pane state under the ordinary watched-window policy, then recompute.
fn write_status(pane: &str, mut window: tmux::Window, state: State) -> io::Result<()> {
    if !state.is_sticky() && window.watched {
        // The window is already on screen, so writing the glyph would only be
        // read by the person who is looking at it anyway: clear instead, the
        // way focusing the window would. This pane is passed as superseded
        // because its own old state is over - a `done` that left `working` in
        // place would strand a 🤖 no later focus event ever clears.
        return clear_statuses(pane, &mut window.panes, Some(pane));
    }
    tmux::set_pane_status(pane, state.name())?;
    // The panes were read before that write, so this pane still carries the
    // state the write just replaced, and the rollup must not see the old one.
    for reporter in window.panes.iter_mut().filter(|listed| listed.pane == pane) {
        reporter.status = state.name().to_owned();
    }
    recompute(pane, &window.panes)
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
    let Some(pane) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let mut window = tmux::window(&pane)?;
    clear_statuses(&pane, &mut window.panes, None)
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

/// Reduce the window's panes to one glyph, or to no option at all.
fn recompute(target: &str, panes: &[tmux::PaneStatus]) -> io::Result<()> {
    let states: Vec<Option<State>> = panes
        .iter()
        .map(|pane| pane.status.parse::<State>().ok())
        .collect();
    match rollup(&states) {
        Some(state) => tmux::set_window_status(target, state.icon()),
        None => tmux::clear_window_status(target),
    }
}
