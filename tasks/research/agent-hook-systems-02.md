## Capability Matrix — Dated 2026-09-09

| Agent | Shape | Drop-in file? | Needs enabling? | Subagent events? | Multi-session/pane? | error event? | waiting repeats? | Stdout parsed? | Payload on stdin? | TMUX_PANE inherited? | Session start? | Session end? | Surveyed version/date |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Grok CLI (xAI Grok Build) | A | Yes — `~/.grok/hooks/*.json` or `<project>/.grok/hooks/*.json` (Claude-compatible JSON) [1][2] | Project hooks need `/hooks-trust`; global hooks are always trusted [1] | Only via `spawn_subagent` tool name in `PreToolUse`/`PostToolUse`, no dedicated subagent-stop event documented [3] | Unknown/not documented (single-session focus in docs) | Not published as a distinct event — inferred from `PostToolUse` tool_status or turn `stopReason` in `--output-format json` [4][1] | Unknown/undocumented | Yes, if hook prints valid JSON on stdout; otherwise passthrough — no strict "must be pure JSON" requirement [1] | Yes, JSON on stdin [1] | Unknown/not documented | Yes (`SessionStart`) [1] | Yes (`SessionEnd`, fires on `/exit` and headless quit) [5][1] | Docs dated 2026, README last synced 2026-07-25 [5] |
| Mistral Vibe (Vibe Code CLI) | A | Yes — `./.vibe/hooks.toml` (project) or `~/.vibe/hooks.toml` (user) [6] | Project hooks load only when the working directory is a trusted folder [6] | Subagents inherit parent's hooks — no separate subagent-stop signal; a `post_agent` fires per assistant turn, not per subagent [6] | Unknown/not documented | Not published as a distinct event; must infer from `post_tool` `tool_status = "failure"` or repeated `post_agent` denies | Not applicable — no waiting/blocked event; `ask_user_question` is a tool call visible via `pre_tool`/`post_tool` only | Yes — `strict = true` turns malformed stdout into a denial; default is a warning [6] | Yes, JSON on stdin (`session_id`, `parent_session_id`, `transcript_path`, `cwd`, `hook_event_name` + type-specific fields) [6] | Unknown/not documented | No dedicated `session_start` hook type documented (only `pre_tool`, `post_tool`, `post_agent`) [6] | No dedicated `session_end` hook type documented [6] | docs.mistral.ai, current as of 2026-09 [6] |
| Kiro (CLI + IDE, unified hook system) | C (CLI: A-like drop-in file, but richer state incl. subagent/multi-session visibility lives in-app) | Yes — `.kiro/hooks/*.json`, PascalCase triggers, works on IDE, CLI, and Web [7] | No separate flag — hooks activate automatically once the file exists; project trust may still gate execution | No dedicated subagent-stop trigger documented; multi-subagent approvals are surfaced in-TUI, not as hook events [8] | Yes — Kiro CLI can run "multi-subagent TUI" with several subagents' approval prompts in one pane [8] | Not published; command hooks only report success (exit 0) or failure (any other exit code) generically, not a lifecycle "error" state [9] | `PromptSubmit`/blocking hooks don't repeat on their own; no documented recurring "waiting" event | Not JSON-parsed — exit code determines success/failure; stdout is added to agent context as text, not parsed as structured JSON [9] | Yes, JSON via STDIN (session context) [7] | Unknown/not documented | Yes for IDE (`SessionStart`); CLI uses `AgentSpawn` instead [7] | No `SessionEnd`/`AgentEnd` trigger listed in the trigger table — absence confirmed by omission [7] | kiro.dev docs updated Sept 2, 2026 (hooks) / Aug 21, 2026 (examples) [7][10] |
| Antigravity (Google) | C | Plugin bundle drop-in at `~/.gemini/antigravity-cli/plugins/<name>/hooks.json`, but requires full plugin scaffold (`plugin.json` manifest), not a single-file hook drop-in [11] | Plugin must be installed via `agy plugin install` and enabled (`agy plugin enable`) — disabled by default until explicitly enabled [11] | Not documented in the plugin overview page; plugin bundles can define "background subagents" but no subagent-stop event is described [11] | Unknown/not documented | Not published as an event name in the docs reviewed; a `dcg` integration shows Antigravity's `PreToolUse` hook returning `{"decision":"block", ...}`, implying block/deny semantics rather than a turn-level error event [3] | Unknown/not documented | Unknown — not confirmed in available docs | Unknown/not documented (dcg example shows JSON envelope for `toolCall.name`, format of delivery not specified) [3] | Unknown/not documented | Unknown/not documented — `hooks.json` is described only as "pre/post tool event hooks," no session-start trigger confirmed [11] | Unknown/not documented | antigravity.google/docs, page undated in content but current as of survey date 2026-09-09 [11] |

***

## Grok CLI (xAI Grok Build)

**Shape and routes.** Grok Build supports two overlapping mechanisms: a Claude-compatible drop-in JSON hooks directory (`~/.grok/hooks/*.json` or `<project>/.grok/hooks/*.json`) and an equivalent `[[hooks.<Event>]]` TOML table embedded in `config.toml`/`managed_config.toml`/`requirements.toml`. The richer, more idiomatic route for a plugin author is the drop-in JSON directory — it is the one documented with a "Quick Start," is self-contained, and is what third-party integrations like the `dcg` guard tool target.[1][2][3]

**Config location and drop-in.** Global hooks live at `~/.grok/hooks/*.json`; project-scoped hooks live at `<project>/.grok/hooks/*.json`; a Claude-settings compatibility layer (`~/.claude/settings.json` and `<project>/.claude/settings.json`) is also auto-discovered. Grok also loads plugin-bundled hooks. This is a genuine drop-in shape — the plugin can ship a single JSON file the user copies into `~/.grok/hooks/`.[2]

