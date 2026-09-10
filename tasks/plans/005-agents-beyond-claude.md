# 005 - Agents beyond Claude Code

Status: completed - infrastructure, all selected Shape A agents, and two Shape B agents are implemented and verified.

Covers *how a second, third and fourth agent get a glyph*. The states, the rank, the rollup and the
"absent glyph means no signal" principle are 001 and do not change here. 002 deferred this on
purpose: "adding a second agent is what shows whether the shape is right".

Research (committed in `tasks/research/`) surveyed eleven agents and placed them into the three
delivery shapes. Shape C is deferred; this plan now implements the shared infrastructure, all
Shape A agents found, and the two Shape B agents.

007 has since shipped `reset` and `finish`, so the session boundaries this plan argued for are
primitives that already exist. Every agent table below has a row for each of them.

## Goal

Any agent that can push a lifecycle event drives the same four states, through a per-agent mapping
that is **data**, not a code path. Done when all five are true:

1. A capability matrix in the docs says, per agent, which of the four states it can express and why
   the rest are blank.
2. Two agents beyond Claude Code work end to end on a real tmux, and they are of two different
   delivery shapes, not two of the same.
3. No agent's setup requires the user to write shell glue, a JSON parser or a `jq` dependency.
4. An unrecognised event writes nothing and exits 0. A wrong glyph is worse than no glyph.
5. Adding the next agent is a table entry, a fixture, a docs page, and nothing else.

## Three delivery shapes

Every agent surveyed fits one of three. The design is about not growing a fourth.

**A. Hook config with one command per event.** The agent reads a JSON (or TOML) file mapping event
names to commands. Nothing new is needed: the existing CLI *is* the adapter.

```
<event> -> tmux-agent-status set working|waiting|done|error
<session start> -> tmux-agent-status reset
<session end>   -> tmux-agent-status finish
```

This is the most common shape by far, and the differences between agents are shallow: the key
holding the command is not always `command` (one agent uses `bash`), some wrap the map in a
`{"version": 1, ...}` envelope, the matcher vocabulary differs per agent, and the file is sometimes
the agent's main settings file and sometimes a dedicated drop-in (see below).

**B. Single callback with a JSON payload.** One program is invoked for every event and has to work
out from the payload what happened. A new subcommand maps payload to state:

```
tmux-agent-status notify --agent <name> [<payload>]   # payload from argv
tmux-agent-status notify --agent <name> --stdin       # payload from stdin, opted into
```

Decided: this mapping lives **in the binary**, not in a documented `jq` one-liner. A shell snippet
pushes a parser, a dependency and a quoting problem into every user's agent config, and gives us
nothing to test.

Decided: **stdin is read only when `--stdin` is passed.** See "Reading stdin can hang the agent".

**C. In-process plugin or extension.** The agent loads a JS/TS module that subscribes to an event
stream and shells out. More work to ship (a file in the agent's plugin directory, and its API is a
moving target), but it is the only shape that can see the agent's own session state, which is where
`error` and multi-session correctness actually come from.

Shape C is also the first non-Rust code this repository would run, and that is a cost with a name:

- The per-agent docs page carries a **pinned supported version range** of the host agent, and the
  matrix cell says which version the adapter was verified against and on what date.
- CI grows one job that loads the adapter against the **lowest** supported version and asserts the
  events it claims to receive. A smoke test, not a suite.
- Plain JS with no build step, no bundler and no `node_modules` in the shipped artefact, formatted
  and linted by whatever the host agent's plugin convention already uses. TypeScript buys types we
  would then have to compile and ship; the adapter is a mapping table and a `spawn`.
- When a host version falls out of the range, the docs page says so and the adapter keeps working or
  is removed in a release note. It never silently half-works.

If those three cannot be met for the chosen agent, that agent is the wrong first shape C.

## Lessons from prior art

A shipped implementation of the same idea was read end to end. These are the traps it paid for, all
of which apply to a push-based design and none of which are visible from an agent's docs.

