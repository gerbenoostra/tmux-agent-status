//! Reduce a window's per-pane states to the one state its window entry has room for.
//!
//! Pure: this module knows nothing about tmux.

use crate::state::State;

/// The maximum by rank over the window's panes, or `None` when no pane has a state.
///
/// `None` for a pane means "this pane has no state". That is deliberately not
/// the same as a pane inheriting the window's state: tmux option inheritance
/// makes an unset pane option read back as the window's value, which is why the
/// pane option and the window rollup have different names.
pub fn rollup(panes: &[Option<State>]) -> Option<State> {
    panes.iter().copied().flatten().max()
}
