## Scope Note

"Devin" is ambiguous across Cognition's product line (Devin Cloud, Devin Desktop/Cascade, Devin API automations, and Devin CLI). Per the survey criteria, this profile covers **Devin CLI** ("Devin for Terminal"), Cognition's local terminal coding agent installed via `curl -fsSL https://cli.devin.ai/install.sh | bash`, since it is the only variant that runs as a local process attached to a terminal pane[1][2]. Devin Desktop's "Cascade Hooks" and the cloud API's webhook-driven "Automations" are noted only for disambiguation — neither runs inside a tmux pane, so neither is candidate architecture for this plugin[3][4].

## Capability Matrix

| Agent | Shape | Drop-in file? | Needs enabling? | Subagent events? | Multi-session/pane? | `error` event? | `waiting` repeats? | Stdout parsed? | Payload on stdin? | `TMUX_PANE` inherited? | Session start? | Session end? | Surveyed version/date |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Devin CLI | A | Yes — `.devin/hooks.v1.json`[5] | No (hooks are on by default; no separate flag found)[5] | Partial — subagent invocation visible only as `run_subagent`/`read_subagent` tool calls via `PreToolUse`/`PostToolUse`, no dedicated `SubagentStop` event[6] | Not documented as supported; no multi-session-per-pane concept published[5][7] | No dedicated `error`/`aborted` event; must be inferred from `PostToolUse.tool_response.error`/`success` or absence of a clean `Stop`[6] | No dedicated `waiting` event; closest is `PermissionRequest`, which fires once per pending decision (repetition not documented)[6] | Yes, for `decision`/`hookSpecificOutput` control fields — but only on `PreToolUse`, `PermissionRequest`, `UserPromptSubmit`, `Stop`, `SessionStart`; other events don't need parsed stdout[5] | Yes — JSON on stdin for all hook types[5] | Not documented (see analysis below) | Yes — `SessionStart`[5][6] | Yes — `SessionEnd`[5][6] | Docs snapshot Sept 10 2026; CLI stable v3000.6.14 (Sept 3 2026)[8][9] |

## 1. Shape and Integration Routes

Devin CLI is unambiguously **Shape A**: it reads a JSON hooks configuration mapping named lifecycle events (`PreToolUse`, `PostToolUse`, `PermissionRequest`, `UserPromptSubmit`, `Stop`, `PostCompaction`, `SessionStart`, `SessionEnd`) to shell commands, and runs the matching command for each event. There is only one modern route — the dedicated `.devin/hooks.v1.json` file — but Devin CLI also transparently ingests hooks already defined in Claude Code's format (`hooks` key inside `.claude/settings.json`, `.claude/settings.local.json`, `~/.claude.json`, `~/.claude/settings.json`), so a repo that already has Claude Code hooks gets them picked up automatically without duplication. There is no legacy callback-style route for the CLI; Cognition's separate "Cascade Hooks" (Devin Desktop) and webhook-driven "Automations" (cloud API) are architecturally distinct products that do not apply to a terminal-pane integration and are not richer alternatives here.[5][3][4]

## 2. Hook Config Location

The recommended, richer route is the standalone drop-in file `.devin/hooks.v1.json`, where the hooks object is the entire file content (no wrapper key needed). Devin CLI discovers project-level hook files by walking the working directory and its ancestor directories up to the repository root, the same discovery mechanism used for skills and rules. Alternative, non-drop-in locations require merging a `"hooks"` key into an existing settings file: `.devin/config.json`, `.devin/config.local.json` (project-level, gitignored local override), and user-level `~/.config/devin/config.json` (`%APPDATA%\devin\config.json` on Windows), plus the Claude-Code-format files listed above.[5]

## 3. Enabling Elsewhere

No separate feature flag was found that must be toggled before hooks fire; the docs describe hooks as available out of the box once the `.devin/hooks.v1.json` file (or equivalent settings key) is present, and provide a `/hooks` slash command to verify what is currently loaded. The only related toggle is `read_config_from.claude` in user config, which controls whether Devin CLI also imports hooks from `.claude/` paths (enabled by default) — this affects where hooks can live, not whether the hook system itself is active.[10][5]

## 4. Event Vocabulary and Payloads

Every command hook receives event data as JSON on stdin, and Devin CLI sets the `DEVIN_PROJECT_DIR` environment variable for the child process. Every payload carries a `session_id` (stable per session) and `prompt_id` (rotated per turn, absent before the first user prompt, e.g. on `SessionStart`). The documented events and their stdin fields:[6][5]

- `PreToolUse` — fires before a tool executes. Fields: `tool_name` (e.g. `exec`, `edit`, `mcp__github__create_issue`), `tool_input` (e.g. `{"command": "rm -rf /", "shell_id": "main"}`).
- `PostToolUse` — fires after a tool finishes. Fields: `tool_name`, `tool_input`, `tool_response` (object with `success` boolean, `output` string, `error` string or null).
- `PermissionRequest` — fires when a permission decision is needed. Fields: `tool_name`, `tool_input`.
- `UserPromptSubmit` — fires when the user submits a message. Field: `prompt` (the message text).
- `Stop` — fires when the agent wants to end its turn. Field: `stop_hook_active` (whether a stop hook is already active).
- `PostCompaction` — fires after context compaction. Field: `summary` (may be null).
- `SessionStart` — fires when a session begins. Field: `source` (how the session was started).
- `SessionEnd` — fires when a session ends. Field: `reason` (why the session ended).

