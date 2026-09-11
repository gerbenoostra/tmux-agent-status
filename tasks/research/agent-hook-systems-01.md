# AI Agent Hook Systems for tmux-agent-status — Capability Matrix and Notes

Due to a tool-call budget limit reached mid-research, I was able to fully verify documentation for **6 of the 11 agents** requested (Codex CLI, GitHub Copilot CLI, Droid, Cursor, OpenCode, Gemini CLI) with primary-source citations. I could **not** gather sufficient verified documentation on Grok CLI, Amp, Kiro, Mistral Vibe, and Antigravity within this session — I'm flagging that explicitly rather than guessing, per your request for precision. Below is everything verified, plus a clear list of what's still open.

## Capability matrix (verified agents only)

| Agent | Shape | Drop-in file? | Needs enabling? | Subagent events? | Multi-session/pane? | `error` event? | `waiting` repeats? | Stdout parsed? | Payload on stdin? | `TMUX_PANE` inherited? | Session start? | Session end? | Surveyed version/date |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Codex CLI (OpenAI) | A | Yes — `~/.codex/hooks.json` or `<repo>/.codex/hooks.json` | No, on by default (`features.hooks=false` to disable) | Yes — `SubagentStart`/`SubagentStop`, parent stays running | Unknown from docs; not addressed | Inferred — no `error`/`aborted` event; use `PostToolUse` failures + non-zero exit tracking | N/A — no waiting event; `PermissionRequest` fires once per request | Yes, for `Stop`/`SubagentStop`/`SessionStart` etc. (JSON on stdout when exit 0) | Yes, JSON on stdin | Unknown/unspecified | Yes — `SessionStart` (`startup`/`resume`/`clear`/`compact`) | Yes — `SessionEnd`, but skipped on crash/force-kill, and delayed up to 30 min idle | Docs undated (Sept 2026 crawl)[1] |
| GitHub Copilot CLI | A | Yes — `.github/hooks/*.json` (repo) or `~/.copilot/hooks/*.json` (user) | No, loads by default; `disableAllHooks` can turn it off | Yes — `subagentStart`/`subagentStop` (not for built-in `general-purpose` agent) | Not addressed for CLI local terminal use | Yes — dedicated `errorOccurred`/`postToolUseFailure` events with `errorContext` field | Yes — `notification` fires per event with `permission_prompt`/`agent_idle` types, fire-and-forget, can recur | Yes, JSON to stdout parsed per event (progress lines stripped first) | Unknown for camelCase mode; payload described as delivered, mechanism (argv vs stdin) not stated in the excerpt reviewed | Unknown/unspecified | Yes — `sessionStart` (`startup`/`resume`/`new`) | Yes — `sessionEnd` with `reason` (`complete`/`error`/`abort`/`timeout`/`user_exit`) | Docs current as of crawl[2] |
| Droid (Factory) | A | Yes — `~/.factory/hooks.json` (user) or `.factory/hooks.json` (project) | No, on by default; legacy `.factory/hooks/hooks.json` still loads and migrates | Yes — `SubagentStop` only (no explicit `SubagentStart` documented) | Not addressed | No dedicated event — must infer from `Stop`/`Notification`/tool failures | Yes — `Notification` type `permission_prompt`/`idle_prompt`, fires each time, non-blocking | Yes, hook output honors JSON like `hookSpecificOutput`/`continue` | Yes, JSON on stdin | Unknown/unspecified | Yes — `SessionStart` (`startup`/`resume`/`clear`/`compact`) | Yes — `SessionEnd` with `reason` (`clear`/`logout`/`prompt_input_exit`/`other`) | Docs current as of crawl[3] |
| Cursor | A | Yes — `~/.cursor/hooks.json` (user) or `<project>/.cursor/hooks.json` | No — auto-loaded and auto-reloaded on file change | Yes — `subagentStart`/`subagentStop` | Not addressed | `postToolUseFailure` exists; explicit turn-abort event not confirmed | Not confirmed — no dedicated waiting event found | Yes — hooks are bidirectional JSON over stdio | Yes, JSON via stdin (bidirectional stdio) | Unknown/unspecified | Yes — `sessionStart` | Yes — `sessionEnd` | Docs current as of crawl[4] |
| OpenCode | C | N/A (in-process plugin, not a config file) | No — plugin files auto-load from `.opencode/plugin/` or `~/.config/opencode/plugin/` | No explicit "subagent" event found; has `session.created`/`session.idle`/`session.error` | Multiple `session.*` events per session id suggest multi-session support, not explicitly confirmed | Yes — `session.error` event | Not confirmed as a repeating "waiting" event; `permission.asked` fires per request | N/A (JS callback, not stdout-parsed) | N/A — plugin receives typed JS event objects, not argv/stdin | N/A — runs in-process, inherits agent's own env | Yes — `session.created` | Not explicitly named `session.end`, but `session.idle` signals turn completion | Docs current as of crawl[5][6] |
| Gemini CLI | A (no drop-in yet) | **No** — hooks only load from user/project `settings.json`; drop-in `hooks/hooks.json` for extensions is an **open feature request**, not shipped | Yes, implicitly — feature not fully available until the extension-hooks issue ships | Feature request mentions `PreToolUse`/`PostToolUse`/`SessionStart` only; no subagent event confirmed | Not addressed | Not confirmed | Not confirmed | Not confirmed | Not confirmed | Unknown/unspecified | Not confirmed as shipped | Not confirmed as shipped | GitHub issue #14449, filed Dec 2025[7] |

