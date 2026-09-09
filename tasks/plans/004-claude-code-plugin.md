# 004 - Ship the Claude Code hook set as a plugin

Status: implemented on `feat/claude-code-plugin`; verification steps 3, 4, 6 and 7 still need a real
Claude Code session and a real tmux, because they are the end-to-end path a checkout cannot fake.

Covers *how the Claude Code hook entries reach a user's machine*. What the hooks mean and why those
six events were chosen is 001, which stays normative; this file never restates a behavioural
decision.

## Goal

Replace the manual JSON paste of README step 4 with two commands:

```
/plugin marketplace add gerbenoostra/tmux-agent-status
/plugin install tmux-agent-status
```

Done when all five are true:

1. Installing the plugin makes a real `Stop` put a real ✅ on a real window, with no
   `tmux-agent-status` entry under `hooks` in `~/.claude/settings.json`.
2. Uninstalling it stops that behaviour, and the only residue is Claude Code's own installation
   bookkeeping (see below), which the user removes with `/plugin marketplace remove`.
3. `/tmux-agent-status:doctor` tells a user with a broken setup which of the four setup steps is
   missing.
4. The hook set in the manifest and the hook table in the README cannot drift apart unnoticed.
5. The binary is still installed separately, and its absence is still a silent no-op.

## Naming, settled

The earlier draft of this file used `agent-status` throughout. That is wrong: the repository, the
crate and the executable are all **`tmux-agent-status`** (001, closed decisions). So the open
question about the plugin name collapses - plugin, marketplace, crate and binary all share the one
name, and the slash command is `/tmux-agent-status:doctor`.

`tmux-agent-status@tmux-agent-status` (plugin@marketplace) reads oddly but is correct, and users
never have to type it: `/plugin install tmux-agent-status` is unambiguous as long as no other
marketplace they have added offers the same plugin name.

## Why a plugin does not violate "never write the user's config"

001 forbids this tool merging hook entries into `~/.claude/settings.json`, and explicitly allows the
plugin route. The distinction that matters is **who writes**: the plugin *ships* its hook config
inside the plugin directory, and Claude Code - not us - records the installation.

What Claude Code writes was verified by installing this exact plugin against a throwaway `HOME`:

```json
{
  "extraKnownMarketplaces": { "tmux-agent-status": { "source": { ... } } },
  "enabledPlugins": { "tmux-agent-status@tmux-agent-status": true }
}
```

Both keys land in `~/.claude/settings.json`. `hooks` is untouched. `claude plugin uninstall` empties
`enabledPlugins` and leaves `extraKnownMarketplaces`, which `/plugin marketplace remove` clears.

