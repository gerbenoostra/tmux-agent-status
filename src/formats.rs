//! The tmux formats that decide every write, expanded by the server as it writes.
//!
//! Pure: this module builds strings and never runs tmux.
//!
//! An agent may run its hooks concurrently, so a decision taken on a value read
//! in an earlier tmux call can be stale by the time it is written. Every
//! decision that depends on what tmux holds - which layer value wins, what the
//! projection and window glyph are - is therefore a format that
//! `set-option -F` expands against its target at the moment it sets it, and the
//! server runs one command at a time. That makes each write a compare-and-set
//! taken inside the server: no lock file, no read-then-write race. A `-F`
//! write can only produce a value, so a clear produces `""` and is then
//! normalised to unset by a conditional `if-shell -F ... 'set-option -u'` that
//! decides on the value current at that instant - an unconditional unset could
//! erase a concurrent write that landed in between.
//!
//! The pane state an agent reports is layered, not scalar:
//!
//! - `@agent_pane_root`: the parent turn's phase - `working`, `stopped`, or
//!   `settling` (the last tracked item has stopped and the host's automatic
//!   turn is expected to end with a root stop).
//! - `@agent_pane_attention`: an unacknowledged `waiting` or `error`.
//! - `@agent_pane_completion`: `pending` for a clean stop nobody has seen.
//! - `@agent_pane_work`: the ledger of tracked work items, `,token,...,` or
//!   unset. Tokens are lowercase hex, so the `,token,` substring is an exact
//!   membership test and `s|,token,|,|` an exact removal.
//! - `@agent_pane_host_session`: the hex-encoded host session the lifecycle
//!   events must match.
//! - `@agent_pane_model`: `1` once the layers have been initialised; before
//!   that, the first transition imports the legacy `@agent_pane_status`.
//!
//! `@agent_pane_status` remains the only public pane value: it is `project`ed
//! from the layers inside the same queue that mutated them.

use crate::notify::{HostSession, WorkKey};
use crate::state::State;
use crate::tmux::{
    PANE_ATTENTION, PANE_COMPLETION, PANE_HOST_SESSION, PANE_MODEL, PANE_OPTION, PANE_ROOT,
    PANE_WORK,
};

/// One layer write of a transition: the pane option and the format computing
/// its new value from what the server holds when the write runs.
#[derive(Debug)]
pub struct Layer {
    pub option: &'static str,
    pub format: String,
}

/// A layer write on `option` produced by `format`.
fn layer(option: &'static str, format: impl Into<String>) -> Layer {
    Layer {
        option,
        format: format.into(),
    }
}

/// The current value of `option`, expanded where the format is used.
fn opt(option: &'static str) -> String {
    format!("#{{{option}}}")
}

fn status() -> String {
    opt(PANE_OPTION)
}
fn root() -> String {
    opt(PANE_ROOT)
}
fn attention() -> String {
    opt(PANE_ATTENTION)
}
fn completion() -> String {
    opt(PANE_COMPLETION)
}
fn work() -> String {
    opt(PANE_WORK)
}
fn host_session() -> String {
    opt(PANE_HOST_SESSION)
}
fn model() -> String {
    opt(PANE_MODEL)
}

/// `a == b`, expanded where the format is used.
fn eq(a: String, b: &str) -> String {
    format!("#{{==:{a},{b}}}")
}

/// `then` when `condition` expands true, `otherwise` when it does not.
fn gate(condition: String, then: &str, otherwise: String) -> String {
    format!("#{{?{condition},{then},{otherwise}}}")
}

/// Any of `conditions`.
fn any(conditions: &[String]) -> String {
    let mut it = conditions.iter().rev();
    let last = it.next().cloned().unwrap_or_default();
    it.fold(last, |rest, c| format!("#{{||:{c},{rest}}}"))
}

