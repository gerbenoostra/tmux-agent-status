# sanitize.jq - reduce one probe JSONL record to an adapter-readable fixture.
#
# A probe record is {event, ts_enter, ts_exit, stdin} where stdin is the raw
# hook payload as a string. The fixture keeps the envelope plus a `payload`
# holding only fields an adapter could act on: identity (session, prompt,
# agent, task), the event vocabulary (hook_event_name, source, reason,
# notification_type, tool_name, an error code), state flags
# (stop_hook_active, permission_mode), the ID-bearing subset of tool_input and
# tool_response (a TaskStop's task_id, an Agent launch's agentId), and the
# background_tasks snapshot's ids, types and statuses. Everything
# content-bearing or path-bearing is dropped: prompts, messages, command
# lines, transcripts, working directories, output files, tool input text,
# tool response bodies and tool failure messages.

def keep($keys): with_entries(select(.key as $k | $keys | index($k)));

def san_payload:
    . + {
        hook_event_name: (.hook_event_name // .hookEventName),
        session_id: (.session_id // .sessionId),
        prompt_id: (.prompt_id // .promptId),
        permission_mode: (.permission_mode // .permissionMode),
        stop_hook_active: (.stop_hook_active // .stopHookActive),
        notification_type: (.notification_type // .notificationType),
        agent_id: (.agent_id // .subagentId),
        agent_type: (.agent_type // .subagentType),
        tool_name: (.tool_name // .toolName),
        tool_use_id: (.tool_use_id // .toolUseId // .tool_call_id),
        tool_status,
        background_tasks: (.background_tasks // .backgroundTasks)
    }
    | with_entries(select(.value != null))
    | keep([
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
        "tool_status",
        "trigger",
        "name",
        "error",
        "tool_input",
        "tool_response",
        "background_tasks"
    ])
    # `error` is a code such as `server_error` on StopFailure, but a failed
    # tool's free-text message on PostToolUseFailure; only the code is kept.
    | (if has("error") and ((.error | type) != "string" or (.error | test("^[a-z_]+$") | not))
       then del(.error) else . end)
    | (if .tool_input | type == "object"
       then .tool_input |= keep(["task_id", "run_in_background", "subagent_type", "isolation"])
       else . end)
    | (if .tool_name == "run_subagent"
          and (.tool_response | type) == "object"
          and (.tool_response.output | type) == "string"
       then .tool_response.agentId = (try (.tool_response.output | capture("(?<id>[0-9a-f]{8})").id) catch null)
       else . end)
    | (if .tool_response | type == "object"
       then .tool_response |= (
           keep(["task_id", "task_type", "agentId", "status", "isAsync", "success"])
           | with_entries(select(.value != null)))
       else . end)
    | (if .background_tasks | type == "array"
       then .background_tasks |= map(
           if type == "object"
           then . + {agent_type: (.agent_type // .agentType)}
               | keep(["id", "type", "status", "agent_type"])
           else .
           end)
       else . end);

{event, ts_enter, ts_exit, payload: (.stdin | fromjson | san_payload)}