So the honest promise is **"no hook entries in your settings file"**, not "no trace at all"; the
earlier draft overclaimed and the README must not repeat that. `tmux-agent-status` itself still
writes nothing but two tmux options and a bell. The rule in AGENTS.md ("agent hook entries are
documented, never written") is about **our** code writing **their** file; it stands, and gains one
clarifying clause (see "Work items").

## Scope boundary

| In | Out |
| --- | --- |
| `.claude-plugin/marketplace.json` + a plugin under `plugins/` | a separate plugin repository |
| the six hook entries of the README table | changing which events map to which state (that is 001) |
| a read-only `/tmux-agent-status:doctor` command | any command that writes tmux options, agent config or `~/.tmux.conf` |
| README and `docs/install.md` pointing at the plugin | replacing the manual paste; it stays documented for non-plugin users |
| a drift test between manifest and README | shipping the binary inside the plugin |
| | plugins for other agents (see 005) |

## What the schema actually is

Verified against the installed Claude Code CLI (`claude plugin validate --strict`, `claude plugin
init --with hooks`) and the `claude-plugins-official` marketplace on disk, rather than copied from
memory. The two questions the earlier draft left open are answered:

**1. Hooks go in `hooks/hooks.json`, not inline in `plugin.json`.**
An inline `hooks` object in `plugin.json` is a *recognized* field - `--strict` does not flag it as
unknown - but it is never validated: a bogus event name inline passes, while the same bogus event in
`hooks/hooks.json` is reported as `unknown hook event; entry ignored at runtime`. Every plugin in
the official marketplace that has hooks uses `hooks/hooks.json`, and so does the CLI's own
scaffolder. Inline is at best undocumented and at worst unread; the separate file is the path with
tooling behind it.

**2. The manifest shapes**, both confirmed to pass `--strict`, are given verbatim below. `$schema`
is accepted and is what the scaffolder emits, so both files carry it.

Two further facts worth recording because they shape the work:

- `StopFailure` validates as a known event. So do the other five.
- `matcher` is optional and its absence means "all"; the README's current JSON uses `"matcher": "*"`
  on `PostToolUse` alone, which is inconsistent with its own neighbours. The manifest omits it
  everywhere the table says "all", and the README's table is the contract.
- `claude plugin validate --strict <dir>` validates the marketplace when the directory holds a
  `marketplace.json`, and the plugin (with its hooks and commands) when it holds a `plugin.json`.
  With the plugin in a subdirectory both are reachable by directory path; that is why the layout
  below is the one chosen.
- `claude plugin tag` exists and validates that `plugin.json` and the enclosing marketplace entry
  agree. It is a release convenience, not a dependency of this plan.

## Decisions taken here

| Decision | Choice | Why |
| --- | --- | --- |
| Plugin contents | hooks + one slash command | the hook paste is the error-prone half; the tmux half stays manual by 001, so a command that *diagnoses* it is the most a plugin may honestly do |
| Marketplace host | this repository | one tag ships tool, tmux snippet and plugin together; a second repo would need its own release choreography to stay in step |
| Binary delivery | out of the plugin | the binary comes from nix, cargo or a release asset (`docs/install.md`); a plugin that downloads a binary is a second, unsigned distribution channel |
| Binary resolution | plain `tmux-agent-status` on `PATH` | matches the documented hook lines exactly, so plugin and paste behave identically; a resolution wrapper is a fallback to add only if the thin-`PATH` failure is actually observed |
| Failure mode | unchanged: silent exit 0 | a missing binary must never break a turn; the doctor command is how a user finds out |
| Hook config location | `hooks/hooks.json` | the only location the validator checks; see above |
| Repo layout | plugin under `plugins/tmux-agent-status/` | keeps two agent-specific directories out of a Rust repo root, lets both manifests be validated by directory path, and costs nothing if a second plugin ever appears |
| README duplication | the pasteable JSON block is **removed** and replaced by a link to `hooks/hooks.json` | that file's shape is exactly a `settings.json` fragment, so there is nothing left to keep in sync but the human-readable table |
| Drift check | a Rust integration test | CI already runs `just test` on Linux and macOS and in the nix sandbox; no second language, no new CI step |
| `claude plugin validate` in CI | no - a `just check-plugin` recipe plus the pre-tag checklist | it would mean installing Claude Code in a runner, whose auth and network behaviour there is unverified; the drift test already covers the part that rots |

## Layout to add

```
tmux-agent-status/
├── .claude-plugin/
│   └── marketplace.json                       # this repo as a single-plugin marketplace
├── plugins/
│   └── tmux-agent-status/
│       ├── .claude-plugin/plugin.json         # name, version, metadata - no hooks key
│       ├── hooks/hooks.json                   # the six hook entries
│       └── commands/doctor.md                 # /tmux-agent-status:doctor - read-only setup check
└── tests/plugin_manifest.rs                   # the drift test
```

## The manifests

`.claude-plugin/marketplace.json`:

```json
{
  "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
  "name": "tmux-agent-status",
  "description": "The tmux-agent-status Claude Code plugin: lifecycle hooks that drive the tmux window glyph.",
  "owner": { "name": "Gerben Oostra", "url": "https://github.com/gerbenoostra" },
  "plugins": [
    {
      "name": "tmux-agent-status",
      "description": "Agent lifecycle events as one glyph on the tmux window entry",
      "source": "./plugins/tmux-agent-status",
      "category": "productivity",
      "homepage": "https://github.com/gerbenoostra/tmux-agent-status"
    }
  ]
}
```

`plugins/tmux-agent-status/.claude-plugin/plugin.json`:

```json
{
  "$schema": "https://anthropic.com/claude-code/plugin.schema.json",
  "name": "tmux-agent-status",
  "version": "0.0.1",
  "description": "Agent lifecycle events as one glyph on the tmux window entry",
  "author": { "name": "Gerben Oostra" },
  "homepage": "https://github.com/gerbenoostra/tmux-agent-status",
  "repository": "https://github.com/gerbenoostra/tmux-agent-status",
  "license": "MIT",
  "keywords": ["tmux", "agent", "status", "hook"]
}
```

`version` tracks the crate version; the drift test asserts equality, so a release that forgets the
bump fails CI rather than shipping a lie.

`plugins/tmux-agent-status/hooks/hooks.json` is the README table, verbatim in meaning:

| Event | Matcher | Command |
| --- | --- | --- |
| `UserPromptSubmit` | all | `tmux-agent-status set working` |
| `PostToolUse` | all | `tmux-agent-status set working` |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `tmux-agent-status set waiting` |
| `Notification` | all, **not** narrowed | `tmux-agent-status set waiting` |
| `Stop` | all | `tmux-agent-status set done` |
| `StopFailure` | all | `tmux-agent-status set error` |

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set working" }] }],
    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set working" }] }],
    "PreToolUse": [
      {
        "matcher": "AskUserQuestion|ExitPlanMode",
        "hooks": [{ "type": "command", "command": "tmux-agent-status set waiting" }]
      }
    ],
    "Notification": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set waiting" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set done" }] }],
    "StopFailure": [{ "hooks": [{ "type": "command", "command": "tmux-agent-status set error" }] }]
  }
}
```

`Notification` stays unnarrowed for the reason 001 gives: the idle nag is the event that means
"still blocked, and has been for a while". A plugin that narrows it silently loses that.

The optional top-level `description` key that official plugins put in `hooks.json` is **omitted on
purpose**. Without it the file is byte-for-byte a valid `~/.claude/settings.json` fragment, which is
what lets the README stop duplicating the JSON and link to the file instead. That is the whole
mechanism by which the two cannot drift.

## The doctor command

`/tmux-agent-status:doctor` walks the four setup steps of the README in order and reports which one
is missing. It is **read-only**, and that is a hard constraint, not a style preference:

| Step | Check | Read-only means |
| --- | --- | --- |
| 0 tmux | `$TMUX` set at all | a Claude session outside tmux explains every other symptom at once |
| 1 binary | `tmux-agent-status --version` | reports the version *and the path it ran from*, which is what catches a shadowing dev build |
| 2 tmux snippet | `tmux show-hooks -g` filtered for `tmux-agent-status`; expect both `session-window-changed` and `window-pane-changed` | never `set-hook` |
| 3 format term | show the current `window-status-format` and `window-status-current-format`, and whether `@agent_status` is in **both**, plus the paste-ready term | `show-options` yes, `set-option` never |
| 4 hooks | report that the plugin owns them, and warn if `~/.claude/settings.json` also has manual entries | never edits that file |

Frontmatter, which is where the prohibition is enforced rather than merely stated:

```yaml
---
description: Read-only check of the tmux-agent-status setup - binary, tmux snippet, format term, hooks.
allowed-tools: Bash(tmux-agent-status --version), Bash(command -v tmux-agent-status), Bash(tmux show-hooks:*), Bash(tmux show-options:*), Bash(tmux display-message:*), Read(~/.claude/settings.json)
disable-model-invocation: true
---
```

`disable-model-invocation: true` because a diagnostic should run when a human asks, not when a model
guesses it might help.

Reading the format string for a diagnostic is explicitly allowed: the AGENTS.md rule is about
*writing* it, because a spliced copy written to a window-local option freezes that window's format
forever. The command body still carries the prohibition in full, so the next reader cannot widen it:
no `set-option`, no `setw`, no `set-hook`, nothing that writes.

The duplicate-hooks warning matters: a user who pastes the JSON *and* installs the plugin gets every
event twice. The writes are idempotent, but `done` then rings the bell twice. The doctor says so and
tells them to remove the manual entries, and the README says the same.

## Keeping the manifest and the README honest

The pasteable JSON stops existing in two places: the README's Claude Code section loses its JSON
block and links to `plugins/tmux-agent-status/hooks/hooks.json` instead, telling a manual installer
to copy that file's contents into `~/.claude/settings.json`. What remains duplicated is the
human-readable Event/Matcher/State table, and `tests/plugin_manifest.rs` pins it:

1. Parse `hooks/hooks.json`. For every entry derive `(event, matcher, state)`, where `matcher` is
   `None` when the key is absent and `state` is the last word of the command.
2. Assert every command is exactly `tmux-agent-status set <state>` with `state` one of
   `working`, `waiting`, `done`, `error`. This is what catches a typo'd binary name.
3. Parse the first markdown table after the README's `#### Claude Code` heading into the same
   triples: strip backticks, unescape the `\|` that a table cell needs, and read any cell starting
   with `all` as `None`.
