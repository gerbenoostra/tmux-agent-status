//! What each command does: the policy that joins the formats to the tmux calls.
//!
//! Every command returns `io::Result<()>` so tmux I/O failures can be handled by
//! the caller. The CLI's `hook()` wrapper turns those failures into a silent exit
//! 0, because a hook must never break the agent that called it.
//!
//! Each command resolves the one pane it addresses and then sends all of its
//! writes as one tmux invocation. What those writes set is decided by the
//! server as it runs them - see `formats` - because hooks of one agent can
//! race each other, and a decision taken on a read would be stale. No command
//! reads an aggregate option into Rust before deciding what to write.
//!
//! A pane's public status is layered: the parent turn's phase, unacknowledged
//! attention, a pending clean outcome, the tracked-work ledger and the
//! accepted host session, projected to `@agent_pane_status` in the same queue.
//! The first transition on a pane migrates the legacy scalar into those
//! layers, so a pane updated by an older binary keeps its state.

use std::io;

use crate::bell;
use crate::formats::{self, Layer};
use crate::notify::NotifyAction;
use crate::state::State;
use crate::tmux::{self, Cmd, PaneId};

/// `tmux-agent-status set <state>`: report a state on the pane and recompute the window.
///
/// `waiting` and `error` ring before tmux is touched, so no tmux, or a tmux
/// that fails, costs the write and not the signal.
///
/// `done` is different: it is the clean-stop announcement, and a clean stop is
/// not final while tracked work is still running. Its queue therefore ends
/// with a `display-message` verdict the server evaluates after the writes -
/// the bell rings only when the pane reports no remaining work. A pane
/// resolution or tmux failure rings anyway, as does a repeated `done`: a
/// silently dropped completion is worse than a bell.
pub fn set(state: State, pane: Option<&str>) -> io::Result<()> {
    if state != State::Done && state.rings_bell() {
        bell::ring();
    }
    let Some(target) = tmux::resolve_pane(pane) else {
        ring_done(state);
        return Ok(());
    };
    let pane = match tmux::pane(&target) {
        Ok(pane) => pane,
        Err(err) => {
            ring_done(state);
            return Err(err);
        }
    };
    let mut commands = queue(&pane, formats::report(state), true);
    if state != State::Done {
        return tmux::run(&commands).map(drop);
    }
    commands.push(tmux::work_verdict(&pane));
    match tmux::run(&commands) {
        Ok(verdict) => {
            if !verdict.trim().is_empty() {
                bell::ring();
            }
            Ok(())
        }
        Err(err) => {
            ring_done(state);
            Err(err)
        }
    }
}

/// Ring for a `done` that could not learn its verdict - resolution or tmux
/// failing cannot say whether work remains, so the bell errs on ringing.
/// `done` is the only state this is called for; other states rang already.
fn ring_done(state: State) {
    if state == State::Done {
        bell::ring();
    }
}

