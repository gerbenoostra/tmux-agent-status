//! Map a single JSON payload to a state for agents that deliver one callback
//! per event.
//!
//! The agent vocabulary lives here, not in the caller, so a shell snippet never
//! has to parse JSON or depend on `jq`. An unrecognised payload returns `None`,
//! which the CLI turns into a silent exit 0.

use crate::state::State;

/// Map `payload` for `agent` to a state, or `None` when the event carries no
/// recognised lifecycle state.
pub fn dispatch(agent: &str, payload: &str) -> Option<State> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    match agent {
        "mistral-vibe" => mistral_vibe(&value),
        "gemini" => gemini(&value),
        _ => None,
    }
}

fn mistral_vibe(value: &serde_json::Value) -> Option<State> {
    let event = value.get("hook_event_name")?.as_str()?;
    match event {
        "pre_tool" => Some(State::Working),
        "post_tool" => {
            let status = value.get("tool_status")?.as_str()?;
            if status == "failure" {
                Some(State::Error)
            } else {
                Some(State::Working)
            }
        }
        "post_agent" => Some(State::Done),
        _ => None,
    }
}

fn gemini(_value: &serde_json::Value) -> Option<State> {
    // Gemini CLI has not yet shipped an extension-hooks drop-in mechanism, and
    // the payload schema is unconfirmed. Returning `None` keeps the command
    // safe until the schema is known.
    None
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
    fn mistral_vibe_post_tool_failure_is_error() {
        let payload = r#"{"hook_event_name":"post_tool","tool_status":"failure"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), Some(State::Error));
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
    fn mistral_vibe_post_tool_missing_status_is_dropped() {
        let payload = r#"{"hook_event_name":"post_tool"}"#;
        assert_eq!(dispatch("mistral-vibe", payload), None);
    }

    #[test]
    fn mistral_vibe_post_tool_non_string_status_is_dropped() {
        let payload = r#"{"hook_event_name":"post_tool","tool_status":false}"#;
        assert_eq!(dispatch("mistral-vibe", payload), None);
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
    fn gemini_payload_is_dropped() {
        assert_eq!(dispatch("gemini", "{}"), None);
    }
}
