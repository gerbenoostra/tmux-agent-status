//! The four agent states: their rank, their names, their glyphs and whether they ring.
//!
//! Pure: this module knows nothing about tmux.

use std::fmt;
use std::str::FromStr;

/// A state an agent pane can be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// A turn is in flight. Sticky: it does not clear on focus.
    Working,
    /// The turn ended cleanly.
    Done,
    /// The turn aborted: API error, context overflow, unparseable tool call.
    Error,
    /// Blocked on the human: permission prompt, plan mode, question, idle nag.
    Waiting,
}

impl State {
    /// Every state, in rank order.
    pub const ALL: [State; 4] = [State::Working, State::Done, State::Error, State::Waiting];

    /// The rollup rank, across the panes of one window. `Working` ranks *lowest*
    /// on purpose: the glyph answers "does this window want me", and a pane that
    /// finished wants a look while one still grinding does not.
    pub fn rank(self) -> u8 {
        match self {
            State::Working => 1,
            State::Done => 2,
            State::Error => 3,
            State::Waiting => 4,
        }
    }

    /// The precedence within one pane: a state reported on a pane replaces the
    /// one it holds only if it does not rank lower here.
    ///
    /// Not the rollup rank, because it answers a different question - which of
    /// two events on the same pane you still need to see. `working` ranks lowest,
    /// so a sibling tool call finishing cannot hide a prompt that is still open.
    /// `waiting` ranks below `done`, so a nag after a finished turn cannot turn
    /// ✅ into a 💬 with nothing behind it. `error` beats everything.
    pub fn precedence(self) -> u8 {
        match self {
            State::Working => 1,
            State::Waiting => 2,
            State::Done => 3,
            State::Error => 4,
        }
    }

    /// The name the hook passes on the command line, and the value stored in
    /// `@agent_pane_status`.
    pub fn name(self) -> &'static str {
        match self {
            State::Working => "working",
            State::Done => "done",
            State::Error => "error",
            State::Waiting => "waiting",
        }
    }

    /// The glyph stored in `@agent_status` and rendered by the format term.
    ///
    /// Emoji by default: they survive a font change, which nerdfont glyphs do
    /// not. The nerdfont set and per-glyph overrides are config, and config is
    /// deferred until there is a config file.
    pub fn icon(self) -> &'static str {
        match self {
            State::Working => "🤖",
            State::Done => "✅",
            State::Error => "❗",
            State::Waiting => "💬",
        }
    }

    /// Whether reaching this state rings the terminal bell.
    ///
    /// Every turn-ending state rings. `working` fires on every tool call, and a
    /// bell per tool call is not a signal.
    pub fn rings_bell(self) -> bool {
        self != State::Working
    }

    /// Whether the state survives focusing the window.
    ///
    /// `working` is sticky, or an agent you glance at goes blank while it is
    /// still running.
    pub fn is_sticky(self) -> bool {
        self == State::Working
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A state name that is not one of the four. A bug in the caller's hook config,
/// and therefore loud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownState(String);

impl fmt::Display for UnknownState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let states = State::ALL
            .iter()
            .map(|state| state.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        write!(f, "unknown state '{}', expected one of: {states}", self.0)
    }
}

impl std::error::Error for UnknownState {}

impl FromStr for State {
    type Err = UnknownState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        State::ALL
            .into_iter()
            .find(|state| state.name() == s)
            .ok_or_else(|| UnknownState(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailWriter;

    impl fmt::Write for FailWriter {
        fn write_str(&mut self, _s: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }

    #[test]
    fn every_name_parses_back_to_its_state() {
        for state in State::ALL {
            assert_eq!(state.name().parse(), Ok(state));
        }
    }

    #[test]
    fn an_unknown_name_is_an_error() {
        assert_eq!("busy".parse::<State>(), Err(UnknownState("busy".into())));
        assert_eq!("".parse::<State>(), Err(UnknownState(String::new())));
    }

    #[test]
    fn rank_orders_working_lowest_and_waiting_highest() {
        assert!(State::Working.rank() < State::Done.rank());
        assert!(State::Done.rank() < State::Error.rank());
        assert!(State::Error.rank() < State::Waiting.rank());
    }

    #[test]
    fn precedence_orders_working_lowest_and_error_highest() {
        assert!(State::Working.precedence() < State::Waiting.precedence());
        assert!(State::Waiting.precedence() < State::Done.precedence());
        assert!(State::Done.precedence() < State::Error.precedence());
    }

    #[test]
    fn only_working_is_sticky_and_silent() {
        for state in State::ALL {
            assert_eq!(state.is_sticky(), state == State::Working);
            assert_eq!(state.rings_bell(), state != State::Working);
        }
    }

    #[test]
    fn names_and_icons_are_distinct() {
        for (i, a) in State::ALL.into_iter().enumerate() {
            for b in State::ALL.into_iter().skip(i + 1) {
                assert_ne!(a.name(), b.name());
                assert_ne!(a.icon(), b.icon());
            }
        }
    }

    #[test]
    fn ranks_are_unique_and_ordered() {
        let ranks: Vec<u8> = State::ALL.iter().map(|state| state.rank()).collect();
        let mut deduped = ranks.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            deduped.len(),
            ranks.len(),
            "each state must have a unique rank, or the rollup has no single answer"
        );
        assert!(
            ranks.windows(2).all(|w| w[0] < w[1]),
            "State::ALL must be ordered by increasing rank"
        );
    }

    #[test]
    fn unknown_state_display_propagates_write_errors() {
        let mut writer = FailWriter;
        let result = fmt::write(&mut writer, format_args!("{}", UnknownState("busy".into())));
        assert!(result.is_err());
    }
}