Example full stdin envelope for `PreToolUse`:[5]

```json
{
  "hook_event_name": "PreToolUse",
  "tool_name": "exec",
  "tool_input": {
    "command": "rm -rf /"
  },
  "session_id": "3f8d1c2a-...",
  "prompt_id": "b71e9d40-..."
}
```

A hook can return JSON on stdout to influence outcome: a top-level `decision` (`"approve"`/`"block"`) with optional `reason`, or a `hookSpecificOutput` object carrying `additionalContext` (for `UserPromptSubmit`, `SessionStart`, `PostToolUse`) or `updatedInput` (for `PreToolUse`). Exit code 0 means success/continue, 2 means block, any other code is logged as an error but does not block.[5]

## 5. Subagent Events

Devin CLI does not publish a dedicated `SubagentStart`/`SubagentStop` hook event. Subagents are spawned and read via the `run_subagent` and `read_subagent` tool calls, which are visible only indirectly through `PreToolUse`/`PostToolUse` hooks matching those tool names, and subagent prompts/results stream through the live display rather than a separate hook channel. Because the parent session's own `Stop` event fires only when the parent turn ends — not when a subagent finishes — a subagent completing its work should **not** be treated as the overall turn reaching `done`; the glyph should remain `working` until the parent's `Stop`/`SessionEnd` fires, since `run_subagent` in foreground mode pauses the parent while background subagents run concurrently but the parent conversation is still active. Subagents can be disabled entirely by setting `subagents_enabled: false` in the user config, which removes the `run_subagent`/`read_subagent` tools.[7][11][12]

## 6. Multiple Sessions per Pane

No documentation states that multiple Devin CLI sessions can run concurrently inside one terminal pane; the CLI is described and used as a single foreground REPL process per invocation, with `/handoff` used to delegate a task to a separate cloud session rather than spawning a second local session in the same pane. No rollup logic across sessions-per-pane is indicated as necessary for this integration.[13][8]

## 7. Turn-Failed / Error Event

There is no published `Error`, `TurnFailed`, or `Aborted` hook event. Failure signals must be inferred: `PostToolUse.tool_response` includes `success` (boolean) and `error` (string or null) per tool call, and a turn that never reaches a clean `Stop` (e.g. the process exits or a `SessionEnd` with a `reason` indicating a crash/interrupt) would need to be treated as an error state by the integrating plugin. This aligns with the survey's general note that `error` states often must be inferred rather than delivered as a first-class event.[6]

## 8. Blocked-on-User / Waiting Event

The closest documented analogue is `PermissionRequest`, which fires "when the agent needs a permission decision". The docs do not state whether `PermissionRequest` repeats while a decision remains outstanding, or fires only once per request; no polling/retry behavior for unanswered permission prompts is documented. There is no separate "awaiting user input" event distinct from a permission decision.[6][5]

## 9. `TMUX_PANE` Inheritance

Not documented. The docs explicitly confirm only that Devin CLI sets `DEVIN_PROJECT_DIR` for hook child processes and are silent on any environment-variable filtering. Since Devin CLI is a normal terminal-attached process (not sandboxed by default outside enterprise policy) and hook commands are literal shell commands, standard POSIX child-process semantics would inherit the parent shell's environment including `TMUX_PANE` — but this behavior is not stated in official docs and should be verified empirically before relying on it; an explicit `--pane` override in the plugin's CLI is the safer default.[5]

## 10. Payload Delivery: argv or stdin

Stdin. All command hooks receive their JSON event payload on stdin; the docs make no mention of argv-based payload passing for hooks.[6][5]

## 11. Stdout Parsing Requirement

Yes, conditionally. Devin CLI parses hook stdout as JSON only for events where control output is meaningful: `PreToolUse` (block/approve, `updatedInput`), `PermissionRequest` (approve/block), `UserPromptSubmit` and `SessionStart` (`additionalContext`), and `Stop` (`decision`/`reason`). For `PostToolUse`, `PostCompaction`, and `SessionEnd` hooks used purely as side-effect triggers (which is what `tmux-agent-status` would use), the docs' own examples pipe stdout to a log file rather than back to Devin, and non-JSON stdout is not flagged as an error condition in the docs. To be safe across all event types, any command the plugin registers should still print `{}` and nothing else, consistent with the general rule for Shape A/B agents that may parse stdout as JSON.[5]

## 12. Session-Start / Session-End Events

Yes to both: `SessionStart` fires when a new session begins (field: `source`), and `SessionEnd` fires when a session ends (field: `reason`). These map directly to the plugin's `reset` and `finish` commands. Because `SessionEnd` fires on the CLI process's own lifecycle, an ungraceful process kill (e.g. `kill -9`, terminal closed forcibly) could still bypass hook execution entirely and leave a stale glyph, the same crash-of-agent risk flagged generically for this integration pattern.[6]