**Enabling.** Global hooks under the user's home directory are "always trusted." Project-level hooks require the user to run `/hooks-trust` (or use the Hooks modal) the first time a project with hooks is opened — this is a folder-trust gate, not a separate feature flag file.[2]

**Event vocabulary and payload.** Documented events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `Notification`, `SessionEnd`. The wire payload uses a camelCase envelope with `hookEventName` carrying a snake_case value (e.g., `"pre_tool_use"`), plus tool fields `toolName`/`toolInput`/`toolUseId`/`toolInputTruncated`, and `PostToolUse` additionally carries `toolResult`. A hook's JSON response for blocking events is `{"decision":"allow"}` or `{"decision":"deny","reason":"..."}`.[3][2]

**Subagent events.** There is no dedicated "subagent stop" event; subagent invocation is only visible as a `spawn_subagent` tool name inside `PreToolUse`/`PostToolUse`. This means a subagent completing is not distinguishable from any other tool call finishing — the parent session's own `Stop`/`SessionEnd` is what should map to `done`/`finish`.[3]

**Multi-session per pane.** Not documented; Grok's docs describe headless named sessions (`-s/--session-id`) for scripting multiple parallel invocations, but nothing about multiple interactive TUI sessions sharing one terminal pane.[5]

**Error/waiting events.** No distinct `error` event is published; failure must be inferred from `PostToolUse`'s `tool_status`/`toolResult` or the headless JSON output's `stopReason` field. No `waiting`/blocked-on-user event is documented either — `UserPromptSubmit` only fires once the user has already submitted a prompt, not while blocked.[4][1]

**Stdout parsing and exit codes.** Hooks respond via exit code and stdout: exit `0` = allow, exit `2` = deny, any other code fails open (passthrough). This means a `tmux-agent-status` command run as a Grok hook must exit `0` and can print an empty JSON object safely, since non-JSON output is treated as informational for passive hook types.[2]

**Payload delivery.** Confirmed as JSON on stdin.[1][2]

**Session start/end.** Both exist: `SessionStart` and `SessionEnd`, with `SessionEnd` explicitly confirmed to fire "on `/exit` and headless quit" per the changelog. This is a meaningful advantage over agents lacking `SessionEnd` — it substantially reduces stale-glyph risk.[5]

**Copy-paste-ready snippet (`~/.grok/hooks/tmux-agent-status.json`):**

```json
{
  "hooks": {
    "SessionStart": [
      { "hooks": [{ "type": "command", "command": "tmux-agent-status reset" }] }
    ],
    "PreToolUse": [
      { "hooks": [{ "type": "command", "command": "tmux-agent-status set working" }] }
    ],
    "PostToolUse": [
      { "hooks": [{ "type": "command", "command": "tmux-agent-status set working" }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "tmux-agent-status finish" }] }
    ],
    "SessionEnd": [
      { "hooks": [{ "type": "command", "command": "tmux-agent-status finish" }] }
    ]
  }
}
```

Note: since Grok has no native `error` or `waiting` events, `error` must be approximated inside a `PostToolUse` script that inspects `toolResult`/`tool_status` and calls `tmux-agent-status set error` conditionally, and `waiting` has no first-class trigger at all.

**Verdict: Implement as first A.**

***

## Mistral Vibe (Vibe Code CLI)

**Shape and routes.** Single route: `hooks.toml` with three hook types — `pre_tool`, `post_tool`, `post_agent` — no legacy callback exists.[6]

**Config location and drop-in.** `./.vibe/hooks.toml` (project, trusted-folder gated, loaded first) then `~/.vibe/hooks.toml` (user, loaded second); matching `name` entries let the project override the user file. This is a genuine drop-in TOML file.[6]

**Enabling.** Project-level `hooks.toml` only loads when the working directory is in the trusted-folders list (`~/.vibe/trusted_folders.toml`); no separate feature flag.[6]

**Event vocabulary and payload.** Every hook receives a session-context object on stdin: `session_id`, `parent_session_id`, `transcript_path`, `cwd`, `hook_event_name`. `pre_tool` adds `tool_name`, `tool_call_id`, `tool_input`. `post_tool` adds `tool_name`, `tool_call_id`, `tool_input` (post-rewrite), `tool_status` (`success`/`failure`/`cancelled`), `tool_output`, `tool_output_text`, `tool_error`, `duration_ms`. `post_agent` receives no extra fields beyond the session context and fires "after every assistant turn that ends without pending tool calls".[6]

**Subagent events.** Subagents inherit the parent session's hook configuration transitively, but there is no distinct subagent-start/stop signal — a subagent's activity only surfaces as ordinary `pre_tool`/`post_tool` events with `parent_session_id` populated, and `post_agent` fires per assistant turn rather than per subagent completion. This means a subagent finishing its own turn should not be treated as `done` for the tmux glyph — the parent session's own `post_agent`/turn-end is the correct signal, since the docs describe `post_agent` firing whenever *any* assistant turn (parent or nested) ends without pending tool calls, so the adapter needs `parent_session_id` filtering to avoid false "done" flips mid-turn.[6]

**Multi-session, error, waiting.** None of these are documented on the hooks page or the CLI overview; Vibe's docs emphasize `ask_user_question` as a tool (visible only via `pre_tool`/`post_tool`, not a standalone blocking event). No session-start/session-end hook type exists — only `pre_tool`, `post_tool`, `post_agent` are listed as valid `type` values, so `reset`/`finish` semantics must be approximated from the first `pre_tool` of a session and the process exiting, respectively.[12][6]

**Stdout parsing.** Yes — a hook's exit `0` with empty stdout is passthrough; exit `0` with a valid JSON object triggers structured handling; exit `0` with non-empty non-conforming stdout is a "hook failure" (warning, or denial/clear under `strict = true`); non-zero exit, timeout, or spawn failure is also a hook failure. A `tmux-agent-status` command invoked as a hook should print nothing (empty stdout, exit 0) or a bare `{}` to stay in the "passthrough" or "structured response" lane safely.[6]

