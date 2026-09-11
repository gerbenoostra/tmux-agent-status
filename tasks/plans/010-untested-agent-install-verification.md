# 010 - Untested agent install verification

Status: done - every fix below is applied. `cargo fmt`, `cargo clippy --all-targets`, `cargo test`
and `prek run --all-files` are green.

Covers *actually installing the six agents `005` shipped drop-ins for from research alone*
(Codex CLI, GitHub Copilot CLI, Cursor, Droid, Grok CLI, Kiro) and confirming the shipped
`share/agents/<agent>/` file and the `docs/agents/<agent>.md` mapping table match the real CLI,
not just its docs. Claude Code, Mistral Vibe and Devin CLI were already verified this way; these
six were not. One subagent per agent installed the real CLI, dropped in the file, and pushed as
far as possible without ever supplying real credentials. Raw logs for every command are under the
session scratchpad (not committed - ephemeral, and some paths are machine-local), one directory per
agent: `codex/`, `copilot/`, `cursor/`, `droid/`, `grok/`, `kiro/`. Re-run the same subagent prompts
to regenerate them; nothing here depends on the logs surviving.

## Result by agent

### Codex CLI - verified live, one doc gap

Installed via `pnpm add -g @openai/codex` (0.153.4). The drop-in's `SessionStart`, `UserPromptSubmit`
and `Stop` events were all observed firing for real, including on an aborted turn (`Stop` still fired,
matching the docs' `error`-is-inferred stance), by running `codex exec --dangerously-bypass-hook-trust`
in a throwaway tmux session and polling `@agent_pane_status`. `PostToolUse`, `PermissionRequest`,
`SubagentStart/Stop` and `SessionEnd` remain unverified (need a real tool call / approval / subagent /
teardown that only happens post-auth).

**Fixed.** `docs/agents/codex.md` Quirks now says hooks are gated behind a "persisted hook trust"
prompt on first run - accept it interactively once, or pass `--dangerously-bypass-hook-trust` for
automation.

### GitHub Copilot CLI - matches, one footnote worth adding

Installed via `npm install -g @github/copilot` (1.0.83). Couldn't run a real session (auth-gated past
the ACP `initialize` handshake), but statically confirmed every path, event name and notification
subtype the drop-in and docs use against the shipped `app.js` bundle and `schemas/api.schema.json` -
all correct, high confidence.

**Fixed.** `docs/agents/copilot.md` Quirks now notes repo-scope hooks are gated behind the same
folder-trust prompt every repo session needs; the "no enable step is required" line stays, since it's
not a hooks-specific step.

### Cursor - matches, one unconfirmed claim worth flagging

Installed via the official `curl https://cursor.com/install -fsS | bash` (`cursor-agent
2026.09.08-6caf4ff`). Every session start failed instantly on `Error: Authentication required`, before
any hook activity, so live firing was unverifiable. Instead extracted and grepped the installed JS
bundle directly: every event name in the drop-in exists in the real binary, the reader accepts our
file's shape (`{"hooks": {...}}` without a top-level `"version"` key - not a bug), and no `error` hook
event exists anywhere in the bundle, confirming the docs' "no error event" claim.

**Fixed (as a caveat, not a rewrite).** `docs/agents/cursor.md` Quirks now flags, as unconfirmed and
second-hand, that a Cursor community forum thread claims headless (`-p`) sessions only fire
`sessionStart`/`sessionEnd`, never `beforeSubmitPrompt`/`stop`. Not reproduced; relevant to anyone
pointing this hook file at CI rather than the interactive CLI.

### Droid (Factory) - verified live, no issues

Installed via the official `curl -fsSL https://app.factory.ai/cli | sh` (0.216.0). Copied the drop-in
to a scratch project's `.factory/hooks.json` (project scope, deliberately not touching the real
`~/.factory/hooks.json`, which already held unrelated user config) and ran `droid exec`. It failed on
auth, but not before Factory's own log file recorded `SessionStart` and `SessionEnd` matching and
executing `tmux-agent-status reset`/`finish` successfully - both to completion, exit code 0. Also
cross-checked the doc pages directly (not just search snippets): all 9 event names, the three file
scopes including the legacy auto-migrated path, and the four `Notification` subtypes match exactly.

**Fix:** none. Confidence high on the parts observed firing, moderate on the rest (same auth wall).

One process note: the subagent's own sandbox had `$TMUX` pointed at its own private tmux server, and
a doctor/exec run briefly wrote a real `working` state to that pane before the subagent caught it and
ran `tmux-agent-status finish`. Not a finding about the agent - a reminder that any future live test
of a hook-firing agent should start from a `tmux -L <name>` throwaway session before running the
agent at all, not just before polling it.

### Grok CLI - real doc drift, two possibly-missing mappings

Installed via `npm i -g @xai-official/grok` (`grok 1.0.25`) - confirmed as xAI's actual CLI (maintainer
`xai-security@x.ai`), not the unrelated community `@vibe-kit/grok-cli` package the task briefing
warned about. `grok inspect` is a genuine pre-auth discovery command: it confirmed the drop-in loads
correctly from `~/.grok/hooks/`, and that copying the same file into an *untrusted* project's
`.grok/hooks/` is correctly excluded until `/hooks-trust` runs - both exactly as documented.

**Fixed in `docs/agents/grok.md`, deliberately without adding new mappings:**
- Removed the false claim of an inline TOML `[[hooks]]` alternative from the intro.
- Corrected the plugin-route aside: `Notification` and `PostToolUseFailure`/`UserPromptSubmit`/
  `PermissionDenied`/`SubagentStart`/`SubagentStop`/`PreCompact`/`PostCompact` are real Grok events,
  contrary to the old "have no Grok equivalent" line.