/// All of `conditions`.
fn all(conditions: &[String]) -> String {
    let mut it = conditions.iter().rev();
    let last = it.next().cloned().unwrap_or_default();
    it.fold(last, |rest, c| format!("#{{&&:{c},{rest}}}"))
}

/// The layered equivalent of the legacy scalar status, applied only while
/// `@agent_pane_model` does not mark the pane migrated.
///
/// An un-migrated pane has no ledger and no accepted session, so those two
/// layers need no import. The marker itself is written after the imports, so
/// every import in the queue still sees the legacy value.
pub fn migrate() -> Vec<Layer> {
    let migrated = eq(model(), "1");
    let legacy = status();
    let terminal = any(&[
        eq(legacy.clone(), "done"),
        eq(legacy.clone(), "waiting"),
        eq(legacy.clone(), "error"),
    ]);
    let root_import = format!(
        "#{{?{},working,#{{?{terminal},stopped,}}}}",
        eq(legacy.clone(), "working")
    );
    let attention_import = format!(
        "#{{?{},waiting,#{{?{},error,}}}}",
        eq(legacy.clone(), "waiting"),
        eq(legacy, "error")
    );
    let completion_import = format!("#{{?{},pending,}}", eq(status(), "done"));
    vec![
        layer(PANE_ROOT, gate(migrated.clone(), &root(), root_import)),
        layer(
            PANE_ATTENTION,
            gate(migrated.clone(), &attention(), attention_import),
        ),
        layer(
            PANE_COMPLETION,
            gate(migrated.clone(), &completion(), completion_import),
        ),
        layer(PANE_MODEL, "1"),
    ]
}

/// The public pane state the layers project to, in precedence order:
/// unacknowledged attention, then activity (a `working` or `settling` root, or
/// tracked work remaining), then a pending clean stop, then nothing.
pub fn project() -> String {
    let activity = any(&[eq(root(), "working"), eq(root(), "settling"), work()]);
    let pending_stop = all(&[eq(root(), "stopped"), eq(completion(), "pending")]);
    format!(
        "#{{?{a},{a},#{{?{activity},working,#{{?{pending_stop},done,}}}}}}",
        a = attention()
    )
}

/// `1` while the pane shows a clean stop nobody has seen and nothing is
/// active: a stopped root, a pending completion and no tracked work.
fn done_shown() -> String {
    all(&[
        eq(root(), "stopped"),
        eq(completion(), "pending"),
        work_gone(),
    ])
}

/// The layer writes for `set <state>` and `Report(state)`.
///
/// A `working` or `waiting` that arrives on a shown ✅ is refused: the turn
/// already ended cleanly, so it is a straggler tool event that nothing would
/// ever end, or an idle nag with no question behind it. Only tracked work can
/// put activity or attention over a clean stop. `waiting` still rings.
///
/// The `waiting` attention write runs before its root write, so a `settling`
/// or `working` root it is about to stop is not mistaken for a shown ✅.
pub fn report(state: State) -> Vec<Layer> {
    match state {
        State::Working => vec![layer(
            PANE_ROOT,
            gate(done_shown(), &root(), "working".to_owned()),
        )],
        State::Waiting => vec![
            layer(
                PANE_ATTENTION,
                gate(
                    done_shown(),
                    &attention(),
                    gate(eq(attention(), "error"), "error", "waiting".to_owned()),
                ),
            ),
            layer(PANE_ROOT, "stopped"),
        ],
        State::Error => vec![layer(PANE_ROOT, "stopped"), layer(PANE_ATTENTION, "error")],
        State::Done => finish(),
    }
}

/// The layer writes for `start`: a new turn acknowledges whatever the last one
/// left, but work that outlives a turn is preserved.
pub fn start() -> Vec<Layer> {
    vec![
        layer(PANE_ROOT, "working"),
        layer(PANE_ATTENTION, ""),
        layer(PANE_COMPLETION, ""),
    ]
}