4. Assert the two sets are equal, and that both have six entries.
5. Assert `plugin.json`'s `version` equals `Cargo.toml`'s `package.version`, and that its `name`
   equals the marketplace entry's `name` and that the entry's `source` directory exists.

`serde_json` goes in `[dev-dependencies]`. It is dev-only, so the shipped binary keeps its zero
runtime dependencies; hand-rolling a JSON parser to avoid it would be the more fragile half of this
test. `Cargo.lock` is committed and CI builds `--locked`, so the lock update is part of the change.

The test lives entirely in `tests/`, adding no lines to `src/`, so `just coverage`'s 100% line and
region floor is unaffected. `nix flake check` runs the suite in its sandbox against
`lib.cleanSource ../.`, which includes `README.md` and `plugins/`, so the test runs there too.

Manifest *schema* validity is a separate concern from drift and is not in CI:

```just
# Validate the plugin and marketplace manifests. Needs the `claude` CLI; skips without it.
check-plugin:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v claude >/dev/null 2>&1; then
        echo "claude CLI not found; skipping manifest validation." >&2
        echo "The README/manifest drift check runs in 'just test'." >&2
        exit 0
    fi
    claude plugin validate --strict .
    claude plugin validate --strict plugins/tmux-agent-status
```

`just check` stays `fmt-check lint test`, because its contract is "exactly what CI runs".
`check-plugin` is listed in CONTRIBUTING's development commands and is a required step in the
pre-tag checklist.