**Event streams are not clean.** Observed: repeated "busy" events for a single turn (harmless, our
writes are idempotent), and a **stale trailing "busy" after the turn already went idle**. That last
one replaces ✅ with 🤖 and loses the notification the user was waiting for. A stateless mapper
cannot tell that late `working` from the legitimate `working` of the next turn. Options: keep `set`
dumb and accept the flap for shape A; do the suppression in the shape C adapter, which has session
context; or teach `set` the difference between a turn-start `working` and a mid-turn `working` and
refuse the latter after a sticky state. Recommendation: the first two, and only reach for the third
if a shape A agent turns out to have the same defect.

**A stop event from a subagent is not a stop.** Mapping a subagent's stop to `done` clears the
parent's `working` while the parent is still running. Subagent start *and* subagent stop both map to
`working`. This is a mapping rule, not an implementation detail, and it belongs in every agent's
table that has subagent events.

**One pane can hold several agent sessions.** A parent session plus children, all in one pane, all
emitting independently. Our per-pane option holds one value, so the last event wins and the glyph
flaps.

Decided: **the in-pane rollup is the shape C adapter's job, and the binary does not grow for it.**
The two option names in 001 are per pane and per window, and a third level would mean a third
namespace keyed by the agent's session id, written and garbage-collected by a tool that deliberately
keeps no state. The adapter is a live process that already holds the session ids, so the reduction
is a `max` over a map it owns, in memory, and what reaches `set` is one state per pane. Concretely,
each shape C adapter:

- keeps `session id -> state` for the pane it runs in,
- writes `set <max by 001's rank>` on every change, and never writes a raw per-session state,
- drops a session from the map on that session's end, and writes `finish` only when the map empties.

Shape A cannot do this: it has no process and no session id. Its per-agent page states the
limitation in the matrix rather than papering over it, and the honest wording is that a pane running
nested sessions of a shape A agent shows the most recent event, not the rollup.

**Some agents parse the hook's stdout as JSON.** A hook that prints anything else breaks the agent's
turn. There, the documented line has to swallow output and print an empty object:
`tmux-agent-status set working >/dev/null 2>&1 || true; printf '{}\n'`. Our binary already writes
nothing to stdout for hook commands and rings the bell on `/dev/tty`, which is exactly what makes
this safe, but the stdout contract is a per-agent finding that must be checked, not assumed.

**Reading stdin can hang the agent.** Payloads sometimes arrive on stdin, so `notify` must be able
to read it. If stdin is a terminal, reading blocks forever and takes the turn with it. An
`is_terminal()` guard is necessary and **not sufficient**: a pipe that is never closed, or one that
carries nothing, is not a terminal and still blocks. Detecting "readable without blocking" means
`poll(2)`, which means a `libc` runtime dependency and `unsafe`, for a case the caller already knows
the answer to.

Decided, in this order:

1. `notify` takes the payload from **argv** by default and never touches stdin.
2. `--stdin` is an explicit opt-in, written into the documented hook line only for an agent the
   survey proves pipes its payload.
3. `is_terminal()` stays as a guard *under* `--stdin`: a terminal stdin makes it a no-op that exits
   0 rather than a hang. That is the belt on top of the braces, not the mechanism.
4. Neither present, or an unparseable payload: write nothing, exit 0.

**Hooks can need enabling, not just configuring.** One agent keeps its hook system behind a feature
flag in a different file from the hooks themselves. A docs page that shows only the JSON leaves the
user with a silent no-op and nothing to diagnose. Every per-agent page needs an "enable it" step and
a "prove it fired" step.

**Several agents accept a dedicated drop-in hooks file.** Rather than merging entries into the
agent's hand-maintained settings, the user drops a file that is entirely ours into a hooks
directory. That is the same win 004 gets from the plugin mechanism: nothing of the user's is
touched, the file can be shipped verbatim in this repo, and uninstalling is deleting one file.
Prefer it wherever an agent supports it, and ship the file under `share/agents/<agent>/`.