/// The layer writes of a clean stop: `set done` and `Report(done)`.
///
/// `error` keeps standing - it outranks a clean stop - while `waiting` is
/// replaced: the turn could only end once the input was given.
pub fn finish() -> Vec<Layer> {
    vec![
        layer(PANE_ROOT, "stopped"),
        layer(
            PANE_ATTENTION,
            gate(eq(attention(), "error"), "error", String::new()),
        ),
        layer(PANE_COMPLETION, "pending"),
    ]
}

/// `1` while a client displays this pane: the pane is selected in its
/// window, that window is its session's current one, and a client is
/// attached to that session. `pane_active` alone also holds for the selected
/// pane of a background window, and an attached client shares its session's
/// current window, so only all three terms mean "on screen".
fn displayed() -> String {
    all(&[
        "#{pane_active}".to_owned(),
        "#{window_active}".to_owned(),
        "#{session_attached}".to_owned(),
    ])
}

/// The layer writes of a session-ending clean stop: `finish`, and a
/// `finish --agent <name> --stdin` whose payload maps to nothing.
///
/// The same clean stop as the turn-end `finish` formatter, except on the
/// pane a client is already displaying: that stop was seen as it happened,
/// so the completion stays empty instead of `pending` and nothing remains to
/// acknowledge.
pub fn finish_session() -> Vec<Layer> {
    vec![
        layer(PANE_ROOT, "stopped"),
        layer(
            PANE_ATTENTION,
            gate(eq(attention(), "error"), "error", String::new()),
        ),
        layer(PANE_COMPLETION, gate(displayed(), "", "pending".to_owned())),
    ]
}

/// The layer writes for `clear-pane`: focus acknowledges attention and a
/// pending outcome; it never clears activity.
pub fn seen() -> Vec<Layer> {
    vec![layer(PANE_ATTENTION, ""), layer(PANE_COMPLETION, "")]
}

/// The layer writes for `reset`: every layer dropped, the pane marked
/// initialised so no later transition imports a legacy scalar.
pub fn reset() -> Vec<Layer> {
    vec![
        layer(PANE_ROOT, ""),
        layer(PANE_ATTENTION, ""),
        layer(PANE_COMPLETION, ""),
        layer(PANE_WORK, ""),
        layer(PANE_HOST_SESSION, ""),
        layer(PANE_MODEL, "1"),
    ]
}

/// The layer writes for `ResetSession`: the aggregate dropped and `session`
/// accepted as the host session every later lifecycle event must match.
pub fn reset_session(session: &HostSession) -> Vec<Layer> {
    vec![
        layer(PANE_ROOT, ""),
        layer(PANE_ATTENTION, ""),
        layer(PANE_COMPLETION, ""),
        layer(PANE_WORK, ""),
        layer(PANE_HOST_SESSION, session.encoded().to_owned()),
        layer(PANE_MODEL, "1"),
    ]
}

/// `1` when the event's session is the accepted host session; lifecycle events
/// from any other session are a no-op.
fn accepted(session: &HostSession) -> String {
    eq(host_session(), session.encoded())
}

/// The layer writes for `WorkStarted`: an exact-add of the key's token to the
/// ledger, so a duplicate start is a no-op. The first token opens the ledger
/// with its leading comma; later tokens append after the trailing one.
pub fn work_started(key: &WorkKey) -> Vec<Layer> {
    let ledger = work();
    let token = key.encoded();
    let removed = format!("#{{s|,{token},|,|:{ledger}}}");
    let absent = eq(removed, &ledger);
    let append = format!("#{{?{ledger},{ledger},#,}}{token}#,");
    let add = gate(
        accepted(key.session()),
        &gate(absent, &append, ledger.clone()),
        ledger,
    );
    vec![layer(PANE_WORK, add)]
}

