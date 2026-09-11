//! Map a single JSON payload to a state for agents that deliver one callback
//! per event.
//!
//! The agent vocabulary lives here, not in the caller, so a shell snippet never
//! has to parse JSON or depend on `jq`. An unrecognised payload returns `None`,
//! which the CLI turns into a silent exit 0.
//!
//! Each shape-B agent is a row in a data table. The table maps one JSON string
//! field to a `State`; adding a new agent is a new row, not a new code path.

use crate::state::State;

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

/// Map `payload` for `agent` to a state, or `None` when the event carries no
/// recognised lifecycle state.
pub fn dispatch(agent: &str, payload: &str) -> Option<State> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let mapping = AGENTS.iter().find(|m| m.name == agent)?;
    let event = value.get(mapping.event_field)?.as_str()?;
    mapping
        .table
        .iter()
        .find(|(name, _)| *name == event)
        .map(|(_, state)| *state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mistral_vibe_pre_tool_is_working() {
        let payload = r#"{"hook_event_name":"pre_tool"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), Some(State::Working));
    }

    #[test]
    fn mistral_vibe_post_tool_failure_stays_working() {
        let payload = r#"{"hook_event_name":"post_tool","tool_status":"failure"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), Some(State::Working));
    }

    #[test]
    fn mistral_vibe_post_tool_success_stays_working() {
        let payload = r#"{"hook_event_name":"post_tool","tool_status":"success"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), Some(State::Working));
    }

    #[test]
    fn mistral_vibe_post_agent_is_done() {
        let payload = r#"{"hook_event_name":"post_agent"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), Some(State::Done));
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
                Some(State::Working),
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
}
