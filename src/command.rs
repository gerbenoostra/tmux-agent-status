//! What each command does: the policy that joins the formats to the tmux calls.
//!
//! Every command returns `io::Result<()>` so tmux I/O failures can be handled by
//! the caller. The CLI's `hook()` wrapper turns those failures into a silent exit
//! 0, because a hook must never break the agent that called it.
//!
//! Each command reads the window once, to learn which panes to write, and then
//! sends all of its writes as one tmux invocation. What those writes set is
//! decided by the server as it runs them - see `formats` - because hooks of one
//! agent can race each other, and a decision taken on the read would be stale.

use std::io;

use crate::bell;
use crate::formats;
use crate::state::State;
use crate::tmux::{self, Cmd, PaneId};

/// `tmux-agent-status set <state>`: report a state on the pane and recompute the window.
///
/// The bell rings for every state that rings, whether or not the pane keeps it.
/// A `waiting` refused by a `done` still means the agent is blocked on you, and
/// precedence keeps the glyph on `done`; the bell is the one channel that still
/// says so. It is rung before tmux is touched, so no tmux, or a tmux that
/// fails, costs the write and not the signal.
pub fn set(state: State, pane: Option<&str>) -> io::Result<()> {
    if state.rings_bell() {
        bell::ring();
    }
    let Some(target) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&target)?;
    tmux::run(&report(&window.pane, state)).map(drop)
}

/// `tmux-agent-status start`: a turn begins on this pane.
///
/// The one write that does not defer to what the pane already holds. It is
/// reported by the event that means the human typed a prompt, and typing into a
/// pane is seeing it, so whatever the last turn left there - a `done` no window
/// switch ever cleared, an `error` - is over. Without it, a state left by the
/// last turn would outrank every state of this one, and the whole turn would
/// render as the last one's ending.
///
/// No bell: it opens a turn rather than ending one.
pub fn start(pane: Option<&str>) -> io::Result<()> {
    let Some(target) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&target)?;
    let mut commands = write(&window.pane, State::Working.name()).to_vec();
    commands.extend(recompute(&window.pane));
    tmux::run(&commands).map(drop)
}

/// `tmux-agent-status finish`: silently resolve this pane's session to done.
///
/// `set done` without the bell. A pane holding `error` keeps it, because `error`
/// outranks `done`.
pub fn finish(pane: Option<&str>) -> io::Result<()> {
    let Some(target) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&target)?;
    tmux::run(&report(&window.pane, State::Done)).map(drop)
}

/// `tmux-agent-status reset`: unconditionally drop this pane's session status.
pub fn reset(pane: Option<&str>) -> io::Result<()> {
    let Some(target) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&target)?;
    let mut commands = write(&window.pane, "").to_vec();
    commands.extend(recompute(&window.pane));
    tmux::run(&commands).map(drop)
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
    let Some(target) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&target)?;
    let mut commands: Vec<Cmd> = window
        .panes_with_status()
        .flat_map(|pane| write(pane, &formats::seen()))
        .collect();
    commands.extend(recompute(&window.pane));
    tmux::run(&commands).map(drop)
}

/// `tmux-agent-status clear-pane [<pane>]`: drop the non-sticky state of that
/// one pane, then recompute its window.
///
/// The acknowledgement a focus hook sends: tmux said this pane gained focus, so
/// this pane was seen. Its siblings were not, even when they share the screen,
/// and keep whatever they hold. `working` is sticky and survives.
///
/// The recompute runs even when the pane holds nothing, which heals a window
/// glyph a failed write left stale and keeps the command list non-empty.
///
/// A hook can fire for a pane that has closed since: the read then fails and
/// the hook wrapper turns that into a silent exit 0, as for `clear_window`.
/// The pane is an argument for the reason `clear_window` documents.
pub fn clear_pane(pane: Option<&str>) -> io::Result<()> {
    let Some(target) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let window = tmux::window(&target)?;
    let mut commands: Vec<Cmd> = window
        .addressed_with_status()
        .into_iter()
        .flat_map(|pane| write(pane, &formats::seen()))
        .collect();
    commands.extend(recompute(&window.pane));
    tmux::run(&commands).map(drop)
}

/// The writes that report `state` on `pane`, and only that pane.
fn report(pane: &PaneId, state: State) -> Vec<Cmd> {
    let mut commands = write(pane, &formats::report(state)).to_vec();
    commands.extend(recompute(pane));
    commands
}

/// Set a pane's status to what `format` expands to, unset if that is nothing.
fn write(pane: &PaneId, format: &str) -> [Cmd; 2] {
    [
        tmux::set_pane_status(pane, format),
        tmux::unset_pane_status_if_empty(pane),
    ]
}

/// Recompute the glyph of the pane's window, unset when no pane holds a state.
fn recompute(pane: &PaneId) -> [Cmd; 2] {
    [
        tmux::set_window_status(pane, &formats::glyph()),
        tmux::unset_window_status_if_empty(pane),
    ]
}