Shipping a second `share/` tree is not free, and three places hardcode the one file that is there
today: `nix/package.nix`'s `postInstall`, the release workflow's tarball step, and `docs/install.md`.
All three copy `share/tmux/tmux-agent-status.conf` by name. Each has to learn `share/agents/`, or the
Nix user gets the file and the prebuilt-binary and `cargo install` users do not. That is a work item
in step 4, not a footnote.

**Session start and session end are load-bearing.** 007 settled this and shipped both:
`tmux-agent-status reset` on session start drops whatever glyph the previous agent left in the pane,
and `tmux-agent-status finish` on session end resolves a lingering `working` to `done` without a bell
and without overwriting an `error`. For this plan they are two more rows in every mapping table, and
two more columns in the survey. Not every agent has a session end event, and at least one documents
that it never will; those agents keep the known limit that a crashed agent strands 🤖.

**`error` is usually inferred, not published.** The abort case tends to be visible only in the
agent's own session record (the last assistant message's stop reason), which only shape C can read.
Expect the `error` column of the matrix to be near-empty outside Claude Code, and publish it that
way.

**Pane resolution needs tiers.** `$TMUX_PANE` is not always there: hook runners that are not children
of the pane, and agents in a sandbox or container, have none. The prior art resolves in order:
explicit override, then the current pane, then **process ancestry**.

Decided, and the split matters:

1. **An explicit override** on every subcommand: `--pane <id>`, and `TMUX_AGENT_STATUS_PANE` for the
   agent configs that cannot pass an argument. Cheap, testable, no dependency, and it is the tier a
   user can always reach for. Ships in step 2.
2. **`$TMUX_PANE`**, as today.
3. **Process ancestry**, deferred to its own step and gated on evidence *from our own survey*.

Ancestry is deferred because it is not the three-line fallback it reads as. The walk that works is
from our own pid **upwards**, comparing each ancestor against `tmux list-panes -a -F
'#{pane_pid} #{pane_id}'`, because a pane's pid is the shell and the agent is an arbitrary
descendant of it. Getting the chain needs a pid-to-ppid lookup: `/proc` on Linux, and on macOS
`sysctl(KERN_PROC_PID)`, which means a `libc` runtime dependency and the first `unsafe` in a crate
that ships with zero runtime dependencies (`libc` is currently `dev-dependencies` only). It also
adds a process listing and a `list-panes -a` to the three cheap tmux calls 001 budgets for `set`.

So: the survey's "does the hook inherit `TMUX_PANE`?" column decides whether we pay that. The prior
art needing ancestry proves the case exists somewhere, not that it exists in the hook environments
we are about to support. If it does, the walk is depth-bounded (16 is generous), stops at pid 1, and
is covered by a test that fakes the chain rather than spawning one.

What we still refuse is the tier after that, a session-id lookup in a state store, because it needs
the store 001 rules out. Inside a sandbox there is no route to the host's tmux at all, and the honest
answer there is a documented no-op.

**An opt-out is cheap and gets used.** Decided: **`TMUX_AGENT_STATUS_DISABLED=1`** turns every write
into a no-op that exits 0. It costs nothing and saves the user who wants status everywhere except in
CI, a demo recording, or a nested test session. Any non-empty value counts; the documented spelling
is `=1`. It is documented on every per-agent page, not only the README, because the user who needs it
is reading a per-agent page.

**A silent ignore needs a way to see it.** "An unrecognised event writes nothing and exits 0" is
right for the user and wrong for whoever is capturing fixtures or chasing an upstream payload change:
a typo in a mapping table and a breaking change in an agent's payload look identical, which is to say
they look like nothing at all. Decided: **`TMUX_AGENT_STATUS_DEBUG=1`** logs the dropped event and
the reason to **stderr**. Never to stdout, which agents parse, and never by default. Same prefix as
the opt-out, so there is one namespace to document.

## Step 1 is research, and it is most of the work

001's survey (OpenCode richest, then Gemini, then Droid and Amp) is stale: at least two agents have
grown full hook systems since. Redo it, and capture per agent:

| Question | Why it decides something |
| --- | --- |
| Which of the three shapes, and is there more than one route? | one agent has both a hook file and an older single-callback; prefer the richer one |
| Where does the config live, and is there a drop-in file? | a drop-in means we never ask the user to edit their own file |
| Does the hook system need enabling elsewhere? | a missing feature flag is a silent no-op |
| Event vocabulary, with the exact payload of each | becomes the mapping table and the test fixtures |
| Subagent events? Several sessions per pane? | decides whether stop maps to `done` or `working`, and whether the agent needs a shape C adapter to roll up |
| A turn-failed or abort event, or only inferable state? | fills or empties the `error` column |
| A blocked-on-you event, and does it repeat while blocked? | `waiting` is only useful if the idle repeat exists; see 001 on the idle nag |
| Is stdout parsed? | decides whether the documented line needs the `printf '{}'` wrapper |
| Does the hook inherit `TMUX_PANE`? | decides whether that agent needs `--pane`, and whether ancestry is ever needed at all |
| Is the payload on argv or piped to stdin? | decides whether the documented `notify` line carries `--stdin` |
| Session start event? | maps to `reset` (007) |
| Session end event? | maps to `finish` (007), or the agent keeps the stranded-🤖 limit |
| For shape C: which host versions, and how stable is the plugin API? | becomes the pinned range and the smoke test |

Output of step 1 is the capability matrix, committed, with a dated "surveyed on" line. Blank cells
carry a link to the upstream issue or doc that says the event does not exist.

The pair to implement is chosen by criteria, not by a ranking: **one shape A agent that supports a
drop-in file, and one shape C agent**, because that pair exercises everything above. Shape B gets
implemented when the survey finds an agent that offers nothing else. Candidates to survey, in no
particular order: OpenCode, Codex, Gemini CLI, Copilot CLI, Grok, Amp, Droid, Kiro, Mistral Vibe,
Antigravity, Cursor. Whichever two first satisfy the criteria win; the list is a starting set for the
survey, not a queue.

Research results of OpenCode, Codex CLI, Gemini CLI, Copilot CLI, Droid, and Cursor are in (../research/agent-hook-systems-01.md).
Research results of Grok CLI,  Amp,  Kiro, Mistral Vibe, and Antigravity are in (../research/agent-hook-systems-02.md).

## Agents that publish nothing

They get no glyph and are listed in the matrix as unsupported, with the link that proves it.
No wrapper command, no polling of `pane_current_command`, no inference from the pane title. Both
were considered and rejected: a wrapper only ever knows two states and takes over the user's launch
command, and polling is inference rather than events, which 001's design and the "never spawns a
daemon" rule rule out. An absent glyph already has a defined meaning.

## Documentation

- `docs/agents/<agent>.md` per supported agent: the mapping table, the copy-paste config or the
  drop-in file to place, the enable step, the "prove it fired" step, the opt-out and debug
  variables, and the agent's quirks. Same structure every time, so a reader who set up one can skim
  the next.
- Shipped drop-in files live under `share/agents/<agent>/`, next to
  `share/tmux/tmux-agent-status.conf`, and the docs page says where to copy them for each of the
  three install routes (Nix, prebuilt tarball, `cargo install`).
- README keeps Claude Code inline as the reference agent, and links the matrix plus the per-agent
  pages.
- Every page repeats that the config is placed by the user. The tool writes no agent config, ever,
  for any agent (AGENTS.md).
- 004's plugin route is Claude-only unless an agent has an equivalent that installs without editing
  a hand-maintained file. A drop-in hooks file counts; a merge into the main settings does not.

## Testing

- Table-driven mapping tests, one case per `(agent, event)` pair, asserting the state or asserting
  "no write". Same style as `tests/rollup.rs`.
- Fixtures under `tests/fixtures/<agent>/<event>.json`, captured from a real run of that agent and
  never hand-written. A hand-written fixture tests our idea of the payload, which is exactly the
  thing that is wrong.
