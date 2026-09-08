//! The rollup, exhaustively, with no tmux anywhere near it.

use agent_status::rollup::rollup;
use agent_status::state::State::{self, Done, Error, Waiting, Working};

#[test]
fn no_panes_and_no_states_roll_up_to_nothing() {
    assert_eq!(rollup(&[]), None);
    assert_eq!(rollup(&[None]), None);
    assert_eq!(rollup(&[None, None, None]), None);
}

#[test]
fn one_state_rolls_up_to_itself() {
    for state in State::ALL {
        assert_eq!(rollup(&[Some(state)]), Some(state));
    }
}

#[test]
fn a_pane_without_a_state_does_not_contribute() {
    for state in State::ALL {
        assert_eq!(rollup(&[None, Some(state), None]), Some(state));
    }
}

#[test]
fn every_ordered_pair_rolls_up_to_the_higher_rank() {
    let expected = |a: State, b: State| if a.rank() >= b.rank() { a } else { b };
    for a in State::ALL {
        for b in State::ALL {
            assert_eq!(
                rollup(&[Some(a), Some(b)]),
                Some(expected(a, b)),
                "{a} over {b}"
            );
        }
    }
}

#[test]
fn working_ranks_below_every_state_that_ends_a_turn() {
    for ended in [Done, Error, Waiting] {
        assert_eq!(rollup(&[Some(Working), Some(ended)]), Some(ended));
    }
}

#[test]
fn three_panes_working_done_and_unset_roll_up_to_done() {
    assert_eq!(rollup(&[Some(Working), Some(Done), None]), Some(Done));
}

#[test]
fn clearing_the_higher_ranked_pane_reveals_the_lower_one() {
    assert_eq!(rollup(&[Some(Waiting), Some(Working)]), Some(Waiting));
    assert_eq!(rollup(&[None, Some(Working)]), Some(Working));
}