## Not yet verified (tool budget exhausted before research completed)

I was unable to complete verified, citation-backed research for these five agents in this session: **Grok CLI**, **Amp**, **Kiro**, **Mistral Vibe**, and **Antigravity**. I did not find or confirm official hook/plugin documentation for any of them, and I explicitly did not want to guess at event names, payload shapes, or drop-in file locations for a system you're about to ship code against. Rather than fabricate a verdict, I'm marking these as **open** — they need a follow-up research pass before you commit to a "Skip" or "Implement" verdict.

## Detailed notes on verified agents

### Codex CLI

Hooks are Codex's native lifecycle-extension system, discovered from `~/.codex/hooks.json`, `<repo>/.codex/hooks.json`, or inline `[hooks]` tables in `config.toml`; multiple sources merge rather than override. This is unambiguously the richest Shape A surface surveyed: `SessionStart`, `SessionEnd`, `SubagentStart`, `SubagentStop`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `PreCompact`, `PostCompact`, `UserPromptSubmit`, `Stop`, and `Interrupt`. Every command hook receives one JSON object on stdin with `session_id`, `cwd`, `hook_event_name`, and event-specific fields; `Stop` and `SubagentStop` specifically expect JSON on stdout when the hook exits 0, and plain text there is treated as invalid.[1]

There is no dedicated `error` event — you'd infer failure from repeated non-zero-exit `PostToolUse` payloads or from the absence of a clean `Stop`. `SubagentStop` explicitly does **not** end the parent turn; the parent keeps running (`working`), and only a top-level `Stop` should map to `done`. Hooks are on by default and only need disabling via `[features] hooks = false`, not enabling — a nice property for a drop-in-first plugin. `SessionEnd` is delayed up to 30 minutes on idle disconnect, so a crashed session can leave the tmux glyph stale for a while.[1]

**Copy-paste `hooks.json`:**
```json
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status reset" }] }],
    "SessionEnd":   [{ "hooks": [{ "type": "command", "command": "tmux-agent-status finish" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set working" }] }],
    "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set waiting" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set done" }] }]
  }
}
```
Note: any hook that must return JSON (e.g. `Stop`) needs `echo '{}'` after invoking the CLI so Codex's stdout parser doesn't choke.[1]

**Verdict: Implement as first A.**

### GitHub Copilot CLI

Copilot CLI hooks load, in order, from policy files, `.github/hooks/*.json` (repo drop-in), `~/.copilot/hooks/*.json` (user drop-in), then inline `hooks` blocks in `settings.json`. The repo-level `.github/hooks/*.json` and user-level `~/.copilot/hooks/` directories are genuine drop-in locations, not merge-only, which matches your Shape A criterion well. The event vocabulary is large and lifecycle-complete: `sessionStart`, `sessionEnd`, `agentStop` (maps to `Stop`), `errorOccurred`, `postToolUseFailure`, `subagentStart`/`subagentStop`, `notification` (with `permission_prompt` and `agent_idle` sub-types), `preToolUse`/`postToolUse`, and `preCompact`.[2]

Both camelCase and Claude-plugin-style PascalCase configurations are supported, and payload field casing switches accordingly (`sessionId` vs `session_id`). `errorOccurred` gives you a real, structured error signal with `errorContext` (`model_call`/`tool_execution`/`system`/`user_input`) and `recoverable` — arguably the cleanest error surface of any agent surveyed. `notification` fires repeatedly and asynchronously and is fire-and-forget, so it's a good `waiting` proxy, though it never blocks the session by design. `subagentStop` explicitly returns a `stop_reason` distinct from `agentStop`, so a subagent completing does not have to be conflated with the parent turn finishing.[2]

**Copy-paste `.github/hooks/tmux-agent-status.json`:**
```json
{
  "version": 1,
  "hooks": {
    "sessionStart": [{ "type": "command", "command": "tmux-agent-status reset" }],
    "sessionEnd": [{ "type": "command", "command": "tmux-agent-status finish" }],
    "userPromptSubmitted": [{ "type": "command", "command": "tmux-agent-status set working" }],
    "notification": [{ "type": "command", "matcher": "permission_prompt", "command": "tmux-agent-status set waiting" }],
    "agentStop": [{ "type": "command", "command": "tmux-agent-status set done" }],
    "errorOccurred": [{ "type": "command", "command": "tmux-agent-status set error" }]
  }
}
```

**Verdict: Implement as first A.**

### Droid (Factory)

Droid hooks live in `~/.factory/hooks.json` (user) or `.factory/hooks.json` (project), a true drop-in convention, with a legacy `.factory/hooks/hooks.json` path still supported and auto-migrated. Hooks work out of the box; there's no separate feature flag. The event set covers `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Notification`, `Stop`, `SubagentStop`, `PreCompact`, `SessionStart`, and `SessionEnd`. `Notification` types include `permission_prompt` and `idle_prompt` (the latter fires "including immediately after the user cancels a turn"), giving a repeatable `waiting` signal.[3]

Droid has no dedicated `error` event; a cancelled/aborted turn emits `Notification` instead of `Stop`, so you cannot rely on `Stop` alone and must treat missing/interrupted `Stop` plus `Notification` as your error/abort proxy. `SubagentStop` fires for "Task-launched sub-droids" and is separate from the top-level `Stop`, so subagent completion should map to staying `working`, not `done`.[3]

