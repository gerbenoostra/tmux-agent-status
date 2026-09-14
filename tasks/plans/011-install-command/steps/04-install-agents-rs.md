# Step 4: `src/install/agents.rs`

## Scope

The agent table (name, detect directory, detect command, target path, merge class, embedded contents),
detection, JSON merges, the TOML append, and the Claude Code plugin route.

## Relevant design

From [design.md](../design.md):

### Detection

Two independent signals per agent, both reported so the user can see why something was preselected:

- its config directory exists (`~/.codex`, `~/.cursor`, `~/.factory`, `~/.grok`, `~/.kiro`,
  `~/.vibe`, `~/.copilot`, `~/.claude`, `~/.gemini`, `~/.config/devin`)
- its command is on `PATH`

An agent is **preselected** when either hits, listed but unselected when neither does. Never
installed without appearing in the list.

### Delivery, in preference order

| Class | Agents | What we do | Risk |
| --- | --- | --- | --- |
| **plugin** | Claude Code | shell out to `claude plugin marketplace add` + `claude plugin install -y --json` | none of ours: Claude Code writes its own bookkeeping (004) |
| **own file** | Copilot, Grok, Kiro | write a whole file that is ours alone | lowest; nothing to merge |
| **shared file** | Codex, Cursor, Droid, Mistral Vibe, Gemini, Devin (user scope), Claude Code (fallback) | merge our entries into a file the user maintains | the safe write exists for this row |

## Relevant decisions

From [decisions.md](../decisions.md):

- **Plugin route does not fall back on failure.** The routes are chosen by what is available, not by
  what worked. If `claude` is present and plugin commands fail, fail the step and suggest
  `--claude-route=settings`; do not silently merge into `~/.claude/settings.json`.
- **Never repoint an existing marketplace.** Idempotency keys on the plugin being installed
  (`tmux-agent-status@...` in `claude plugin list --json`); do not check where the marketplace points.
- **Repository slug.** Default from `env!("CARGO_PKG_REPOSITORY")`; `--marketplace <source>` overrides.
- **Drop-in contents.** Embedded in the binary with `include_str!`, not read from `share/agents/` at
  runtime.
- **Merge idempotency.** Two levels: marker comments identify the block we manage; our own
  `name = "tmux-agent-status-..."` keys identify the hook set being present, marked or not. If an
  equivalent unmarked set exists, offer to adopt (rewrap in markers) rather than duplicate.
- **Managed and unmanaged paths.** Summary groups edits by resolved destination.
- **User scope.** Every agent gets user-scope config; Devin has no user-scope drop-in, so Devin gets
  `~/.config/devin/config.json`.

## Relevant findings

From [findings.md](../findings.md):

- Droid puts event names at the top level, with no wrapping `hooks` key.
- Devin user-scope config nests the whole hook map under `hooks` in `~/.config/devin/config.json`, and
  one unknown event key discards the entire hook map.
- Mistral Vibe is TOML.
- An appended `[[hooks]]` header is valid TOML after almost anything, but a file whose last line is
  inside an unclosed multi-line string swallows the block into that string.
- `claude plugin` commands are non-interactive and machine-readable; a marketplace can be a
  `directory` source pointing at a local checkout.
- `cargo install` ships only the binary and nothing else.
- Guessing an agent's binary name gives wrong preselections; names are taken from verified installs.

## Implementation

- Build the agent table mapping each agent to its detection signals, target path, merge class, and
  embedded drop-in contents.
- Implement detection: check config directory and `PATH` command; report both signals.
- Implement the three delivery classes:
  - **plugin**: run `claude plugin marketplace list --json`, `claude plugin list --json`,
    `marketplace add`, `plugin install -y --json`; fail loudly on error, never fall back to settings.
  - **own file**: write the embedded contents to the agent's target path.
  - **shared file**: merge into the existing JSON/TOML config.
- JSON merge: identify our entries by command string beginning `tmux-agent-status `; drop matches,
  insert ours, preserve order with `serde_json::preserve_order`, handle Droid top-level events and
  Devin's eight-event constraint.
- TOML append (Mistral Vibe): perform a lexical top-level scan over comments, single-line strings,
  and `"""` / `'''` multi-line strings; refuse to append if not at top level; prepend newline if the
  file does not end in one.
- Two-level idempotency: marker comments and hook keys. Offer "adopt" for unmarked equivalent sets.
- Emit warnings for lines mentioning `tmux-agent-status` that are neither markers nor our keys.

## Verification

Pure, no filesystem:

- **JSON merge**, per merge class: empty file, no `hooks` key, unrelated hooks preserved, our
  entries already present (**byte-identical output**), our entries present but stale (replaced, not
  duplicated), key order preserved, Droid's top-level shape, Devin's eight-key constraint.
- **The TOML top-level scan**, from fixtures: a file ending inside `"""`, inside `'''`, inside a
  comment, inside a single-line string, and cleanly at top level; only the last is appended to.

CLI:

- **The Claude route does not fall back on failure**: with a stub `claude` on `PATH` that exits
  non-zero, the step fails, `~/.claude/settings.json` is untouched, and the message names
  `--claude-route=settings`. With `claude` absent, the same run merges into `settings.json`.

Drift, extending `tests/agent_configs.rs`:

- Every directory under `share/agents/` has a row in the installer's agent table, and every row's
  embedded contents equal the shipped file. A new agent cannot be added without the installer
  learning about it.

By hand, on a real machine:

- A real `install` on a machine with several agents, followed by a real turn producing a real glyph,
  with `git diff` in the dotfiles repo showing exactly the intended edits and nothing else.
- The Claude Code plugin route end to end, against a throwaway `HOME`, confirming that
  `~/.claude/settings.json` still has no `hooks` entry of ours.
