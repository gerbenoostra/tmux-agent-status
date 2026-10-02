# sanitize.jq - reduce one probe JSONL record to an adapter-readable fixture.
#
# A probe record is {event, ts_enter, ts_exit, stdin} where stdin is the raw
# hook payload as a string. The fixture keeps the envelope plus a `payload`
# holding only fields an adapter could act on: identity (session, prompt,
# agent, task), the event vocabulary (hook_event_name, source, reason,
# notification_type, tool_name), and state flags (stop_hook_active,
# permission_mode). Everything content-bearing or path-bearing is dropped:
# prompts, messages, command lines, transcripts, working directories,
# output files, tool input text and tool response bodies.

def keep($keys): with_entries(select(.key as $k | $keys | index($k)));

def san_payload:
    keep([
        "hook_event_name",
        "session_id",
        "prompt_id",
        "permission_mode",
        "source",
        "reason",
        "stop_hook_active",
        "notification_type",
        "agent_id",
        "agent_type",
        "tool_name",
        "tool_use_id",
        "trigger",
        "name"
    ])
    | (if ((.error? // "") | tostring | test("/")) then del(.error) else . end)
    | (if .tool_input | type == "object"
       then .tool_input |= keep(["task_id", "run_in_background", "subagent_type", "isolation"])
       else . end)
    | (if .tool_response | type == "object"
       then .tool_response |= keep(["task_id", "task_type", "agentId", "status", "isAsync", "success"])
       else . end)
    | (if .background_tasks | type == "array"
       then .background_tasks |= map(if type == "object" then keep(["id", "type", "status", "agent_type"]) else . end)
       else . end);

{event, ts_enter, ts_exit, payload: (.stdin | fromjson | san_payload)}
