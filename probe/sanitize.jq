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
#
# Hosts that name fields in camelCase (Grok's sessionId, subagentId,
# backgroundTasks) get them renamed to the snake_case names above, so every
# host's fixtures share one vocabulary. Devin reports a spawned subagent's ID
# only inside run_subagent's free-text output; that eight-character hex ID is
# lifted into tool_response.agentId and the text itself is dropped.

def keep($keys): with_entries(select(.key as $k | $keys | index($k)));

# Rename $from to $to unless $to is already there. Decided by key presence,
# not `//`, which would treat a false or null value as missing.
def alias($to; $from):
    if has($to) or (has($from) | not) then . else .[$to] = .[$from] end;

def san_payload:
    alias("hook_event_name"; "hookEventName")
    | alias("session_id"; "sessionId")
    | alias("prompt_id"; "promptId")
    | alias("permission_mode"; "permissionMode")
    | alias("stop_hook_active"; "stopHookActive")
    | alias("notification_type"; "notificationType")
    | alias("agent_id"; "subagentId")
    | alias("agent_type"; "subagentType")
    | alias("tool_name"; "toolName")
    | alias("tool_use_id"; "toolUseId")
    | alias("tool_use_id"; "tool_call_id")
    | alias("background_tasks"; "backgroundTasks")
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
       then ([.tool_response.output | match("\\b[0-9a-f]{8}\\b").string][0]) as $id
           | if $id then .tool_response.agentId = $id else . end
       else . end)
    | (if .tool_response | type == "object"
       then .tool_response |= keep(["task_id", "task_type", "agentId", "status", "isAsync", "success"])
       else . end)
    | (if .background_tasks | type == "array"
       then .background_tasks |= map(
           if type == "object"
           then alias("agent_type"; "agentType") | keep(["id", "type", "status", "agent_type"])
           else .
           end)
       else . end);

{event, ts_enter, ts_exit, payload: (.stdin | fromjson | san_payload)}
