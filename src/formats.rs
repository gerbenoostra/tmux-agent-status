//! The tmux formats that decide every write, expanded by the server as it writes.
//!
//! Pure: this module builds strings and never runs tmux.
//!
//! An agent may run its hooks concurrently, so a decision taken on a value read
//! in an earlier tmux call can be stale by the time it is written. Every
//! decision that depends on what tmux holds - which state wins on a pane,
//! what the window's glyph is - is therefore a
//! format that `set-option -F` expands against its target at the moment it
//! sets it, and the server runs one command at a time. That makes each write a
//! compare-and-set taken inside the server: no lock file, no read-then-write
//! race. A `-F` write can only produce a value, so a clear produces `""` and is
//! then normalised to unset by a conditional `if-shell -F ... 'set-option -u'`
//! that decides on the value current at that instant - an unconditional unset
//! could erase a concurrent write that landed in between.

use crate::state::State;
use crate::tmux::PANE_OPTION;

/// The pane's own status, as the server holds it when the format expands.
fn status() -> String {
    format!("#{{{PANE_OPTION}}}")
}

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
/// `state`. Whether anyone is looking is never consulted: tmux cannot tell a
/// focused terminal from a background tab, so the glyph is always written and
/// only a focus event on the pane clears it.
pub fn report(state: State) -> String {
    State::ALL
        .into_iter()
        .filter(|held| held.precedence() > state.precedence())
        .fold(state.name().to_owned(), |otherwise, held| {
            if_holds(held, held.name(), &otherwise)
        })
}

/// The value a pane keeps once it is acknowledged: nothing in place of a state
/// that clears on focus, and whatever it holds otherwise, so a sticky state and
/// a value this tool does not recognise both survive.
pub fn seen() -> String {
    State::ALL
        .into_iter()
        .filter(|state| !state.is_sticky())
        .fold(status(), |otherwise, state| if_holds(state, "", &otherwise))
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
    fn no_report_consults_whether_the_window_is_on_screen() {
        for state in State::ALL {
            let format = report(state);
            assert!(
                !format.contains("window_active"),
                "report({state}): {format}"
            );
            assert!(
                !format.contains("session_attached"),
                "report({state}): {format}"
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
