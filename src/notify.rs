//! Map a single JSON payload to a lifecycle action for agents that deliver one
//! callback per event.
//!
//! The agent vocabulary lives here, not in the caller, so a shell snippet never
//! has to parse JSON or depend on `jq`. An unrecognised payload returns `None`,
//! which the CLI turns into a silent exit 0.
//!
//! Each shape-B agent is a row in a data table. The table maps one JSON string
//! field to a `State` report; adding a new agent is a new row, not a new code
//! path.

use crate::state::State;

/// The most bytes a host session ID or work ID may carry.
const MAX_ID_BYTES: usize = 256;

/// One lifecycle action a host payload asks for.
///
/// `Report` is the shape-B form every agent can produce. The lifecycle variants
/// are what a tracked-aggregate adapter maps its probed events to; a payload
/// that cannot supply them is a silent drop or the matching generic command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyAction {
    /// Report one of the four public states, applied as `set` would.
    Report(State),
    /// A lifecycle-aware session start: clear the pane's aggregate state and
    /// accept this host session.
    ResetSession { session: HostSession },
    /// A tracked work item started under the accepted host session.
    WorkStarted { key: WorkKey },
    /// A tracked work item stopped under the accepted host session.
    WorkStopped { key: WorkKey },
    /// The accepted host session ended; its tracked work ended with it.
    EndSession { session: HostSession },
}

/// A host session ID, stored hex-encoded so it can be compared inside a tmux
/// format without giving the payload a way to inject format syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSession {
    encoded: String,
}

impl HostSession {
    /// A session ID worth tracking, or `None` for one that is empty or over
    /// the byte cap.
    pub fn new(session_id: &str) -> Option<HostSession> {
        valid_id(session_id).then(|| HostSession {
            encoded: hex(session_id),
        })
    }

    /// The lowercase hex `@agent_pane_host_session` compares against.
    pub fn encoded(&self) -> &str {
        &self.encoded
    }
}

/// The composite key of one tracked work item: the host session and the work
/// ID, encoded so delimiters and tmux format syntax cannot be injected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkKey {
    session: HostSession,
    encoded: String,
}

impl WorkKey {
    /// The work key of `work_id` reported under `session_id`, or `None` when
    /// either ID is empty or over the byte cap.
    pub fn new(session_id: &str, work_id: &str) -> Option<WorkKey> {
        if !valid_id(session_id) || !valid_id(work_id) {
            return None;
        }
        let mut composite = String::with_capacity(session_id.len() + 1 + work_id.len());
        composite.push_str(session_id);
        composite.push('\0');
        composite.push_str(work_id);
        Some(WorkKey {
            session: HostSession {
                encoded: hex(session_id),
            },
            encoded: hex(&composite),
        })
    }

    /// The host session this key is scoped to.
    pub fn session(&self) -> &HostSession {
        &self.session
    }

    /// The lowercase hex of `session_id`, one NUL byte and `work_id`: the
    /// token the work ledger stores.
    pub fn encoded(&self) -> &str {
        &self.encoded
    }
}

/// A usable ID is non-empty and at most [`MAX_ID_BYTES`] bytes.
fn valid_id(raw: &str) -> bool {
    !raw.is_empty() && raw.len() <= MAX_ID_BYTES
}

