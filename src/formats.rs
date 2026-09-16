//! The tmux formats that decide every write, expanded by the server as it writes.
//!
//! Pure: this module builds strings and never runs tmux.
//!
//! An agent may run its hooks concurrently, so a decision taken on a value read
//! in an earlier tmux call can be stale by the time it is written. Every
//! decision that depends on what tmux holds - which state wins on a pane,
//! whether the window is on screen, what the window's glyph is - is therefore a
//! format that `set-option -F` expands against its target at the moment it
//! sets it, and the server runs one command at a time. See
//! `tasks/plans/013-pane-state-precedence.md`.

use crate::state::State;
use crate::tmux::PANE_OPTION;

/// The pane's own status, as the server holds it when the format expands.
fn status() -> String {
    format!("#{{{PANE_OPTION}}}")
}

/// `1` when the window is on screen: its session's current window, with a
/// client attached to look at it. `session_attached` counts clients, and tmux
/// reads `0` as false.
const WATCHED: &str = "#{?window_active,#{?session_attached,1,},}";

/// `then` when the pane holds `state`, `otherwise` when it does not.
fn if_holds(state: State, then: &str, otherwise: &str) -> String {
    format!(
        "#{{?#{{==:{},{}}},{then},{otherwise}}}",
        status(),
        state.name()
    )
}

/// The value a pane takes when `state` is reported on it.
///
/// A state that outranks `state` in precedence keeps itself; anything else - a
/// lower state, nothing, or a value this tool does not recognise - becomes
/// `state`. A state that clears on focus clears the pane outright on a watched
/// window instead: whoever it is for is already looking, and the pane's old
/// state is over, whatever it was.
pub fn report(state: State) -> String {
    let kept = State::ALL
        .into_iter()
        .filter(|held| held.precedence() > state.precedence())
        .fold(state.name().to_owned(), |otherwise, held| {
            if_holds(held, held.name(), &otherwise)
        });
    if state.is_sticky() {
        kept
    } else {
        format!("#{{?{WATCHED},,{kept}}}")
    }
}

/// The value a pane keeps once its window is seen: nothing in place of a state
/// that clears on focus, and whatever it holds otherwise, so a sticky state and
/// a value this tool does not recognise both survive.
pub fn seen() -> String {
    State::ALL
        .into_iter()
        .filter(|state| !state.is_sticky())
        .fold(status(), |otherwise, state| if_holds(state, "", &otherwise))
}

/// The value a sibling of the reporting pane takes: [`seen`] when the window is
/// watched, and what it already holds when it is not.
pub fn sibling() -> String {
    format!("#{{?{WATCHED},{},{}}}", seen(), status())
}

/// The window's glyph: the icon of the highest-ranked state any of its panes
/// holds, or nothing.
///
/// A loop over the panes writes one rank digit per pane holding a recognised
/// state, and the glyph is picked from the highest rank down. The digits come
/// only from the loop body, so no value a pane holds can pass for one.
pub fn glyph() -> String {
    let digits: String = State::ALL
        .into_iter()
        .map(|state| if_holds(state, &state.rank().to_string(), ""))
        .collect();
    let panes = format!("#{{P:{digits}}}");
    let mut by_rank = State::ALL;
    by_rank.sort_by_key(|state| state.rank());
    by_rank.into_iter().fold(String::new(), |otherwise, state| {
        format!(
            "#{{?#{{m:*{}*,{panes}}},{},{otherwise}}}",
            state.rank(),
            state.icon()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests against a real server are what prove these formats right;
    /// these pin the properties a format silently breaks on.
    #[test]
    fn a_report_checks_exactly_the_states_that_outrank_it() {
        for state in State::ALL {
            let format = report(state);
            for other in State::ALL {
                let checked = format.contains(&format!("#{{==:{},{}}}", status(), other.name()));
                assert_eq!(
                    checked,
                    other.precedence() > state.precedence(),
                    "report({state}) and {other}: {format}"
                );
            }
        }
    }

    #[test]
    fn only_a_state_that_clears_on_focus_looks_at_the_window() {
        for state in State::ALL {
            assert_eq!(
                report(state).contains(WATCHED),
                !state.is_sticky(),
                "report({state})"
            );
        }
    }

    #[test]
    fn ranks_are_single_digits() {
        // `*4*` would also match a `14`.
        for state in State::ALL {
            assert!(state.rank() < 10, "{state}");
        }
    }

    #[test]
    fn names_and_icons_cannot_break_a_format() {
        for state in State::ALL {
            for spliced in [state.name(), state.icon()] {
                assert!(
                    !spliced.contains([',', '#', '{', '}']),
                    "{state}: {spliced}"
                );
            }
        }
    }
}