- Left the `waiting` and `error` table rows unmapped rather than guessing what `Notification` and
  `PostToolUseFailure` actually carry - added Quirks entries explaining these events are real but
  their payload/trigger semantics are unverified without an authenticated session, so someone can add
  the row once confirmed instead of us shipping another unverified mapping (the exact defect `005`
  and `009` both warn about).

### Kiro - the one real mismatch, drop-in likely wrong for the default engine

Installed via the official `curl -fsSL https://cli.kiro.dev/install | bash` (`kiro-cli 2.21.2`,
confirmed genuine - it is the Amazon Q Developer CLI lineage, using `~/.aws/amazonq/cli-agents/` and
AWS SSO/SigV4 internals). Every auth-gated command failed cleanly and fast except `chat
--no-interactive`, which started opening a browser login flow before being killed - no credentials
were exchanged.

**High-confidence mismatch**, found by running `strings` on the actual 1.5GB `kiro-cli-chat` binary:

- The real, hardcoded trigger set for the CLI's default engine is
  `["agentSpawn","userPromptSubmit","preToolUse","postToolUse","stop"]` - **camelCase**, and there is
  no `AgentStop`, only `stop`. The shipped `share/agents/kiro/tmux-agent-status.json` and
  `docs/agents/kiro.md` both use PascalCase (`AgentSpawn`, `PreToolUse`, `PostToolUse`, `AgentStop`).
- No reference to a standalone `.kiro/hooks/*.json` drop-in directory exists anywhere in the binary.
  Hooks instead appear to load from a `"hooks"` field embedded inside a per-agent config JSON, under
  `~/.aws/amazonq/cli-agents/*.json` or a workspace agent file - a materially different install shape
  than "drop a file in a hooks directory."
- kiro.dev's current public docs (`/docs/hooks/`, `/docs/hooks/types/`) describe exactly the
  PascalCase, standalone-file system the drop-in was written from - but that appears to belong to
  Kiro's `--v3` ("next generation") engine, which `kiro-cli` does not default to, and which could not
  be reached without logging in.
- `~/.config/kiro/hooks/` (the doc's documented user scope) is also unconfirmed; a CLI 2.13 changelog
  points at `~/.kiro/hooks/` instead, and even that may be `--v3`-only.

**Fixed, within what could actually be confirmed:**
- `share/agents/kiro/tmux-agent-status.json` now uses the confirmed camelCase trigger names
  (`agentSpawn`, `preToolUse`, `postToolUse`, `stop`), taken directly from the installed binary's own
  trigger set. The per-hook object shape (`name`/`trigger`/`action.command`) was left unchanged -
  there's no evidence either way on that shape, only on the trigger names.
- `docs/agents/kiro.md` no longer presents this as a drop-in file to copy verbatim. It now documents
  the CLI's default engine as reading hooks from a `"hooks"` field embedded in a per-agent config JSON
  (`~/.aws/amazonq/cli-agents/*.json`), and tells the reader to merge the shipped file's `hooks` array
  into their own agent config by hand, rather than shipping an untested guess at that embedded format.
- The page's intro and the `docs/agents/README.md` matrix row both flag that this was verified against
  `kiro-cli` 2.21.2's default (v2) engine only, and that whether `--v3` reaches the standalone-file,
  PascalCase system kiro.dev's own docs describe is still open - explicitly not "unconfirmed" the way
  the other five agents' looser ends are, but "known to differ from what was previously shipped."

**Still open:** someone who can authenticate to Kiro needs to confirm (a) whether `--v3` is reachable
and really matches kiro.dev's PascalCase/standalone-file docs, and (b) the exact shape of the
default engine's agent-config `"hooks"` field, since the merge instructions above assume our existing
per-hook object shape carries over unchanged.

## What's still not verified anywhere

Every agent above hit the same wall past the auth boundary: `PostToolUse`/tool-call events, a real
permission prompt, subagent start/stop, and session end/teardown in a *live, authenticated* run.
None of the six was driven through an actual tmux pane end to end the way Claude Code, Mistral Vibe
and Devin CLI were in prior work. That gap is real work, not a doc fix, and needs either test
accounts/API keys for each service or a volunteer who already has them.

## What was fixed and how

All doc/file corrections above are applied: `docs/agents/{codex,copilot,cursor,grok,kiro}.md`,
`docs/agents/README.md`'s matrix row and Verified date for all six agents tested this round, and
`share/agents/kiro/tmux-agent-status.json`'s trigger names. `tests/agent_configs.rs` (the drop-in/docs
drift validator from `009`) still passes - it checks command syntax and state coverage, which the
Kiro trigger-name rewrite didn't change. Droid needed no changes.

No new event mappings were added anywhere (Grok's `waiting`/`error`, Kiro's default-engine hook
delivery shape) where the only evidence was a real-but-unverified event name - consistent with `005`'s
"a wrong glyph is worse than no glyph" and `009` item 3's proof that guessing a mapping from a plausible
event name is exactly how that mistake happens twice.

## Still open, needs an authenticated session

- Kiro: whether `--v3` reaches the PascalCase/standalone-file system kiro.dev's docs describe, and the
  exact shape of the default engine's agent-config `"hooks"` field.
- Grok: what `Notification` and `PostToolUseFailure` actually carry, to decide if they can become real
  `waiting`/`error` mappings.
- Every agent: `PostToolUse`/tool-call events, a real permission prompt, subagent start/stop, and
  session end/teardown in a live, authenticated run, the way Claude Code, Mistral Vibe and Devin CLI
  were driven end to end in prior work.