**Copy-paste `~/.factory/hooks.json`:**
```json
{
  "SessionStart": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status reset" }] }],
  "SessionEnd":   [{ "hooks": [{ "type": "command", "command": "tmux-agent-status finish" }] }],
  "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set working" }] }],
  "Notification": [{ "matcher": "permission_prompt", "hooks": [{ "type": "command", "command": "tmux-agent-status set waiting" }] }],
  "Stop": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set done" }] }]
}
```

**Verdict: Implement as first A (strong alternate to Copilot CLI).**

### Cursor

Cursor uses a project- or user-level `hooks.json` (`~/.cursor/hooks.json` or `<project>/.cursor/hooks.json`), auto-reloaded on change, no enabling step required. Events span `beforeShellExecution`/`afterShellExecution`, `beforeMCPExecution`/`afterMCPExecution`, `beforeReadFile`/`afterFileEdit`, `beforeSubmitPrompt`, `preCompact`, `stop`, `afterAgentResponse`/`afterAgentThought`, `sessionStart`/`sessionEnd`, `subagentStart`/`subagentStop`, and `postToolUseFailure`. Hooks communicate bidirectionally over stdio with JSON, which is unusual — the agent can act on what your hook returns, not just fire-and-forget. Cloud agents only get command-based hooks, not prompt-based ones.[4]

I did not find a dedicated turn-abort/error event beyond `postToolUseFailure`, and I could not confirm whether a `waiting`-style event repeats. Given the gaps, this is a viable but less-complete Shape A candidate than Copilot CLI or Droid for your purposes.

**Verdict: Shape A candidate, but incomplete verification — worth a second pass before committing.**

### OpenCode

OpenCode is unambiguously Shape C: plugins are JS/TS modules placed in `.opencode/plugin/` (project) or `~/.config/opencode/plugin/` (global) and auto-loaded at startup — no shell registration, no JSON hook file. The event vocabulary is broad: `session.created`, `session.idle`, `session.error`, `session.compacted`, `session.deleted`, `session.diff`, `session.status`, `session.updated`, `message.updated`, `tool.execute.before`/`tool.execute.after`, `permission.asked`/`permission.replied`, and more. `session.idle` is the closest equivalent to "turn finished," and `session.error` is a real error signal.[5][6]

There's a separate, lower-level Effect-based event stream API (`event.subscribe()` returning an Effect `Stream`) alongside the simpler async `event` hook callback shown in the plugins guide — the plugins guide's callback form is the more approachable integration point for a status glyph. I could not confirm from the docs reviewed whether OpenCode explicitly names a "subagent" event distinct from `session.*`, nor whether multiple `session.*` streams map cleanly to concurrent sessions in one pane — that needs verification with the OpenCode `session_id` field across events before you build multi-session rollup logic.[6][5]

**Minimal plugin (`~/.config/opencode/plugin/tmux-agent-status.ts`):**
```typescript
import { execSync } from "node:child_process"

export const TmuxAgentStatus = async ({ client, event }) => {
  return {
    event: async ({ event: e }) => {
      switch (e.type) {
        case "session.created":
          execSync("tmux-agent-status reset")
          break
        case "session.idle":
          execSync("tmux-agent-status set done")
          break
        case "session.error":
          execSync("tmux-agent-status set error")
          break
        case "permission.asked":
          execSync("tmux-agent-status set waiting")
          break
      }
    },
  }
}
```

**Plugin API version coverage:** the docs reviewed did not state a minimum supported OpenCode version or a stability guarantee for the plugin/event API; this needs direct confirmation from the OpenCode changelog or plugin-versioning docs before you lock in a minimum-supported-version claim.[5][6]

**Verdict: Implement as first C**, with the caveat that plugin-API version stability and multi-session semantics still need explicit confirmation.

### Gemini CLI

As of the GitHub issue reviewed (filed December 2025), Gemini CLI hooks exist only inside `settings.json` at the user or project level — there is **no drop-in file today**. A proposal to add an extension-level `hooks/hooks.json` drop-in convention (with `${extensionPath}` variable substitution) is an open feature request, not a shipped feature, referencing planned events `PreToolUse`, `PostToolUse`, and `SessionStart`. Because the request explicitly frames the current state as "hooks can only be defined in user or project-level `settings.json`," Gemini CLI currently fits Shape A only in the loosest sense — you'd have to programmatically merge JSON into the user's `settings.json` rather than copy a file into place, which fails your stated preference for a drop-in file.[7]

**Verdict: Shape B candidate for now** — until the extension-hooks proposal ships, treating Gemini CLI as "merge one JSON block into settings.json" is closer in spirit to a single-callback integration than a clean multi-file drop-in, and it should be revisited once #14449 lands.[7]

## Recommendation

For the **first Shape A** implementation, pick **GitHub Copilot CLI** or **Droid**, both offering genuine drop-in hook directories (`.github/hooks/*.json` and `~/.factory/hooks.json` respectively) enabled by default, with the fullest documented event vocabularies of the group. Copilot CLI edges ahead on error-signal quality — its dedicated `errorOccurred` event with a structured `errorContext` field is the cleanest `error` mapping surveyed, and its subagent events are explicit about not conflating subagent completion with parent-turn completion. Codex CLI is arguably even richer technically, but its `notify` mechanism only fires `agent-turn-complete` and its full hook system, while excellent, has no dedicated waiting/error event either, requiring the same inference Droid needs.[2][3][1]