/// The layer writes for `WorkStopped`: an exact-remove of the key's token, so
/// an unmatched or foreign-session stop is a no-op.
///
/// When the removal empties the ledger under a `stopped` root, the root enters
/// `settling`: the pane stays on `working` through the host's automatic turn,
/// whose own clean stop is what exposes `done`. The root write goes first, so
/// its format still sees the old ledger: "the last item stopped" means the
/// token was in it and taking it out left nothing. A stop whose token was
/// never there - a helper's `SubagentStop`, a redelivered event - must not
/// settle a root whose `done` is already shown.
pub fn work_stopped(key: &WorkKey) -> Vec<Layer> {
    let ledger = work();
    let session_matches = accepted(key.session());
    let removed = format!("#{{s|,{key},|,|:{ledger}}}", key = key.encoded());
    let removed_the_last = all(&[
        session_matches.clone(),
        format!("#{{!=:{removed},{ledger}}}"),
        any(&[eq(removed.clone(), ""), eq(removed.clone(), "#,")]),
        eq(root(), "stopped"),
    ]);
    vec![
        layer(PANE_ROOT, gate(removed_the_last, "settling", root())),
        layer(PANE_WORK, gate(session_matches, &removed, ledger)),
    ]
}

/// `1` when the ledger holds no tracked work: unset, empty, or the bare `,` a
/// last removal leaves behind before its normalisation command runs.
pub fn work_gone() -> String {
    any(&[eq(work(), ""), eq(work(), "#,")])
}