**Copy-paste-ready snippet (`./.vibe/hooks.toml`):**

```toml
[[hooks]]
name = "tmux-agent-status-working"
type = "pre_tool"
match = "*"
command = "tmux-agent-status set working"

[[hooks]]
name = "tmux-agent-status-done"
type = "post_agent"
command = "tmux-agent-status finish"
```

There is no `reset`/session-start hook type, so `tmux-agent-status reset` cannot be wired via `hooks.toml` alone — it would need to run from a wrapper shell alias around the `vibe` command itself.

**Verdict: Shape B candidate** (works, but lacks native session-start/end and error/waiting vocabulary that Shape A ideally needs — it satisfies the "one command per event" structure but the event set is too thin to reliably drive all four states without external inference).

***

## Kiro (AWS, IDE + CLI + Web)

**Shape and routes.** Kiro unified its hook system across IDE, CLI, and Web onto a single `.kiro/hooks/*.json` schema (`"version": "v1"`) as of **IDE 1.0 / CLI 3.0**; the CLI previously embedded hooks in agent config (CLI 2.x) and has a migration command (`kiro-cli agent migrate`). The current format is the richer, unified one and is what should be targeted.[7]

**Config location and drop-in.** `.kiro/hooks/<id>.json` per project, each file self-contained with `version`, `hooks[]` array (name, trigger, matcher, action, timeout, enabled). This is a genuine drop-in file per hook, applicable on CLI too.[7]

**Enabling.** Hooks "activate automatically when a session starts — no manual registration needed". No separate feature flag is documented.[7]

**Event vocabulary.** CLI-relevant triggers: `Prompt Submit`, `Agent Stop`, `Agent Spawn`, `Pre Tool Use`, `Post Tool Use`. IDE-only triggers (`Session Start`, `File Create`, `File Save`, `File Delete`, `Pre/Post Task Execution`) are not available in the CLI table. Payload: command actions receive "session context as JSON on STDIN"; the exact field names beyond this general description were not published on the pages reviewed.[7]

**Subagent events.** No distinct subagent-stop trigger is listed in the trigger table; however, third-party tooling documents that Kiro CLI supports a "multi-subagent TUI" where several subagent approval prompts appear concurrently in one pane, confirming subagent execution is visible at the UI level even though no hook event exists specifically for it.[8][7]

**Multi-session per pane.** Confirmed — Kiro's multi-subagent TUI runs several subagents' approval prompts simultaneously in a single pane, requiring a monitoring tool to detect and cycle through multiple concurrent prompts. This is direct evidence that a Shape C-style adapter (or in this case, richer polling) would need to roll up multiple subagent states, since Kiro is architecturally Shape A on paper but behaves like it needs Shape C depth for full subagent visibility.[8]

**Error/waiting.** No lifecycle `error` event is published: a shell-command hook signals failure only through its own exit code (non-zero triggers stderr being sent to the agent, and for `Pre Tool Use` specifically blocks the tool call)  — this is a hook-execution failure signal, not an agent-turn-failure event. No recurring "waiting" event is documented; `Prompt Submit` fires once per submission, not while idle waiting for input.[9]

**Stdout parsing.** Not JSON-parsed. Exit code 0 sends stdout as additive context text to the agent; any other exit code sends stderr and (for `Pre Tool Use`) blocks the tool. This means a `tmux-agent-status` hook command does not need to emit JSON at all for Kiro — plain output and correct exit codes suffice, which simplifies the plugin's implementation contract compared to JSON-parsing agents.[9]

**Session start/end.** `Agent Spawn` is the CLI's session-start equivalent; there is no `Agent End`/`Session End` trigger in the documented trigger table — its absence is confirmed by omission from the "Available triggers" table, meaning a crashed Kiro CLI session may leave a stale glyph exactly as the user anticipated.[7]

**Copy-paste-ready snippet (`.kiro/hooks/tmux-agent-status.json`):**

```json
{
  "version": "v1",
  "hooks": [
    { "name": "reset", "trigger": "AgentSpawn", "action": { "type": "command", "command": "tmux-agent-status reset" } },
    { "name": "working-pre", "trigger": "PreToolUse", "action": { "type": "command", "command": "tmux-agent-status set working" } },
    { "name": "working-post", "trigger": "PostToolUse", "action": { "type": "command", "command": "tmux-agent-status set working" } },
    { "name": "done", "trigger": "AgentStop", "action": { "type": "command", "command": "tmux-agent-status finish" } }
  ]
}
```

**Verdict: Implement as first A** (a strong candidate: genuine drop-in JSON, no JSON-stdout parsing burden, CLI-native `AgentSpawn`/`AgentStop` covering `reset`/`finish` cleanly) — though note the missing `SessionEnd` and `error` event as known gaps.

***

## Antigravity (Google)

**Shape.** Antigravity's CLI (`agy`) uses a plugin architecture: plugins are namespaced bundles containing `plugin.json` (manifest), optional `mcp_config.json`, optional `hooks.json` ("pre/post tool event hooks"), a `skills/` directory, `agents/` subagent templates, and `rules/`. This is Shape C in spirit — a structured, code-adjacent extension bundle rather than a single drop-in hook file — even though the `hooks.json` component inside the bundle looks superficially like a Shape A config.[11]

**Config location and drop-in.** Plugins are staged at `~/.gemini/antigravity-cli/plugins/<plugin_name>/` after running `agy plugin install /path/to/local/plugin`. There is no single drop-in file the user copies — the whole plugin directory structure (manifest + hooks file + optional components) must be authored and installed as a package, which is a materially higher bar than Grok's or Kiro's single-JSON drop-in.[11]

**Enabling.** Plugins must be explicitly installed and are individually toggleable: `agy plugin enable <plugin_name>` / `agy plugin disable <plugin_name>`. This is a genuine "enable elsewhere" step distinct from simply placing a file.[11]