/// Lowercase hex of `raw`'s bytes, without pulling in a hex crate.
fn hex(raw: &str) -> String {
    use std::fmt::Write;
    raw.bytes()
        .fold(String::with_capacity(raw.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// One shape-B agent's mapping: which JSON field holds the event and which
/// events map to which state.
struct AgentMapping {
    name: &'static str,
    event_field: &'static str,
    table: &'static [(&'static str, State)],
}

const AGENTS: &[AgentMapping] = &[AgentMapping {
    name: "mistral-vibe",
    event_field: "hook_event_name",
    table: &[
        ("pre_tool", State::Working),
        ("post_tool", State::Working),
        ("post_agent", State::Done),
    ],
}];

/// Map `payload` for `agent` to an action, or `None` when the event carries no
/// recognised lifecycle signal.
pub fn dispatch(agent: &str, payload: &str) -> Option<NotifyAction> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let mapping = AGENTS.iter().find(|m| m.name == agent)?;
    let event = value.get(mapping.event_field)?.as_str()?;
    mapping
        .table
        .iter()
        .find(|(name, _)| *name == event)
        .map(|(_, state)| NotifyAction::Report(*state))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Some(Report(state))` reads as `Some(state)` in these tests.
    fn report(state: State) -> Option<NotifyAction> {
        Some(NotifyAction::Report(state))
    }

    #[test]
    fn mistral_vibe_pre_tool_is_working() {
        let payload = r#"{"hook_event_name":"pre_tool"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), report(State::Working));
    }

    #[test]
    fn mistral_vibe_post_tool_failure_stays_working() {
        let payload = r#"{"hook_event_name":"post_tool","tool_status":"failure"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), report(State::Working));
    }

    #[test]
    fn mistral_vibe_post_tool_success_stays_working() {
        let payload = r#"{"hook_event_name":"post_tool","tool_status":"success"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), report(State::Working));
    }

    #[test]
    fn mistral_vibe_post_agent_is_done() {
        let payload = r#"{"hook_event_name":"post_agent"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), report(State::Done));
    }

    #[test]
    fn mistral_vibe_unknown_event_is_dropped() {
        let payload = r#"{"hook_event_name":"unknown"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), None);
    }

    #[test]
    fn mistral_vibe_missing_event_name_is_dropped() {
        let payload = r#"{"tool_status":"failure"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), None);
    }

    #[test]
    fn mistral_vibe_non_string_event_name_is_dropped() {
        let payload = r#"{"hook_event_name":123}"#;
        assert_eq!(dispatch("mistral-vibe", payload), None);
    }

    #[test]
    fn mistral_vibe_post_tool_ignores_the_tool_status() {
        // Every tool outcome is the same turn still running, so a missing or
        // malformed `tool_status` costs nothing.
        for payload in [
            r#"{"hook_event_name":"post_tool"}"#,
            r#"{"hook_event_name":"post_tool","tool_status":false}"#,
        ] {
            assert_eq!(
                dispatch("mistral-vibe", payload),
                report(State::Working),
                "payload {payload}"
            );
        }
    }

    #[test]
    fn unknown_agent_is_dropped() {
        assert_eq!(dispatch("no-such-agent", "{}"), None);
    }

    #[test]
    fn unparseable_payload_is_dropped() {
        assert_eq!(dispatch("mistral-vibe", "not-json"), None);
    }

    #[test]
    fn a_host_session_rejects_empty_and_overlong_ids() {
        assert_eq!(HostSession::new(""), None);
        assert!(HostSession::new(&"s".repeat(256)).is_some());
        assert_eq!(HostSession::new(&"s".repeat(257)), None);
    }

    #[test]
    fn a_host_session_encodes_lowercase_hex() {
        assert_eq!(HostSession::new("a").unwrap().encoded(), "61");
        // `:` is hex-adjacent; it must land as 3a, not pass through.
        assert_eq!(HostSession::new("a:b").unwrap().encoded(), "613a62");
    }

    #[test]
    fn a_work_key_validates_both_ids() {
        let long = "w".repeat(257);
        assert_eq!(WorkKey::new("", "w"), None);
        assert_eq!(WorkKey::new("s", ""), None);
        assert_eq!(WorkKey::new(&long, "w"), None);
        assert_eq!(WorkKey::new("s", &long), None);
        assert!(WorkKey::new("s", "w").is_some());
    }

    #[test]
    fn a_work_key_encodes_session_nul_work() {
        let key = WorkKey::new("s", "w").unwrap();
        // 73 = "s", 00 = the NUL separator, 77 = "w".
        assert_eq!(key.encoded(), "730077");
        assert_eq!(key.session(), &HostSession::new("s").unwrap());
    }

    #[test]
    fn work_key_tokens_cannot_collide_across_id_boundaries() {
        // `s\0w` and `s` + `\0w` encode differently, so no pair of raw IDs
        // maps to the same token as another pair.
        let a = WorkKey::new("s", "w").unwrap();
        let b = WorkKey::new("s", "\0w").unwrap();
        assert_ne!(a.encoded(), b.encoded());
    }
}