## 13. Shape C Applicability

Not applicable — Devin CLI has no in-process JS/TS plugin/extension API comparable to Shape C; its "Plugins" feature (`hooks.json` at a plugin root, GA/beta) is itself built on the same Shape A hooks.json mechanism and explicitly documented as "best effort and fail open," not a stable programmatic event-stream API. There is therefore no plugin-API version/stability claim to report for Shape C purposes.[14]

## Copy-Paste `.devin/hooks.v1.json` Snippet

Devin CLI's event set does not include first-class `working`/`waiting`/`done`/`error` names, so the mapping below uses the closest lifecycle hooks: `UserPromptSubmit`/`PreToolUse` for `working`, `PermissionRequest` for `waiting`, `Stop` for `done`, `PostToolUse` (checking `tool_response.error`) as a best-effort `error` signal, `SessionStart` for `reset`, and `SessionEnd` for `finish`:[6][5]

```json
{
  "SessionStart": [
    {
      "matcher": "",
      "hooks": [
        { "type": "command", "command": "tmux-agent-status reset --agent devin" }
      ]
    }
  ],
  "UserPromptSubmit": [
    {
      "matcher": "",
      "hooks": [
        { "type": "command", "command": "tmux-agent-status set working --agent devin" }
      ]
    }
  ],
  "PreToolUse": [
    {
      "matcher": "",
      "hooks": [
        { "type": "command", "command": "tmux-agent-status set working --agent devin" }
      ]
    }
  ],
  "PermissionRequest": [
    {
      "matcher": "",
      "hooks": [
        { "type": "command", "command": "tmux-agent-status set waiting --agent devin" }
      ]
    }
  ],
  "PostToolUse": [
    {
      "matcher": "",
      "hooks": [
        {
          "type": "command",
          "command": "sh -c 'python3 -c \"import sys,json; d=json.load(sys.stdin); sys.exit(1 if d.get(\\\"tool_response\\\",{}).get(\\\"error\\\") else 0)\" < /dev/stdin && exit 0 || tmux-agent-status set error --agent devin; echo {}'"
        }
      ]
    }
  ],
  "Stop": [
    {
      "matcher": "",
      "hooks": [
        { "type": "command", "command": "tmux-agent-status set done --agent devin; echo {}" }
      ]
    }
  ],
  "SessionEnd": [
    {
      "matcher": "",
      "hooks": [
        { "type": "command", "command": "tmux-agent-status finish --agent devin" }
      ]
    }
  ]
}
```

## Verdict

**Implement as first A** — Devin CLI offers a dedicated, well-documented drop-in hooks file (`.devin/hooks.v1.json`) requiring no separate feature flag, with a clean `SessionStart`/`SessionEnd` pairing for `reset`/`finish` and a `Stop` event for `done`. Its main gaps relative to an ideal Shape A agent are the lack of first-class `error`/`waiting`-repeat events (both must be inferred) and the absence of a `TMUX_PANE`-inheritance confirmation, neither of which blocks a workable integration.[5][6]

## Recommendation

Devin CLI is a strong first Shape A candidate on the strength of its drop-in `.devin/hooks.v1.json` file, always-on hook system, and complete `SessionStart`/`SessionEnd` coverage, though the plugin should treat its `error` signal as inferred (from `PostToolUse.tool_response.error` or an unclean session end) rather than as a published event. Devin CLI is not a Shape C candidate since it has no in-process JS/TS event-stream plugin API — its "Plugins" feature is itself Shape A hooks packaged for distribution and is explicitly labeled best-effort/fail-open, so any Shape C pick for the project must come from a different agent in the broader survey.[14][6][5]

## final note
Devin’s terminal client (Devin CLI — the local coding agent at  cli.devin.ai , distinct from Devin Desktop/Cascade and the cloud webhook Automations) is a clean Shape A fit: it has a dedicated drop-in file,  .devin/hooks.v1.json , with no feature flag needed to enable hooks, and clean  SessionStart / SessionEnd  events mapping to  reset / finish .
Key gaps to flag for your matrix:
•	No first-class  error  or  waiting -repeat events —  error  must be inferred from  PostToolUse.tool_response.error  or an unclean session end, and  waiting  is approximated by  PermissionRequest  (repetition undocumented).
•	Subagents are only visible as  run_subagent / read_subagent  tool calls (no dedicated subagent-stop event) — a subagent finishing should not flip the glyph to  done , since the parent’s own  Stop  hasn’t fired yet.
•	 TMUX_PANE  inheritance is undocumented (unknown, not confirmed no).
•	Devin CLI has no Shape C in-process plugin API — its “Plugins” feature is itself Shape A hooks bundled for distribution, and is explicitly labeled best-effort/fail-open, so it’s not a Shape C candidate.
Full details, exact payload schemas, event tables, and a copy-paste  .devin/hooks.v1.json  snippet are in the report above.