**Event vocabulary and payload.** The Antigravity plugin overview describes `hooks.json` only as "Optional pre/post tool event hooks" without enumerating exact event names or payload schema on the page retrieved. Independent third-party evidence (a shell-command guard tool) shows an Antigravity `PreToolUse` hook receiving a payload with `toolCall.name` (e.g., `"run_command"`) and responding with `{"decision":"block","reason":"..."}` on stdout with exit 0  — this confirms a `PreToolUse` event exists and uses a `decision`/`reason` JSON contract, but a full published event list (session start/end, subagent, error) was not found in the primary documentation surveyed.[3][11]

**Subagent, multi-session, error, waiting, TMUX_PANE.** None of these are documented on the plugin overview page reviewed, and no additional Antigravity hooks-specific reference page was located distinct from the plugins overview. Given Antigravity is Google's newest agentic CLI product (Antigravity), and the only concrete schema evidence comes from a third-party security tool rather than official docs, the event vocabulary should be treated as unconfirmed pending a dedicated hooks reference page.[11]

**Minimal JS/TS example:** Not available — the plugin bundle format documented is JSON-configuration-based (`hooks.json`), not a JS/TS module subscribing to an event stream at the API level; no code-level plugin API (imports, event emitter, TypeScript types) was found in the pages retrieved. This is a meaningful documentation gap for a would-be Shape C implementer: without a published in-process plugin API (functions to import, an event bus, or a subscription interface), building a JS/TS subscriber is not currently feasible from public docs.

**Verdict: Skip — insufficient publicly documented lifecycle events and no confirmed in-process plugin API**, pending discovery of a dedicated Antigravity hooks/plugin API reference beyond the overview page at.[11]

***

## Recommendation

For the **first Shape A agent**, pick **Kiro**: its `.kiro/hooks/*.json` format is unified across IDE/CLI/Web, activates with zero extra enabling step beyond the file's presence, avoids the JSON-stdout-parsing complexity that Grok and Mistral Vibe impose, and its CLI-native `AgentSpawn`/`AgentStop` triggers map directly onto `reset`/`finish`. Grok CLI is a strong second choice — it additionally publishes `SessionEnd` (which Kiro lacks), making it worth a near-term second Shape A integration once Kiro is done.[5][7]

For the **first Shape C agent**, none of the five surveyed agents currently has a documented, stable, versioned in-process JS/TS plugin API with a published minimum supported version — Antigravity's plugin bundle is the closest structural fit but its `hooks.json` event vocabulary and any JS/TS subscription surface remain undocumented in the pages retrieved, and no other surveyed agent (Grok CLI, Mistral Vibe, Kiro) exposes an in-process extension model at all — they are all Shape A. Before committing engineering time to a Shape C adapter, it would be worth directly inspecting Antigravity's `hooks.json` schema and any TypeScript SDK by installing the CLI and running `agy plugin list`/inspecting a real plugin bundle, since the authoritative overview page describes the bundle's file layout but not its runtime API contract.[11]