## Work items

1. `.claude-plugin/marketplace.json`, `plugins/tmux-agent-status/.claude-plugin/plugin.json`,
   `plugins/tmux-agent-status/hooks/hooks.json` as given above.
2. `plugins/tmux-agent-status/commands/doctor.md`.
3. `tests/plugin_manifest.rs` plus the `serde_json` dev-dependency and the `Cargo.lock` update.
4. `justfile`: the `check-plugin` recipe.
5. `README.md`: step 4 becomes "install the plugin, **or** paste"; the JSON block is replaced by a
   link to `hooks/hooks.json`; the table stays; a note that doing both double-rings the bell.
   The "Compatible agents" section mentions the plugin for Claude Code. The Devin section is
   untouched - it belongs to 005 and has no plugin.
6. `docs/install.md`: the plugin route for setup step 4, stated as covering step 4 only, with the
   binary still installed by one of the five existing routes.
7. `CONTRIBUTING.md`: `just check-plugin` in the development commands, and two lines in the pre-tag
   checklist - bump `plugin.json` alongside `Cargo.toml`, and run `just check-plugin`.
8. `AGENTS.md`: extend the third "rules that are easy to break" bullet so the plugin route is not
   read as a violation. Something to the effect of: the tool never edits `~/.claude/settings.json`;
   the plugin ships its hook config and Claude Code does its own installation bookkeeping there.

## Verification

Manual, on a real tmux, because the whole point is the end-to-end path. Steps 1 and 6 are the ones
that prove the promise.

1. Strip every `tmux-agent-status` entry from `~/.claude/settings.json`. Confirm no glyph appears.
2. `/plugin marketplace add <local checkout>` then `/plugin install tmux-agent-status`, restart the
   session. Confirm `hooks` in `~/.claude/settings.json` is still untouched and only
   `enabledPlugins` and `extraKnownMarketplaces` changed.
3. A turn shows 🤖, a finished turn shows ✅, an `AskUserQuestion` shows 💬, focusing the window
   clears it, and a `StopFailure` shows ❗.
4. `/tmux-agent-status:doctor` on a deliberately broken setup - binary renamed; snippet not sourced;
   format term in only one of the two formats; manual hooks present alongside the plugin - names the
   right failing step each time, and writes nothing (re-read all four setup surfaces after it runs).
5. `just check-plugin` passes, and `just test` fails if a hook entry is edited without the README
   table (check both directions, then revert).
6. `/plugin uninstall`, restart, confirm the glyph stops, `~/.tmux.conf` is untouched, and the only
   residue is the marketplace registration that `/plugin marketplace remove` clears.
7. Repeat 2-3 against the published marketplace (`gerbenoostra/tmux-agent-status`) once tagged.

### What the implementation branch verified

- `claude plugin validate --strict` passes on both manifests (`just check-plugin`).
- Installing this checkout against a throwaway `HOME` reports the six hooks and the one command,
  and writes only `enabledPlugins` and `extraKnownMarketplaces` - `hooks` is untouched. Uninstalling
  empties `enabledPlugins` and leaves the marketplace registration. That is verification 2 and the
  settings-file half of 6.
- `just test` catches all four drift directions: a manifest edited alone, a README table edited
  alone, a typo'd binary name in a hook command, and a `plugin.json` version left behind.
- `just check`, `just coverage` (still 100% line and region), `cargo build --locked --all-targets`
  and `nix build .#tmux-agent-status` are green; the drift test runs inside the nix sandbox too.

Left for a human at a terminal: 3 (the glyphs on a real turn), 4 (the doctor against four broken
setups), the tmux half of 6, and 7 (the published marketplace, after the tag).

## Order of work

1. The three manifests; `just check-plugin`; local install from the checkout; verification 1-3.
2. `commands/doctor.md`; verification 4.
3. The drift test and its dev-dependency; verification 5.
4. README, `docs/install.md`, `CONTRIBUTING.md`, `AGENTS.md`.
5. Verification 6.
6. Tag, then verification 7 against the public marketplace.

Steps 1-2 are the plugin; 3 is what keeps it from rotting; 4-6 are delivery.

## Open

- Whether the plugin should also ship the tmux snippet as a plugin file so `source-file` can point
  into the plugin directory. Rejected for now: that path moves on every plugin update, and 001 wants
  the snippet at a stable path the user chose.
- Whether other agents get plugins at all, or only documented config. See 005; nothing here assumes
  the Claude route generalises. 005 still uses the old `agent-status` name in one snippet and needs
  the same correction when it is picked up.