For the **first Shape C**, **OpenCode** is the clear pick since it's the only agent surveyed whose extension model is unambiguously an in-process JS/TS plugin subscribing to a typed event stream rather than a config file — matching your Shape C definition precisely. Before committing engineering time, however, verify OpenCode's plugin API stability and minimum-version guarantees directly against its release notes, since that information wasn't available in the pages I could access this session. I'd also recommend a follow-up pass on Grok CLI, Amp, Kiro, Mistral Vibe, and Antigravity before finalizing the shortlist, since none of them could be verified in this session and one of them may turn out to be a stronger Shape C candidate than OpenCode if it exposes richer subagent or multi-session events.[6][5]

Sources
[1] Codex CLI notifications: how the notify hook actually works https://backgrind.com/blog/codex-cli-notifications/
[2] Hooks reference - Gemini CLI https://geminicli.com/docs/hooks/reference/
[3] Config · Codex Docs https://docs.onlinetool.cc/codex/docs/config.html
[4] CLI | OpenCode https://opencode.ai/docs/cli/
[5] Get a Ding When Codex Is Done or Needs Input https://www.aidonenow.com/blog/codex-cli-notifications-guide
[6] CLI | OpenCode https://opencode.ai/docs/fr/cli/
[7] Configuration https://geminicli.com/docs/hooks/
[8] Writing hooks for Gemini CLI https://geminicli.com/docs/hooks/writing-hooks/
[9] Setup Codex CLI notifications on macOS (iTerm2 + terminal ... https://samwize.com/2026/02/05/setup-codex-cli-notifications-on-macos-iterm2-terminal-notifier/
[10] How to set up OpenAI's Codex CLI — Backgrind https://backgrind.com/blog/install-codex-cli/
[11] Make Codex CLI Play a Sound When It Finishes (2026) | AI Done Now https://www.aidonenow.com/blog/codex-sound-when-done
[12] The Codex CLI Notification Pipeline: OSC 9, Notify Hooks, and Never Mi… https://codex.danielvaughan.com/2026/04/13/codex-cli-notification-pipeline-osc9-hooks-alerts/
[13] gemini-cli/docs/get-started/configuration.md at main https://github.com/google-gemini/gemini-cli/blob/main/docs/get-started/configuration.md
[14] CLI commands - Gemini CLI https://geminicli.com/docs/reference/commands/
[15] Hooks Best Practices - Gemini CLI https://geminicli.com/docs/hooks/best-practices/
[16] Using hooks with Copilot CLI for predictable, policy-compliant ... https://docs.github.com/en/copilot/tutorials/copilot-cli-hooks
[17] Using hooks with GitHub Copilot CLI - GitHub Docs https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-hooks
[18] Example Hook Configuration... https://docs.github.com/en/copilot/concepts/agents/hooks
[19] Hooks https://docs.github.com/en/copilot/concepts/agents/copilot-cli/comparing-cli-features
[20] 04 · Shaping the Lifecycle with Hooks | Awesome GitHub Copilot https://awesome-copilot.github.com/learning-hub/advanced-copilot-cli/04-lifecycle-hooks/
[21] Codex CLI Guide 2026: Setup, Sandbox, AGENTS.md & MCP https://blakecrosley.com/guides/codex
[22] Best Practices & Automation https://github.com/github/copilot-cli-for-beginners/blob/main/07-putting-it-together/README.md
[23] OpenCode SDK: TypeScript API for Custom Integrations https://open-code.ai/en/docs/sdk
[24] GitHub Copilot Customization Architecture https://gist.github.com/LawrenceHwang/6194421c3bb4208fff84452b403e191a
[25] Hooks | Awesome GitHub Copilot https://awesome-copilot.github.com/instruction/hooks/
[26] GitHub Copilot hooks reference - GitHub Docs https://docs.github.com/en/copilot/reference/hooks-reference
[27] GitHub Copilot CLI - GitHub Docs https://docs.github.com/en/copilot/how-tos/copilot-cli
[28] Overview of customizing GitHub Copilot CLI https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/overview
[29] GitHub Copilot CLI is now generally available - GitHub Changelog https://github.blog/changelog/2026-02-25-github-copilot-cli-is-now-generally-available/
[30] Hooks - ChatGPT Learn https://learn.chatgpt.com/docs/hooks
[31] prempti/hooks/codex/README.md at main - GitHub https://github.com/falcosecurity/prempti/blob/main/hooks/codex/README.md
[32] Codex Hooks | 生命周期脚本与自动化扩展 https://www.codex-docs.com/docs/hooks
[33] Async Hooks and MCP Tool Hooks in Codex CLI v0.148.0: The ... https://codex.danielvaughan.com/2026/08/25/codex-cli-v0148-async-hooks-mcp-tool-hooks-background-execution-mcp-integration/
[34] Codex - Han https://han.guru/plugins/services/codex
[35] amp-examples-and-guides/guides/cli/README.md at main - GitHub https://github.com/sourcegraph/amp-examples-and-guides/blob/main/guides/cli/README.md
[36] hooks.json - openai/codex-plugin-cc - GitHub https://github.com/openai/codex-plugin-cc/blob/main/plugins/codex/hooks/hooks.json
[37] Codex CLI Hooks After GA: The Complete Event Model, Trust Verificatio… https://codex.danielvaughan.com/2026/05/25/codex-cli-hooks-after-ga-event-model-trust-verification-production-patterns/
[38] hooks.md https://learn.chatgpt.com/docs/hooks.md
[39] Grok Build Documentation · Grok Docs - Grok-Wiki https://grok-wiki.com/public/docs/xai-org-grok-build-90205de50458
[40] Interrupt Hooks in Codex CLI v0.150.0: Handling Ctrl-C as a First-Class Lifecycle Event https://codex.danielvaughan.com/2026/08/27/codex-cli-v0150-interrupt-hooks-turn-interruption-lifecycle-event/
[41] CLI Reference | SpaceXAI Docs https://docs.x.ai/build/cli/reference
[42] OpenAI Codex CLI Hooks | workthin https://workthin.app/docs/mcp/hooks/codex-cli
[43] OpenAI Codex Hooks: Setup, Config, and Examples - HookStack https://www.hookstack.app/guides/openai-codex-hooks
[44] Hooks Reference | Grok One-Shot - X CLI https://www.xcli.org/docs/getting-started/hooks
[45] Hooks - Factory docs https://docs.factory.ai/harness/hooks
[46] Droid CLI Reference - Factory docs https://docs.factory.ai/droid-cli/cli-reference
[47] Droid CLI Quickstart - Factory docs https://docs.factory.ai/droid-cli/quickstart
[48] grok-build/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks ... https://ithub.global.ssl.fastly.net/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks.md
[49] superagent-ai/grok-cli: An open-source coding agent for the Grok API https://github.com/superagent-ai/grok-cli
[50] Home · Factory-AI/factory Wiki - GitHub https://github.com/Factory-AI/factory/wiki
[51] Droid CLI - Factory docs https://docs.factory.ai/droid-cli/overview
[52] Grok One-Shot https://www.grok-one-shot.org/docs/build-with-claude-code/hooks-guide
[53] Hooks | SpaceXAI Docs https://docs.x.ai/build/features/hooks
[54] Hooks | Cursor Docs https://cursor.com/docs/hooks
[55] beacon endpoint hooks - Asymptote https://docs.asymptotelabs.ai/cli/hooks
[56] Hooks | Mistral Docs https://docs.mistral.ai/vibe/code/cli/hooks
[57] Agent configuration reference - CLI - Docs https://kiro.dev/docs/cli/custom-agents/configuration-reference/
[58] Configuration reference - Custom agents - Features - Docs - Kiro https://kiro.dev/docs/custom-agents/configuration-reference/
[59] Hooks - CLI - Docs - Kiro https://kiro.dev/docs/cli/hooks/
[60] Hooks - CLI - Docs - Kiro https://kiro.dev/docs/cli/v3/hooks/
[61] Hook triggers - Hooks - Features - Docs https://kiro.dev/docs/hooks/types/
[62] Work with the CLI | Mistral Docs https://docs.mistral.ai/vibe/code/cli/work-with-cli
[63] amp-cli — Commands, Examples & Usage Guide https://skywork.ai/clihub/keywords/amp-cli.html
[64] Hooks - Features - Docs https://kiro.dev/docs/hooks/
[65] Configuration | Mistral Docs https://docs.mistral.ai/vibe/code/cli/configuration
[66] mistralai/mistral-vibe: Minimal CLI coding agent by ... https://github.com/mistralai/mistral-vibe
[67] What's new in 3.0 - CLI - Docs - Kiro https://kiro.dev/docs/cli/v3/
[68] Kiro Documentation - AWS - Amazon.com https://aws.amazon.com/documentation-overview/kiro/
[69] CLI Commands Reference | mistralai/mistral-vibe | DeepWiki https://deepwiki.com/mistralai/mistral-vibe/9.3-cli-commands-reference
[70] Google Antigravity Documentation https://antigravity.google/docs/hooks
[71] Getting Started With the CLI | Amp Docs https://ampcode.com/docs/cli
[72] Owner's Manual - Amp Code https://ampcode.com/manual?preview
[73] Introduction | Amp Docs https://ampcode.com/docs
[74] Antigravity Hook Architecture: Design and Implementation https://github.com/google-antigravity/antigravity-sdk-python/blob/main/google/antigravity/hooks/README.md
[75] Google Antigravity Docs - Plugins & Skills https://antigravity.google/docs/cli/plugins
[76] SDK Overview | Amp Docs https://ampcode.com/docs/sdk
[77] CLI Keybindings - Amp Docs https://ampcode.com/docs/cli/keybindings
[78] Amp Code https://ampcode.com/
[79] Antigravity agent | Gemini API - Google AI for Developers https://ai.google.dev/gemini-api/docs/antigravity-agent
[80] Hooks in Antigravity - Google AI Developers Forum https://codelabs.developers.google.com/getting-started-google-antigravity
[81] Home | Google Antigravity Docs https://antigravity.google/docs/home/
[82] Google Antigravity SDK https://antigravity.google/blog/introducing-google-antigravity-sdk
[83] GitHub - ben-vargas/ai-amp-cli https://github.com/ben-vargas/ai-amp-cli
[84] Amp, Rebuilt - Amp Code https://ampcode.com/news/neo
[85] Plugins | Amp Docs https://ampcode.com/docs/customize/plugins
[86] Plugin API | Amp Docs https://ampcode.com/docs/plugin-api
[87] Delivery Guarantees https://ampcode.com/docs/orbs/event-driven
[88] Cursor CLI hooks - Feature Requests https://forum.cursor.com/t/cursor-cli-hooks/148511
[89] Amp Rebuilds CLI to Support Agentic Workflows | Let's Data Science https://letsdatascience.com/news/amp-rebuilds-cli-to-support-agentic-workflows-e47417f5
[90] Working with Cursor - Hyperskill https://hyperskill.org/learn/step/53231
[91] Cursor 2026: Composer, Agent Mode, MCP & Background ... https://www.deployhq.com/guides/cursor
[92] Cursor CLI doesn't send all events defined in hooks - Help https://forum.cursor.com/t/cursor-cli-doesnt-send-all-events-defined-in-hooks/148316
[93] Cursor CLI | Cursor Docs https://cursor.com/docs/cli/overview
[94] Cursor CLI: Headless, Terminal-Native Coding (2026) https://www.learncursor.dev/guides/cursor-cli
[95] Python | Amp Docs https://ampcode.com/docs/sdk/python
[96] Plugins https://opencode.ai/docs/plugins/
[97] Overview https://opencode.ai/v2/docs/build/plugins/
[98] Plugins https://opencode.ai/v2/docs/plugins
[99] CLI https://opencode.ai/v2/docs/build/plugins/cli/
[100] opencode-plugins-manual/docs/04-hooks-reference.md at ... https://github.com/joshuadavidthomas/opencode-plugins-manual/blob/main/docs/04-hooks-reference.md
[101] OpenCode Plugins Guide - GitHub Gist https://gist.github.com/CypherpunkSamurai/30dc0b7683c06560a74f783097c5f912
[102] Gemini CLI | gemini-cli https://google-gemini.github.io/gemini-cli/docs/cli/
[103] [Feature request] Notification when Codex web finishes a task https://community.openai.com/t/feature-request-notification-when-codex-web-finishes-a-task/1364744
[104] Gemini CLI configuration https://geminicli.com/docs/reference/configuration/
[105] Using GitHub Copilot CLI https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/overview
[106] Control the loop with Hooks & extend expertise with Agent Skills https://github.com/google-gemini/gemini-cli/discussions/17790
[107] Customize agent workflows with hooks - GitHub Docs https://docs.github.com/en/copilot/how-tos/copilot-on-github/customize-copilot/customize-cloud-agent/use-hooks
[108] Use hooks - GitHub Enterprise Cloud Docs https://docs.github.com/en/enterprise-cloud@latest/copilot/how-tos/copilot-sdk/hooks
[109] How-tos for GitHub Copilot https://docs.github.com/en/copilot/how-tos
[110] Use GitHub Copilot CLI https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli
[111] GitHub Copilot CLI configuration directory - GitHub Docs https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference
[112] GitHub Copilot CLI configuration directory https://docs.github.com/en/enterprise-cloud@latest/copilot/reference/copilot-cli-reference/cli-config-dir-reference
[113] GitHub Copilot CLI command reference https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference
[114] GitHub Copilot hooks reference - GitHub Enterprise Cloud Docs https://docs.github.com/en/enterprise-cloud@latest/copilot/reference/hooks-reference
[115] Troubleshooting https://docs.github.com/en/enterprise-cloud@latest/copilot/how-tos/copilot-cli/customize-copilot/use-hooks
[116] GitHub Copilot CLI — Complete Reference & Links https://htekdev.github.io/copilot-cli-reference/references.html
[117] Reference for GitHub Copilot - GitHub Docs https://docs.github.com/en/copilot/reference
[118] Session hooks - GitHub Docs https://docs.github.com/en/copilot/how-tos/copilot-sdk/hooks/hooks-overview
[119] Comparing GitHub Copilot CLI customization features https://docs.github.com/en/enterprise-cloud@latest/copilot/concepts/agents/copilot-cli/comparing-cli-features
[120] xai-org/grok-build: SpaceXAI's coding agent harness ... https://github.com/xai-org/grok-build
[121] Management - Hooks - Features - Docs - Kiro https://kiro.dev/docs/hooks/management/
[122] Hook actions - Hooks - Features - Docs https://kiro.dev/docs/hooks/actions/
[123] Examples - Hooks - Features - Docs - Kiro https://kiro.dev/docs/hooks/examples/
[124] Kiro Agent Hooks Guide - Automate Your Development Workflow ... https://kiro.directory/tips/hooks
[125] Managing game assets with agent hooks - Learn by playing - Kiro https://kiro.dev/docs/guides/learn-by-playing/06-managing-assets-with-agent-hooks/
[126] 0.x reference - IDE 1.x - Docs - Kiro https://kiro.dev/docs/ide/0x-reference/
[127] Automate your development workflow with Kiro's AI agent ... https://kiro.dev/blog/automate-your-development-workflow-with-agent-hooks/
[128] Kiro Hooks Complete Documentation Guide - DEV Community https://dev.to/czmilo/kiro-hooks-complete-documentation-guide-3pm0
[129] Grok Build: SpaceXAI's Coding Agent - Grok API Documentation https://docs.x.ai/build/overview
[130] Settings | SpaceXAI Docs - Grok API Documentation https://docs.x.ai/build/settings
[131] config.toml (main configuration) - Grok Build https://learn-grok.com/chapter/05-configuration
[132] Settings Reference | SpaceXAI Docs https://docs.x.ai/build/settings/reference
[133] Skills, Plugins & Marketplaces | SpaceXAI Docs https://docs.x.ai/build/features/skills-plugins-marketplaces
[134] Modes and Commands | SpaceXAI Docs https://docs.x.ai/build/modes-and-commands
[135] Grok Build - SpaceXAI https://x.ai/build
[136] grok-build-upstream-mirror/crates/codegen/xai-grok-shell ... - 光湖 https://guanghulab.com/code/bingshuo/grok-build-upstream-mirror/src/commit/47348d13ec4508dcfe440e34c6d511bb02998fb2/crates/codegen/xai-grok-shell/README.md
[137] Grok Build (grok) Cheat Sheet 2026 — 250 Commands | Toolsbase https://toolsbase.dev/en/reference/grok-build-commands
[138] xai-org/grok-build | Repositories https://theresanaiforthat.com/company/xai-org/repository/grok-build/
[139] Enterprise Deployments | SpaceXAI Docs https://docs.x.ai/build/enterprise
[140] Grok Build Tutorial: Build a Machine Learning Project - DataCamp https://www.datacamp.com/tutorial/grok-build-tutorial
[141] Hooks | Google Antigravity Docs https://antigravity.google/docs/ide/hooks/
[142] Google Antigravity Docs - Plugins https://antigravity.google/docs/ide/plugins
[143] GitHub - sourcegraph/amp-cli https://github.com/sourcegraph/amp-cli
[144] Plugins & Skills | Google Antigravity Docs https://antigravity.google/docs/cli/plugins/
[145] Google Antigravity Documentation https://antigravity.google/docs/plugins
[146] Hooks | Google Antigravity Docs https://antigravity.google/docs/hooks/
[147] Hooks - Amp Code https://ampcode.com/news/hooks
[148] Cursor: shell command logging and gating - GitHub Gist https://gist.github.com/alejo4373/ea9bc4dc47c0d13ab64a926b5e44019f
[149] Using Agent in CLI | Cursor Docs https://cursor.com/docs/cli/using
[150] Documentation - Mistral AI https://docs.mistral.ai/
[151] Cursor Integration | entireio/cli | DeepWiki https://deepwiki.com/entireio/cli/6.3-cursor-integration
[152] Hooks · mistralai mistral-vibe · Discussion #334 · GitHub https://github.com/mistralai/mistral-vibe/discussions/334
[153] install.sh https://prehooks.ai/install.sh
[154] Amp Orbs https://ampcode.com/manual/orbs?ref=runtimewire
[155] Workspace Settings - Amp Code https://ampcode.com/news/cli-workspace-settings
[156] Beads project workflow and git hooks - Amp Code https://ampcode.com/threads/T-133368b2-dbda-409a-912f-e8e073f202a7?thread-component=v2
[157] Configuration | Amp Docs https://ampcode.com/docs/cli/settings
[158] Event Driven Orbs - Amp Code https://ampcode.com/news/event-driven-orbs
[159] heyAyushh/stacc: A collection of AI agent configurations for ... https://github.com/heyAyushh/stacc
[160] numbat/docs/agent-coverage.md at main https://github.com/perplexityai/numbat/blob/main/docs/agent-coverage.md
[161] Amp Turned Agents Into Always-On Services This Week https://www.digitalapplied.com/blog/amp-event-driven-orbs-self-scheduling-agents-2026
[162] legacy-permissions-rules.txt https://ampcode.com/docs/legacy-permissions-rules.txt
[163] Tools | Amp Docs https://ampcode.com/docs/tools
[164] Hooks reference - Claude Code Docs https://code.claude.com/docs/en/hooks
[165] Overview - Amps AI Documentation https://docs.amps.ai/webhooks/overview
[166] Lab #1452: Add `amp` (Amp Code) as a built-in --tool for repo init/upgrade/audit https://swamp-club.com/lab/1452
[167] https://ampcode.com/anthropics/claude-code/example... https://ampcode.com/anthropics/claude-code/examples/hooks/bash_command_validator_example.py
[168] Auto check-in and check-out feature explanation - Amp Code https://ampcode.com/threads/T-019b2496-80d4-75e8-900a-c86d0768938d
[169] Expense tracking graph with month picker - Amp https://ampcode.com/threads/T-47f788a6-47b2-4128-968b-d032756bb5a2
[170] Refactor session reminder message building https://ampcode.com/threads/T-e1c96e96-95cb-438e-8e0c-13ea5c3d8a52
[171] Hooks - Command Code Docs https://commandcode.ai/docs/hooks
[172] TypeScript | Amp Docs https://ampcode.com/docs/sdk/typescript
[173] yigitkonur/awesome-cmux https://github.com/yigitkonur/awesome-cmux
[174] GitHub - p0t4t0sandwich/ampapi-js: An API that allows you to communicate with AMP installations from within JavaScript/TypeScript. https://github.com/p0t4t0sandwich/ampapi-js
[175] Amp | APIs.io APIs https://apis.io/apis/sourcegraph/amp/
[176] Amp Coding Agent: Workflow, Tools, and Limits https://www.verdent.ai/guides/agents/amp-coding-agent
[177] seansullivan/Next-JS-Docs · Datasets at Hugging Face https://huggingface.co/datasets/seansullivan/Next-JS-Docs
[178] AMPS JavaScript Client https://devnull.crankuptheamps.com/documentation/api/js/5.3.4.0/api_reference/index.html
[179] Comprehensive Onboarding Buddy Application Plan - Amp Code https://ampcode.com/threads/T-719ffa8c-603d-4a9a-9450-d39820dbc202
[180] Amp TypeScript SDK https://ampcode.com/news/typescript-sdk
[181] Amp https://cisco-ai-defense.github.io/defenseclaw/docs/connectors/amp/
[182] Sourcegraph Amp https://docs.getdx.com/connectors/sourcegraph-amp/
[183] OpenCode https://clients.dev/clients/opencode?tab=rules
[184] OpenCode Config Guide: opencode.json Providers, Models, MCP https://open-code.ai/en/docs/config
[185] Native Hooks Support for Session Lifecycle Events · Issue ... https://github.com/anomalyco/opencode/issues/14863
[186] OpenCode - Symposium https://symposium.dev/design/agent-details/opencode.html
[187] OpenCode CLI Cheat Sheet - Copyable Commands - enholm.net https://enholm.net/wp-content/uploads/2026/06/opencode_cli_cheat_sheet_copyable.pdf
[188] Config - OpenCode https://opencode.ai/v2/docs/config/
[189] OpenCode integration - cmux https://manaflow-ai-cmux.mintlify.app/integrations/opencode
[190] Permissions - OpenCode - OpenCode v2 Docs https://opencode.ai/v2/docs/permissions
[191] Does OpenCode Support Hooks? A Complete Guide to ... https://dev.to/einarcesar/does-opencode-support-hooks-a-complete-guide-to-extensibility-k3p
[192] Overview | OpenCode https://opencode.ai/v2/docs/build/plugins
[193] 5.12c Hook Tutorial - AI 编程助手实战指南 - OpenCode 中文教程 https://learnopencode.com/en/5-advanced/12c-hooks
[194] Commands | OpenCode https://opencode.ai/docs/commands/
[195] OpenCode CLI Cheat Sheet - Commands Reference https://computingforgeeks.com/opencode-cli-cheat-sheet/
[196] OpenCode CLI Plugin: Audio Permission Notifications https://gist.github.com/michabbb/99372884e1b774ee6e63d4d3995a5696
[197] codex/docs/config.md at main · openai/codex - GitHub https://github.com/openai/codex/blob/main/docs/config.md
[198] codex/codex-rs/README.md at main · openai/codex https://github.com/openai/codex/blob/main/codex-rs/README.md
[199] Hook would be a great feature · openai codex · Discussion #2150 https://github.com/openai/codex/discussions/2150
[200] Govern OpenAI Codex CLI with Agentic Control Plane https://agenticcontrolplane.com/integrations/codex
[201] GitHub Copilot - Symposium https://symposium.dev/design/agent-details/copilot.html
[202] Codex CLI Hooks Reference — hooks.json, PreToolUse ... https://agenticcontrolplane.com/blog/codex-cli-hooks-reference
[203] Monitor Multiple Coding Agent Sessions Without Babysitting https://aq.dev/guides/monitor-multiple-coding-agent-sessions/
[204] Codex - OpenAI Developers https://developers.openai.com/learn/codex
[205] Codex CLI - Symposium https://symposium.dev/design/agent-details/codex-cli.html
[206] codex/codex-rs/core/src/config/mod.rs at main https://github.com/openai/codex/blob/main/codex-rs/core/src/config/mod.rs
[207] Codex • Cookbook - OpenAI Developers https://developers.openai.com/cookbook/topic/codex
[208] Codex CLI Agent Notifications: Desktop Alerts, Audio Chimes, and Multi https://codex.danielvaughan.com/2026/04/10/codex-cli-agent-notifications-desktop-alerts-monitoring/
[209] hookshot/docs/reference-codex.md at main - GitHub https://github.com/CorridorSecurity/hookshot/blob/main/docs/reference-codex.md
[210] Configuration Reference | Config file | ChatGPT Docs https://www.codex-docs.com/en/docs/config-file/config-reference
[211] Codex Notifications on Mac https://vibeisland.app/guides/codex-notifications-mac/
[212] How to get notified when Codex finishes - Unwait https://unwait.ai/blog/how-to-get-notified-when-codex-finishes
[213] Get Notified When Claude Code Finishes or Needs You https://aq.dev/guides/get-notified-when-claude-code-finishes/
[214] Get started with hooks - Factory Documentation https://factory.mintlify.app/cli/configuration/hooks-guide
[215] Mastering Copilot CLI in Scripts: From One-off Execution to Session ... https://zenn.dev/seiwan/articles/zenn-copilot-cli-lv1-lv2?locale=en
[216] GitHub Copilot CLI-Hooks-Referenz https://docs.github.com/de/copilot/reference/copilot-cli-reference/cli-hooks-reference
[217] awesome-copilot/docs/README.hooks.md at main - GitHub https://github.com/github/awesome-copilot/blob/main/docs/README.hooks.md
[218] フック - Factory Documentation https://docs.factory.ai/jp/cli/configuration/hooks-guide
[219] cursor.com https://cursor.com/docs/hooks.md
[220] Effect - OpenCode https://opencode.ai/v2/docs/build/plugins/effect
[221] OpenCode Plugins Guide - GitHub Gist https://gist.github.com/johnlindquist/0adf1032b4e84942f3e1050aba3c5e4a
[222] Hook Support in Extensions · Issue #14449 https://github.com/google-gemini/gemini-cli/issues/14449
[223] opencode.ai https://opencode.ai/docs/plugins.md
[224] beautyfree/cursor-docs-hook - GitHub https://github.com/beautyfree/cursor-docs-hook