Sources
[1] superagent-ai/grok-cli: An open-source coding agent for the Grok API https://github.com/superagent-ai/grok-cli
[2] grokstream/grok-cli - GitHub https://github.com/grokstream/grok-cli
[3] grok-cli Review: The Community Grok Coding Agent (2026 ... https://andrew.ooo/posts/grok-cli-superagent-review-open-source-coding-agent/
[4] CLI Reference | SpaceXAI Docs https://docs.x.ai/build/cli/reference
[5] Coding work often needs both an interactive terminal agent ... https://x.com/DanKornas/status/2084373972460691463
[6] Reference | superagent-ai/grok-cli | DeepWiki https://deepwiki.com/superagent-ai/grok-cli/10-reference
[7] Deployment Overview | Grok One-Shot - X CLI https://www.xcli.org/docs/deployment/overview
[8] Hooks Reference | Grok One-Shot - X CLI https://www.xcli.org/docs/getting-started/hooks
[9] Grok Build Documentation · Grok Docs - Grok-Wiki https://grok-wiki.com/public/docs/xai-org-grok-build-90205de50458
[10] 22-permissions-and-safety.md https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/22-permissions-and-safety.md
[11] Releases · superagent-ai/grok-cli https://github.com/superagent-ai/grok-cli/releases
[12] SpaceXAI Open-Sources Grok Build: The Rust Agent ... https://www.marktechpost.com/2026/07/15/spacexai-open-sources-grok-build-the-rust-agent-harness-tui-and-tool-layer-behind-its-coding-cli/
[13] GitHub - stevederico/grok-cli: interactive cli for grok https://github.com/stevederico/grok-cli
[14] whitesmith/grok-cli - GitHub https://github.com/whitesmith/grok-cli
[15] Hooks | SpaceXAI Docs https://docs.x.ai/build/features/hooks
[16] amp-examples-and-guides/guides/cli/README.md at main - GitHub https://github.com/sourcegraph/amp-examples-and-guides/blob/main/guides/cli/README.md
[17] Hooks - Features - Docs https://kiro.dev/docs/hooks/
[18] Management - Hooks - Features - Docs - Kiro https://kiro.dev/docs/hooks/management/
[19] Hook triggers - Hooks - Features - Docs https://kiro.dev/docs/hooks/types/
[20] Configuration reference - Custom agents - Features - Docs - Kiro https://kiro.dev/docs/custom-agents/configuration-reference/
[21] Hook actions - Hooks - Features - Docs https://kiro.dev/docs/hooks/actions/
[22] GitHub - sourcegraph/amp-examples-and-guides https://github.com/sourcegraph/amp-examples-and-guides
[23] 0.x reference - IDE 1.x - Docs - Kiro https://kiro.dev/docs/ide/0x-reference/
[24] Managing game assets with agent hooks - Learn by playing - Kiro https://kiro.dev/docs/guides/learn-by-playing/06-managing-assets-with-agent-hooks/
[25] Kiro Agent Hooks Guide - Automate Your Development Workflow ... https://kiro.directory/tips/hooks
[26] Kiro Hooks Complete Documentation Guide - DEV Community https://dev.to/czmilo/kiro-hooks-complete-documentation-guide-3pm0
[27] Automate your development workflow with Kiro's AI agent ... https://kiro.dev/blog/automate-your-development-workflow-with-agent-hooks/
[28] Examples - Hooks - Features - Docs - Kiro https://kiro.dev/docs/hooks/examples/
[29] Docs - Kiro https://kiro.dev/docs/
[30] Kiro Documentation - AWS - Amazon.com https://aws.amazon.com/documentation-overview/kiro/
[31] Hooks - Amp Code https://ampcode.com/news/hooks
[32] Kiro CLI shim https://kiro-learn.mintlify.app/architecture/kiro-cli-shim
[33] Explore Raycast Deep Search API Integration - Amp https://ampcode.com/threads/T-cb1b23b1-ba18-4684-b247-3dce15710470
[34] Hooks - CLI - Docs - Kiro https://kiro.dev/docs/cli/v3/hooks/
[35] Hook | Kiro CLI https://whchoi98.gitbook.io/kirocli/kiro-cli/hook
[36] Hooks - CLI - Docs - Kiro https://kiro.dev/docs/cli/hooks/
[37] 2.x reference - CLI - Docs - Kiro https://kiro.dev/docs/cli/2x-reference/
[38] kirocli package - github.com/semanticash/cli/internal/hooks ... https://pkg.go.dev/github.com/semanticash/cli/internal/hooks/kirocli
[39] Hooks - What's new in 1.0 - IDE 1.x - Docs - Kiro https://kiro.dev/docs/ide/whats-new-v1/hooks/
[40] npm Package Changes - Amp Code https://ampcode.com/news/npm-package-changes
[41] Event types - Introduction - kiro-learn https://kiro-learn.mintlify.app/concepts/event-types
[42] Amp: Coding agent and dev environment built for the frontier https://ampcode.com/
[43] Implement MCP keybindings and groups - Amp https://ampcode.com/threads/T-23915ebe-b80a-4769-9d42-8e4e70ee9619
[44] Owner's Manual - Amp Code https://ampcode.com/manual?preview
[45] Workspace Settings - Amp Code https://ampcode.com/news/cli-workspace-settings
[46] Configuration | Amp Docs https://ampcode.com/docs/cli/settings
[47] SDK Overview | Amp Docs https://ampcode.com/docs/sdk
[48] Enterprise Managed Settings - Amp Code https://ampcode.com/news/enterprise-managed-settings
[49] Getting Started With the CLI | Amp Docs https://ampcode.com/docs/cli
[50] Amp - Zenable Governance Platform https://docs.zenable.io/integrations/mcp/ide/amp
[51] ben-vargas/ai-amp-cli: AmpCode's ... https://github.com/ben-vargas/ai-amp-cli
[52] MCP | Amp Docs https://ampcode.com/docs/customize/mcp
[53] amp-cli — Commands, Examples & Usage Guide https://skywork.ai/clihub/keywords/amp-cli.html
[54] Sync dotfiles with chezmoi - Amp Code https://ampcode.com/threads/T-019da356-2680-72cd-85c0-712eccdfad6c
[55] Amp CLI best practices and tips https://ampcode.com/threads/T-019cafea-01eb-70a9-8aa7-8988b56ab6e7
[56] GitHub - PeonPing/peon-ping https://github.com/PeonPing/peon-ping
[57] peon-ping/CHANGELOG.md at main - GitHub https://github.com/PeonPing/peon-ping/blob/main/CHANGELOG.md
[58] Event-Driven Orbs | Amp Docs https://ampcode.com/docs/orbs/event-driven
[59] Productize website architecture as Astro integration https://ampcode.com/threads/T-019bd4da-7e04-76b9-9eef-6c9d6e9482c4
[60] Introduction | Amp Docs https://ampcode.com/docs
[61] Hooks – AMP for WordPress https://amp-wp.org/reference/hooks/
[62] Bring Your Own Tools - Amp Code https://ampcode.com/news/toolboxes
[63] Python | Amp Docs https://ampcode.com/docs/sdk/python
[64] Deep Dive into the new Cursor Hooks | Butler's Log - GitButler https://blog.gitbutler.com/cursor-hooks-deep-dive
[65] Inside the Cursor afterFileEdit hook: what fires on save - TailTest https://www.tailtest.com/blog/inside-cursor-after-file-edit-hook/
[66] Cursor hooks.json: the JSON Schema & Payload Reference https://ntorres.dev/blog/cursor-hooks-json-guide
[67] Plugins | Amp Docs https://ampcode.com/docs/customize/plugins
[68] Customizing Orbs | Amp Docs https://ampcode.com/docs/orbs/customizing
[69] Plugin API | Amp Docs https://ampcode.com/docs/plugin-api
[70] Hooks https://clients.dev/hooks
[71] mistralai/mistral-vibe: Minimal CLI coding agent by Mistral - GitHub https://github.com/mistralai/mistral-vibe
[72] Hooks | Mistral Docs https://docs.mistral.ai/vibe/code/cli/hooks
[73] AGENTS.md - mistralai/mistral-vibe - GitHub https://github.com/mistralai/mistral-vibe/blob/main/AGENTS.md
[74] Configuration | Mistral Docs https://docs.mistral.ai/vibe/code/cli/configuration
[75] Vibe gets to work. | Mistral AI https://mistral.ai/news/vibe-agent/
[76] Mistral Vibe vs. Claude Code... https://nevercodealone.de/de/vibe-coding/vibe-coding-modelle/mistral-vibe-terminal-ki-coding-agent-2026
[77] Work with the CLI | Mistral Docs https://docs.mistral.ai/vibe/code/cli/work-with-cli
[78] Hooks | Google Antigravity Docs https://antigravity.google/docs/ide/hooks/
[79] GitHub repositories and permissions - Mistral AI Documentation https://docs.mistral.ai/vibe/code/vibe-code-web/github-repositories-permissions
[80] Get started with Vibe Code Web - Mistral AI Documentation https://docs.mistral.ai/vibe/code/vibe-code-web/get-started
[81] Mistral Vibe | AI coding agent for terminal, IDE by Mistral. https://mistral.ai/products/vibe/code/
[82] antigravity-sdk-python/google/antigravity/hooks/README.md at main https://github.com/google-antigravity/antigravity-sdk-python/blob/main/google/antigravity/hooks/README.md
[83] Mistral Vibe (formerly Le Chat) - AI chat and coding agent https://mistral.ai/products/vibe/
[84] Mistral AI · GitHub https://github.com/mistralai
[85] Hooks | Google Antigravity Docs https://antigravity.google/docs/hooks/
[86] Creating Kiro Packages https://www.skillsdirectory.com/skills/pr-pm-creating-kiro-packages
[87] Grok Build: SpaceXAI's Coding Agent - Grok API Documentation https://docs.x.ai/build/overview
[88] Hooks en Kiro: cómo automatizar tareas repetitivas con ... https://dev.to/antitopy/hooks-en-kiro-como-automatizar-tareas-repetitivas-con-triggers-inteligentes-4cbm
[89] Modes and Commands | SpaceXAI Docs - Grok API Documentation https://docs.x.ai/build/modes-and-commands
[90] Grok Build Hooks Reference — PreToolUse in Every Mode, Fail-Open by Design https://agenticcontrolplane.com/blog/grok-build-hooks-reference
[91] Grok Build Tool-Call Control & Audit — Install Guide https://agenticcontrolplane.com/integrations/grok-build
[92] Skills, Plugins & Marketplaces | SpaceXAI Docs https://docs.x.ai/build/features/skills-plugins-marketplaces
[93] Grok Build runs your Claude Code hooks https://agenticcontrolplane.com/blog/grok-build-acp-integration
[94] Grok Build (grok) Cheat Sheet 2026 — 250 Commands | Toolsbase https://toolsbase.dev/en/reference/grok-build-commands
[95] xai-org/grok-build: SpaceXAI's coding agent harness and ... - GitHub https://github.com/xai-org/grok-build
[96] Rename the current session (alias `/title`) | | `grok ... https://docs.x.ai/llms.txt
[97] Skills and Plugins | xai-org/grok-build | DeepWiki https://deepwiki.com/xai-org/grok-build/4.2-skills-and-plugins
[98] Grok Build Permissions & Control Model, Explained https://agenticcontrolplane.com/controls/grok-build
[99] Hooks - Agent Capabilities - Crew - Docs - Kiro https://kiro.dev/docs/crew/capabilities/hooks/
[100] Hooks migration - What's new in 3.0 - CLI - Docs - Kiro https://kiro.dev/docs/cli/v3/hooks-migration/
[101] 13shivam/park: PARK: parallel agent runtime for kiro-cli, a ... https://github.com/13shivam/park
[102] Session Management & Resuming · Kiro CLI Guide https://kiro.kuronetwork.me/en/06-sessions/
[103] Session management - Chat - CLI - Docs - Kiro https://kiro.dev/docs/cli/chat/session-management/
[104] Terminal UI - CLI - Docs https://kiro.dev/docs/cli/terminal-ui/
[105] Pass hook event JSON (tool_name, tool_input) to IDE runCommand ... https://github.com/kirodotdev/Kiro/issues/7500
[106] CLI changelog - Page 3 - Kiro https://kiro.dev/changelog/cli/page/3/
[107] Grok Build changelog & version history | Tech Dev Notes https://techdevnotes.com/releases/grok-build
[108] Grok Build Changelog — Every Release, Explained Simply https://ppcbasic.com/changelog/grok-build/
[109] Amp Changelog: Every Update Explained - PPCBasic.com https://ppcbasic.com/changelog/amp/
[110] Antigravity Updates by Google - September 2026 https://releasebot.io/updates/google/antigravity
[111] Antigravity Changelog (September 2026) - Gradually AI https://www.gradually.ai/en/changelogs/antigravity/
[112] Grok Build v1.0.13 (Aug 28, 2026) — Every Release, Summarized | Havoptic https://www.havoptic.com/tools/grok-build
[113] Ampcode Release Notes - September 2026 Latest Updates https://releasebot.io/updates/ampcode
[114] Grok Build Changelog (@GrokBuildlogs) / Posts / X https://x.com/GrokBuildlogs
[115] GitHub - lbjlaq/Antigravity-Manager: Professional Antigravity ... https://github.com/lbjlaq/Antigravity-Manager
[116] Changelog https://antigravity.google/changelog
[117] Grok Build Updates by xAI - September 2026 https://releasebot.io/updates/xai/grok-build
[118] Google Antigravity - DevAgentRadar https://www.devagentradar.com/assistants/google-antigravity
[119] Antigravity CLI Changelog & Release Notes - Havoptic https://www.havoptic.com/tools/antigravity-cli
[120] xAI Release Notes - September 2026 Latest Updates https://releasebot.io/updates/xai
[121] Google Antigravity SDK https://antigravity.google/blog/introducing-google-antigravity-sdk
[122] Gemini API Managed Agents: 3.6 Flash, hooks, and more https://blog.google/innovation-and-ai/technology/developers-tools/expanding-managed-agents-gemini-api-3-6-flash-hooks/
[123] Antigravity CLI: A Hands-On Guide to Google's Terminal Coding Agent https://dev.to/arindam_1729/antigravity-cli-a-hands-on-guide-to-googles-terminal-coding-agent-5bc7
[124] Kanezal/antigravity-sdk - GitHub https://github.com/Kanezal/antigravity-sdk
[125] I built the first community SDK for Google Antigravity IDE — you can now build extensions that control the AI agent programmatically https://www.reddit.com/r/google_antigravity/comments/1rh2yhg/i_built_the_first_community_sdk_for_google/
[126] AvenCores/open-antigravity-patcher: 🔑 Патчер ... https://github.com/AvenCores/open-antigravity-patcher
[127] Google Antigravity - Antigravity SDK https://antigravity.google/product/antigravity-sdk
[128] Build with Google | Google Antigravity Docs https://antigravity.google/docs/build-with-google
[129] google-antigravity/antigravity-sdk-python: A Python library ... https://github.com/google-antigravity/antigravity-sdk-python
[130] antigravity library - Dart API - Pub.dev https://pub.dev/documentation/antigravity/latest/antigravity/
[131] Overview + Quick Start | Google Antigravity Docs https://antigravity.google/docs/sdk/overview
[132] IDE Extensions | Google Antigravity Docs https://antigravity.google/docs/ide/extensions/
[133] Home | Google Antigravity Docs https://antigravity.google/docs/home/
[134] 05-configuration.md - GitHub https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/05-configuration.md
[135] impeccable/skill/reference/hooks.md at main https://github.com/pbakaus/impeccable/blob/main/skill/reference/hooks.md
[136] Grok Build sessions spam notification sounds: non-idle ... https://github.com/manaflow-ai/cmux/issues/7611
[137] grok-build/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks ... https://ithub.global.ssl.fastly.net/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks.md
[138] When acceptEdits quietly becomes code execution: a Grok ... https://danielalfasi.com/blog/grok-build-acceptedits-hook-persistence/
[139] Grok One-Shot https://www.grok-one-shot.org/docs/build-with-claude-code/hooks-guide
[140] Integrate Unleash with Kiro https://docs.getunleash.io/integrate/kiro
[141] Grok Build - Asymptote - Agent Beacon https://docs.asymptotelabs.ai/runtimes/grok-build
[142] CLI Commands Reference | mistralai/mistral-vibe | DeepWiki https://deepwiki.com/mistralai/mistral-vibe/9.3-cli-commands-reference
[143] Hooks · mistralai mistral-vibe · Discussion #334 · GitHub https://github.com/mistralai/mistral-vibe/discussions/334
[144] Documentation - Mistral AI https://docs.mistral.ai/
[145] Amp, Rebuilt - Amp Code https://ampcode.com/news/neo
[146] CLI Interface (vibe) | mistralai/mistral-vibe | DeepWiki https://deepwiki.com/mistralai/mistral-vibe/3.1-cli-interface-(vibe)
[147] Install the Vibe CLI and send your first prompt | Mistral Docs https://docs.mistral.ai/getting-started/quickstarts/vibe-code/install-cli
[148] Event Driven Orbs - Amp Code https://ampcode.com/news/event-driven-orbs
[149] Amp Orbs https://ampcode.com/manual/orbs?ref=runtimewire
[150] Amp | APIs.io APIs https://apis.io/apis/sourcegraph/amp/
[151] amphtml/docs/spec/amp-actions-and-events.md at main · ampproject/amphtml https://github.com/ampproject/amphtml/blob/main/docs/spec/amp-actions-and-events.md
[152] Schreiben von Plug-Ins für Azure Media Player https://learn.microsoft.com/de-de/azure/media-services/azure-media-player/azure-media-player-writing-plugins
[153] Azure Media Player https://amp.azure.net/libs/amp/latest/docs/index.html
[154] Execute Mode | Amp Docs https://ampcode.com/docs/cli/execute-mode
[155] Librarian https://ampcode.com/docs/tools
[156] Amp Rebuilds CLI to Support Agentic Workflows | Let's Data Science https://letsdatascience.com/news/amp-rebuilds-cli-to-support-agentic-workflows-e47417f5
[157] _plugin_registercommand — x64dbg documentation https://help.x64dbg.com/en/latest/developers/plugins/API/registercommand.html
[158] Plugin API https://umijs.org/en-US/docs/api/plugin-api/
[159] More Tools for the Agent https://ampcode.com/news/more-tools-for-the-agent
[160] Class PluginManager https://docs.phoenix616.dev/bungee-api/net/md_5/bungee/api/plugin/PluginManager.html
[161] RegisterCommand - FiveM Natives @ Cfx Documentation https://docs.fivem.net/natives/?_0x5FA79B0F
[162] Amp https://cisco-ai-defense.github.io/defenseclaw/docs/connectors/amp/
[163] Event System | figma/plugin-typings | DeepWiki https://deepwiki.com/figma/plugin-typings/2.2-event-system
[164] Core PluginAPI Reference | figma/plugin-typings | DeepWiki https://deepwiki.com/figma/plugin-typings/2-core-pluginapi-reference
[165] @super-productivity/plugin-api https://www.npmjs.com/package/@super-productivity/plugin-api
[166] pi/packages/coding-agent/examples/sdk/06-extensions.ts ... https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/sdk/06-extensions.ts
[167] Plugins - Hermes Agent - NOUS RESEARCH https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins
[168] Plugin API Type Definitions | figma/plugin-typings | DeepWiki https://deepwiki.com/figma/plugin-typings/2-plugin-api-type-definitions
[169] pi/packages/coding-agent/docs/extensions.md at main https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md
[170] pc-style/suborbs: amp plugin that lets the main agent fire- ... https://github.com/pc-style/suborbs
[171] Custom Agents - Amp Code https://ampcode.com/news/custom-agents
[172] subagent-concurrency.md https://canmyagentuse.com/features/subagent-concurrency.md
[173] ralph-wiggum - Claude Code Plugin | ClaudePluginHub https://www.claudepluginhub.com/plugins/hmemcpy-ralph-wiggum
[174] Amp TypeScript SDK https://ampcode.com/news/typescript-sdk
[175] Install phxagents for Amp https://phxagents.dev/install/amp/
[176] Add parallel course correction agent with forced tool call - Amp https://ampcode.com/threads/T-6ff966b8-2a71-4002-a348-1bd8eecf1754
[177] spawn | Skills Marketplace - LobeHub https://lobehub.com/pt-BR/skills/bdsqqq-dots-spawn
[178] CHANGELOG.md - mistralai/mistral-vibe https://github.com/mistralai/mistral-vibe/blob/main/CHANGELOG.md
[179] Changelog | Mistral Docs https://docs.mistral.ai/resources/changelogs
[180] Mistral Release Notes - September 2026 Latest Updates https://releasebot.io/updates/mistral
[181] Reference | mistralai/mistral-vibe | DeepWiki https://deepwiki.com/mistralai/mistral-vibe/9-reference
[182] mistral-vibe/CHANGELOG.md at main https://github.com/nagaraj9s/mistral-vibe/blob/main/CHANGELOG.md
[183] Changelog - GitHub https://raw.githubusercontent.com/mistralai/mistral-vibe/main/CHANGELOG.md
[184] Mistral Vibe Changelog — Every Release, Explained Simply https://play.google.com/store/apps/details?id=ai.mistral.chat&hl=en
[185] samouraiworld/awesome-mistral: A curated list of awesome ... - GitHub https://github.com/samouraiworld/awesome-mistral
[186] Terminally online Mistral Vibe. | Mistral AI https://mistral.ai/news/mistral-vibe-2-0/
[187] File python-mistral-vibe.changes of Package python-mistral-vibe ... https://build.opensuse.org/projects/openSUSE:Factory:PullRequest:9-Factory:IndirectPackageSet/packages/python-mistral-vibe/files/python-mistral-vibe.changes?expand=0
[188] Mistral Vibe CLI - Browse /v2.24.2 at SourceForge.net https://sourceforge.net/projects/mistral-vibe.mirror/files/v2.24.2/
[189] Releases · mistralai/mistral-vibe https://github.com/mistralai/mistral-vibe/releases
[190] Mistral Versions — every Mistral and Mixtral release with ... https://mungomash.com/ai/mistral/versions/
[191] Vibe | Mistral Docs https://docs.mistral.ai/vibe
[192] Configuration reference | Mistral Docs https://docs.mistral.ai/vibe/code/cli/configuration-reference
[193] Resources - docs.mistral.ai https://docs.mistral.ai/resources
[194] Mistral Vibe: Open-Source CLI Coding Agent and Claude ... https://www.scriptbyai.com/mistral-vibe-coding-agent/
[195] Extension settings | Mistral Docs https://docs.mistral.ai/vibe/code/vs-code-extension/settings
[196] Le Chat is now Vibe | Mistral Help Center https://help.mistral.ai/en/articles/682992-le-chat-is-now-vibe
[197] Mistral AI Cookbooks https://docs.mistral.ai/resources/cookbooks
[198] Agents | Mistral Docs https://docs.mistral.ai/vibe/code/cli/agents
[199] mistralai/mistral-vibe v2.15.0 - Signals - frontier lab intelligence https://www.onlylabs.fyi/signals/fb127fa4-2ee0-4f07-9a86-baabab869dca
[200] Grok Build: xAI's AI Coding Agent CLI Explained (2026) https://codersera.com/blog/xai-grok-build-skills-connectors-guide-2026/
[201] Plugins & Skills | Google Antigravity Docs https://antigravity.google/docs/cli/plugins/
[202] Claude Code Hooks - Verdent Guides https://www.verdent.ai/guides/claude/code-hooks
[203] grok-build-upstream-mirror/crates/codegen/xai-grok-shell ... - 光湖 https://guanghulab.com/code/bingshuo/grok-build-upstream-mirror/src/commit/47348d13ec4508dcfe440e34c6d511bb02998fb2/crates/codegen/xai-grok-shell/README.md
[204] Claude Code Hooks Tutorial: 5 Production Hooks From Scratch https://blakecrosley.com/blog/claude-code-hooks-tutorial
[205] Hooks Guide | Build This Now https://www.buildthisnow.com/blog/tools/hooks/hooks-guide
[206] Hooks | Cursor Docs https://cursor.com/docs/hooks
[207] Claude Code hooks explained: PreToolUse, PostToolUse, and Stop https://pushary.com/blog/claude-code-hooks-explained
[208] Claude Code Hooks: Automate Your AI Coding Workflow https://www.ksred.com/claude-code-hooks-a-complete-guide-to-automating-your-ai-coding-workflow/
[209] Custom Hooks Guide - grok-build-upstream-mirror - 光湖 https://guanghulab.com/code/bingshuo/grok-build-upstream-mirror/src/commit/3af4d5d39897855bdcc74f23e690024a5dc05573/crates/codegen/xai-grok-pager/docs/custom-hooks.md
[210] grok-build/README.md at main · xai-org ... https://github.com/xai-org/grok-build/blob/main/README.md
[211] Skarn manual - command-line reference for the AI coding session ... https://getskarn.com/manual/
[212] The Destructive Command Guard (dcg) is for blocking dangerous git ... https://github.com/Dicklesworthstone/destructive_command_guard
[213] ai-memory/docs/install.md at main https://github.com/akitaonrails/ai-memory/blob/main/docs/install.md
[214] grok-build-upstream-mirror/crates/codegen/xai-grok-hooks ... - 光湖 https://guanghulab.com/code/bingshuo/grok-build-upstream-mirror/src/commit/c68e39f60462f28d9be5e683d9cbe2c57b1a5027/crates/codegen/xai-grok-hooks/examples/README.md
[215] agentsentinel command - github.com/plexusone/agentsentinel - Go ... https://pkg.go.dev/github.com/plexusone/agentsentinel