/// Apply a lifecycle action to the pane: the single entry point the `notify`
/// dispatch and the session-scoped commands share.
///
/// `Report` is exactly `set`. The lifecycle actions never ring: a session
/// boundary or a work start/stop is not a turn-ending signal.
pub fn apply(action: &NotifyAction, pane: Option<&str>) -> io::Result<()> {
    match action {
        NotifyAction::Report(state) => set(*state, pane),
        NotifyAction::ResetSession { session } => {
            transition(pane, formats::reset_session(session), false)
        }
        NotifyAction::WorkStarted { key } => transition(pane, formats::work_started(key), true),
        NotifyAction::WorkStopped { key } => transition(pane, formats::work_stopped(key), true),
        NotifyAction::EndSession { session } => {
            transition(pane, formats::end_session(session), true)
        }
    }
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
/// Tracked work survives a new prompt: background items legitimately outlive
/// the turn that started them.
///
/// No bell: it opens a turn rather than ending one.
pub fn start(pane: Option<&str>) -> io::Result<()> {
    transition(pane, formats::start(), true)
}

/// `tmux-agent-status finish`: silently stop this pane's session.
///
/// `set done` without the bell. A pane holding `error` keeps it, because
/// `error` outranks a clean stop. Work the pane is still tracking keeps the
/// pane on `working` until it ends. On the pane a client is already
/// displaying the stop counts as seen - no pending completion is left -
/// because the end happened on screen.
pub fn finish(pane: Option<&str>) -> io::Result<()> {
    transition(pane, formats::finish_session(), true)
}

/// `tmux-agent-status reset`: unconditionally drop this pane's whole aggregate
/// state - layers, tracked work and the accepted host session.
pub fn reset(pane: Option<&str>) -> io::Result<()> {
    transition(pane, formats::reset(), false)
}

/// `tmux-agent-status clear-pane [<pane>]`: drop the pane's unacknowledged
/// attention and pending outcome, then recompute its window.
///
/// The acknowledgement a focus hook sends: tmux said this pane gained focus, so
/// this pane was seen. Its siblings were not, even when they share the screen,
/// and keep whatever they hold. Activity is never acknowledged away: a running
/// turn and tracked work stay visible.
///
/// The transition always runs, even when the pane holds nothing, which heals a
/// window glyph a failed write left stale.
///
/// A hook can fire for a pane that has closed since: the resolution then fails
/// and the hook wrapper turns that into a silent exit 0.
///
/// The pane is an argument because tmux's `run-shell` does not put `TMUX_PANE`
/// in a hook's environment - it does expand formats in the command, so a hook
/// passes `#{pane_id}`. Without one, `$TMUX_PANE` is used, which is what a hand
/// invocation from a pane has.
pub fn clear_pane(pane: Option<&str>) -> io::Result<()> {
    transition(pane, formats::seen(), true)
}

/// Resolve the pane, then run `layers` as one serialized queue: the legacy
/// import first when this transition migrates, the layer writes, the
/// projection and the window recompute.
fn transition(pane: Option<&str>, layers: Vec<Layer>, migrate: bool) -> io::Result<()> {
    let Some(target) = tmux::resolve_pane(pane) else {
        return Ok(());
    };
    let pane = tmux::pane(&target)?;
    tmux::run(&queue(&pane, layers, migrate)).map(drop)
}

/// The full command queue for one transition on `pane`.
fn queue(pane: &PaneId, layers: Vec<Layer>, migrate: bool) -> Vec<Cmd> {
    let mut commands = Vec::new();
    if migrate {
        for layer in formats::migrate() {
            commands.extend(write(pane, &layer));
        }
    }
    for layer in &layers {
        commands.extend(write(pane, layer));
    }
    commands.extend(write(
        pane,
        &Layer {
            option: tmux::PANE_OPTION,
            format: formats::project(),
        },
    ));
    commands.extend(recompute(pane));
    commands
}

/// The write of one layer, and the conditional unset normalising an empty
/// result back to unset.
///
/// The work ledger's canonical empty form is a bare `,` after a last removal,
/// so its normaliser treats that as empty too; every other layer only ever
/// goes empty.
fn write(pane: &PaneId, layer: &Layer) -> [Cmd; 2] {
    let normalize = if layer.option == tmux::PANE_WORK {
        tmux::unset_pane_option_if(pane, layer.option, &formats::work_gone())
    } else {
        tmux::unset_pane_option_if_empty(pane, layer.option)
    };
    [
        tmux::set_pane_option(pane, layer.option, &layer.format),
        normalize,
    ]
}

/// Recompute the glyph of the pane's window, unset when no pane holds a state.
fn recompute(pane: &PaneId) -> [Cmd; 2] {
    [
        tmux::set_window_status(pane, &formats::glyph()),
        tmux::unset_window_status_if_empty(pane),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::{HostSession, WorkKey};

    fn pane() -> PaneId {
        PaneId::parse("%3").unwrap()
    }

    /// The whole queue one transition would send, as one flat arg list per
    /// command, for greppable assertions.
    fn args_of(commands: &[Cmd]) -> Vec<String> {
        commands.iter().map(|command| command.join(" ")).collect()
    }

    /// Commands in the queue that read option state back from tmux. Only the
    /// `set done` verdict is allowed: it is an answer to the writes that just
    /// ran in the same queue, never a read the writes depend on.
    fn reads(commands: &[Cmd]) -> Vec<String> {
        commands
            .iter()
            .filter(|command| {
                matches!(
                    command.first().map(String::as_str),
                    Some("display-message" | "show-options" | "show-option" | "list-panes")
                )
            })
            .map(|command| command.join(" "))
            .collect()
    }

    #[test]
    fn no_transition_reads_state_back_to_decide_a_write() {
        let pane = pane();
        let session = HostSession::new("s").unwrap();
        let key = WorkKey::new("s", "w").unwrap();
        let queues: Vec<Vec<Cmd>> = [
            queue(&pane, formats::start(), true),
            queue(&pane, formats::seen(), true),
            queue(&pane, formats::reset(), false),
            queue(&pane, formats::reset_session(&session), false),
            queue(&pane, formats::work_started(&key), true),
            queue(&pane, formats::work_stopped(&key), true),
            queue(&pane, formats::end_session(&session), true),
            queue(&pane, formats::finish_session(), true),
        ]
        .into_iter()
        .chain(
            State::ALL
                .iter()
                .map(|s| queue(&pane, formats::report(*s), true)),
        )
        .collect();
        for queue in queues {
            assert_eq!(reads(&queue), Vec::<String>::new(), "{queue:?}");
        }
    }

    #[test]
    fn every_queue_projects_and_recomputes() {
        let pane = pane();
        let session = HostSession::new("s").unwrap();
        let key = WorkKey::new("s", "w").unwrap();
        for (layers, migrate) in [
            (formats::start(), true),
            (formats::finish(), true),
            (formats::finish_session(), true),
            (formats::seen(), true),
            (formats::reset(), false),
            (formats::reset_session(&session), false),
            (formats::work_started(&key), true),
            (formats::work_stopped(&key), true),
            (formats::end_session(&session), true),
        ] {
            let args = args_of(&queue(&pane, layers, migrate));
            let writes_pane_status = args
                .iter()
                .any(|arg| arg.contains("set-option -p -F -t %3 @agent_pane_status"));
            let writes_window = args
                .iter()
                .any(|arg| arg.contains("set-option -w -F -t %3 @agent_status"));
            assert!(writes_pane_status && writes_window, "{args:?}");
        }
    }

    #[test]
    fn a_done_queue_ends_in_the_server_verdict() {
        let pane = pane();
        let mut commands = queue(&pane, formats::report(State::Done), true);
        commands.push(tmux::work_verdict(&pane));
        let last = commands.last().unwrap().join(" ");
        let reads = reads(&commands);
        assert_eq!(reads, [last]);
        assert!(reads[0].contains("#{?@agent_pane_work,,1}"), "{reads:?}");
    }

    #[test]
    fn a_done_that_cannot_resolve_its_pane_still_rings_and_errors() {
        // `%999999` names no pane on any server, and no server at all fails
        // the same way - the bell still rings (the verdict cannot answer) and
        // the error propagates for the hook wrapper to swallow.
        assert!(set(State::Done, Some("%999999")).is_err());
    }

    #[test]
    fn migration_only_runs_for_transitions_that_import() {
        // The import writes are gated on `@agent_pane_model != 1`; a reset
        // writes the marker directly and has no gated import.
        let pane = pane();
        let imports = |commands: &[Cmd]| {
            commands
                .iter()
                .any(|command| command.join(" ").contains("#{==:#{@agent_pane_model},1}"))
        };
        assert!(imports(&queue(&pane, formats::start(), true)));
        assert!(!imports(&queue(&pane, formats::reset(), false)));
    }
}