- `notify` without `--stdin` never reads stdin, and `notify --stdin` with a terminal stdin returns
  immediately. The failure mode is a hung agent and nobody finds that twice.
- `--pane` and `TMUX_AGENT_STATUS_PANE` beat `$TMUX_PANE`, and a bad pane id still exits 0.
- `TMUX_AGENT_STATUS_DISABLED=1` makes every subcommand a no-op that runs no tmux command at all,
  asserted the way `tests/tmux_failure.rs` already asserts the no-tmux path.
- Any shipped drop-in file is parsed in a test and its commands checked against the mapping table,
  so the file and the table cannot drift. Same shape as `tests/plugin_manifest.rs`.
- Every shipped `share/agents/` file appears in the release tarball and in the Nix output, asserted
  in CI, because a drop-in nobody receives is worse than none.
- For the shape C adapter: a smoke test in CI that loads it against the lowest pinned host version.
- The tmux integration tier is unchanged and stays agent-agnostic: it already proves that a state
  becomes a glyph.
- End to end, per agent, once: drive the real agent in a real tmux window and watch every state it
  claims to support actually appear, including a turn that ends in an abort, plus `reset` on a fresh
  session and `finish` on exit.

## Order of work

1. The survey and the committed capability matrix. No code.
2. Pane resolution tier 1: `--pane` and `TMUX_AGENT_STATUS_PANE` on every subcommand. With tests.
3. `TMUX_AGENT_STATUS_DISABLED` and `TMUX_AGENT_STATUS_DEBUG`. Small, and they unblock CI, demo
   recordings and the fixture capture in step 4.
4. Shape A agents. Each gets a mapping table, a shipped drop-in file under `share/agents/<agent>/`
   when the agent supports one, the packaging changes that deliver it, and a `docs/agents/<agent>.md`
   page. No new subcommand is needed; the existing `set`/`reset`/`finish` commands are the adapter.
   Agents: Codex CLI, GitHub Copilot CLI, Droid, Cursor, Grok CLI, Kiro.
5. Shape B agents. Add the `notify --agent <name>` subcommand with JSON payload mapping, plus the
   per-agent docs and fixtures. Agents: Gemini CLI (manual settings.json merge), Mistral Vibe.
6. Re-read the two tables against each other; if they needed different shapes, fix the shape before
   agent three.
7. Process ancestry, only if step 1 found an agent whose hooks do not inherit `TMUX_PANE` and that
   step 2's override cannot serve. Its own commit, with the `libc` dependency argued in the message.
8. `notify` plus the JSON dependency decision, when the survey shows an agent that needs shape B.
9. Agents beyond that, one commit each.

## Open

- **The JSON dependency.** `notify` needs a parser. 002 shipped zero dependencies and named the
  config file as the first honest reason for one; this is now the second. Use `serde_json` in
  dependencies, not a hand-rolled top-level string extractor, which looks cheap and then meets
  escapes, nesting and duplicate keys. Implemented in step 5.
- **`--agent <name>` on one subcommand, or a subcommand per agent.** Leaning to `--agent`: the agent
  name is data, and a per-agent subcommand list is a second place to forget to update.
- **User-defined agents via the config file.** The config file deferred by 002 could carry
  `[agents.<name>]` event tables, making a new agent a user edit rather than a release. Attractive,
  and deliberately not in this plan: get two agents right in code first, then see whether the table
  shape is stable enough to expose.
- **Agent identity in the glyph.** Someone will ask for 🤖 to say *which* agent. Out: the glyph is
  the state, the rollup would need a second dimension, and 001's icon set is one glyph per window.
- **Whether a late `working` should be suppressed in `set` itself.** Only if a shape A agent is
  found with the stale-trailing-event defect. Doing it needs `set` to know which event it came from,
  which widens the CLI surface, so it waits for evidence.
- **Whether the in-pane rollup ever belongs in the binary.** Decided above as no, on the grounds
  that it needs per-session state. If two shape C adapters end up writing the same reduction, that
  is the evidence to revisit it, and the answer would be a shared library rather than a third tmux
  option.