/// The layer writes for `EndSession` of the accepted session: the silent clean
/// stop, plus the ledger cleared - a host whose lifecycle ends has no work to
/// keep tracking. An `EndSession` from any other session is a no-op. On the
/// pane a client is already displaying the stop was seen as it happened, so
/// the completion stays empty instead of `pending`.
pub fn end_session(session: &HostSession) -> Vec<Layer> {
    let matches = accepted(session);
    vec![
        layer(PANE_ROOT, gate(matches.clone(), "stopped", root())),
        layer(
            PANE_ATTENTION,
            gate(
                matches.clone(),
                &gate(eq(attention(), "error"), "error", String::new()),
                attention(),
            ),
        ),
        layer(
            PANE_COMPLETION,
            gate(
                matches.clone(),
                &gate(displayed(), "", "pending".to_owned()),
                completion(),
            ),
        ),
        layer(PANE_WORK, gate(matches, "", work())),
    ]
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

/// `then` when the pane's public status holds `state`, `otherwise` when it
/// does not.
fn if_holds(state: State, then: &str, otherwise: &str) -> String {
    format!(
        "#{{?#{{==:{},{}}},{then},{otherwise}}}",
        status(),
        state.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests against a real server are what prove these formats right;
    /// these pin the properties a format silently breaks on.

    #[test]
    fn the_projection_orders_attention_then_activity_then_pending_done() {
        let p = project();
        let attention = p.find(&attention()).unwrap();
        let activity = p.find("working").unwrap();
        let done = p.rfind("done").unwrap();
        assert!(attention < activity && activity < done, "{p}");
    }

    #[test]
    fn a_work_start_is_session_gated_and_idempotent() {
        let key = WorkKey::new("s", "w").unwrap();
        let writes = work_started(&key);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].option, PANE_WORK);
        let format = &writes[0].format;
        assert!(
            format.contains(&eq(host_session(), key.session().encoded())),
            "{format}"
        );
        // The token is added only when removing it is already a no-op.
        assert!(
            format.contains(&format!("#{{s|,{},|,|:", key.encoded())),
            "{format}"
        );
    }

    #[test]
    fn a_work_stop_settles_only_when_it_removed_the_last_token() {
        let key = WorkKey::new("s", "w").unwrap();
        let writes = work_stopped(&key);
        // The root layer comes first: it decides on the ledger as it was
        // before the removal write, so a stop that removed nothing - an
        // unmatched or duplicate stop - cannot settle a stopped root.
        let settle = &writes[0];
        assert_eq!(settle.option, PANE_ROOT);
        for fragment in ["settling", &eq(root(), "stopped"), "#{!=:"] {
            assert!(settle.format.contains(fragment), "{settle:?}");
        }
        assert_eq!(writes[1].option, PANE_WORK);
    }

    #[test]
    fn session_gated_writes_preserve_the_current_value_on_a_mismatch() {
        let other = HostSession::new("other").unwrap();
        for writes in [
            work_stopped(&WorkKey::new("other", "w").unwrap()),
            end_session(&other),
        ] {
            for write in writes {
                assert!(
                    write.format.ends_with(&format!("{}}}", opt(write.option))),
                    "{}: {write:?}",
                    write.option
                );
            }
        }
    }

    #[test]
    fn migration_imports_every_legacy_scalar() {
        let writes = migrate();
        let root = &writes[0].format;
        for (legacy, layer_value) in [
            ("working", "working"),
            ("done", "stopped"),
            ("waiting", "stopped"),
            ("error", "stopped"),
        ] {
            assert!(
                root.contains(&eq(status(), legacy)),
                "{legacy} -> {layer_value}: {root}"
            );
        }
        assert_eq!(writes[3].option, PANE_MODEL);
        assert_eq!(writes[3].format, "1");
    }

    /// `pane_active`, `window_active` and `session_attached` together mean a
    /// client is displaying the pane; only the session-end formats may use
    /// them, to write the already-seen stop.
    #[test]
    fn only_the_completion_layer_of_a_session_end_consults_the_display() {
        let session = HostSession::new("s").unwrap();
        for writes in [finish_session(), end_session(&session)] {
            for write in &writes {
                for term in ["pane_active", "window_active", "session_attached"] {
                    if write.option == PANE_COMPLETION {
                        assert!(write.format.contains(term), "{term}: {write:?}");
                    } else {
                        assert!(!write.format.contains(term), "{term}: {write:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_session_end_writes_pending_completion_only_off_screen() {
        assert_eq!(
            displayed(),
            "#{&&:#{pane_active},#{&&:#{window_active},#{session_attached}}}"
        );
        let seen = gate(displayed(), "", "pending".to_owned());
        let writes = finish_session();
        assert_eq!(writes[0].option, PANE_ROOT);
        assert_eq!(writes[0].format, "stopped");
        assert_eq!(writes[1].option, PANE_ATTENTION);
        assert_eq!(
            writes[1].format,
            gate(eq(attention(), "error"), "error", String::new())
        );
        assert_eq!(writes[2].option, PANE_COMPLETION);
        assert_eq!(writes[2].format, seen);

        // EndSession makes the same choice, inside its session gate.
        let session = HostSession::new("s").unwrap();
        let write = end_session(&session)
            .into_iter()
            .find(|write| write.option == PANE_COMPLETION)
            .expect("end_session writes a completion");
        assert!(
            write
                .format
                .starts_with(&format!("#{{?{},", accepted(&session))),
            "{write:?}"
        );
        assert!(write.format.contains(&seen), "{write:?}");
        assert!(
            write.format.ends_with(&format!("{}}}", completion())),
            "{write:?}"
        );
    }

    #[test]
    fn no_other_format_consults_whether_the_pane_is_displayed() {
        let key = WorkKey::new("s", "w").unwrap();
        let session = HostSession::new("s").unwrap();
        let mut formats = vec![project(), glyph(), work_gone()];
        for writes in [
            start(),
            seen(),
            reset(),
            reset_session(&session),
            work_started(&key),
            work_stopped(&key),
            migrate(),
            finish(),
        ] {
            formats.extend(writes.into_iter().map(|write| write.format));
        }
        formats.extend(
            State::ALL
                .iter()
                .flat_map(|s| report(*s).into_iter().map(|write| write.format)),
        );
        for format in formats {
            for term in ["pane_active", "window_active", "session_attached"] {
                assert!(!format.contains(term), "{term}: {format}");
            }
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
