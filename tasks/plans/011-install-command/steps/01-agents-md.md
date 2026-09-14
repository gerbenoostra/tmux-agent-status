# Step 1: update [AGENTS.md](../../../../AGENTS.md)

## Scope

Rewrite the always-on rule file so it no longer forbids what the `install` subcommand does, while
keeping the protections that are still in force. This is the prerequisite for every code step.

## Why this comes first

[AGENTS.md](../../../../AGENTS.md) is an always-on rule file that currently forbids what items 2-10 do, so it is updated
and committed *first*, on its own. Any other order asks every future session to read a rule and then
violate it, and makes the review of the code a referendum on the rule instead of on the code.

## Relevant decisions

From [decisions.md](../decisions.md), "What changes about the rules":

### "This tool never edits the user's config files"

001, *Never write to `~/.claude/settings.json` from a tool*, rejected a `setup` command. Its four
objections are now requirements in the safe-write contract rather than refutations:

| 001's objection | Answered by |
| --- | --- |
| the file is frequently a symlink into a dotfiles repo | resolve the chain, edit the target, keep the symlink |
| the agent itself writes it at unpredictable moments | read-precondition re-checked immediately before the rename, an exclusive lock file, and a post-rename verify that refuses to restore over a writer that beat it |
| reserialising reorders keys | `serde_json` with `preserve_order`; a no-op merge must be byte-identical |
| a truncate-in-place write loses the file if it loses the race | never truncate: write a sibling temp file and `rename(2)` over the target |

The sentence 001 ends on - *"a tool that can do that to a config it did not write has no business
writing it"* - stands as the bar. The answer is that the tool may write such a file **only** under the
contract in this plan, and that contract is the deliverable, not the subcommand.

The hook commands (`set`, `reset`, `finish`, `clear-window`, `notify`) are unchanged and still write
nothing but two tmux options and a bell. `install` is the one subcommand that touches a user file,
and only when a human types it.

### "Never write, rewrite or splice `window-status-format`"

This rule keeps its teeth and gains a boundary: this tool still never calls `set-option` on
`window-status-format` or `window-status-current-format`, at any scope, ever. `install --tmux-format`
edits the **text of the user's config file**, which is what the README already asks the user to do by
hand and has none of the freezing behaviour. Reading the option (`show-options`) stays fine; writing
it stays forbidden.

A test asserts the string `window-status-format` never appears as an argument to a `set-option`
call anywhere in `src/`.

### "Agent hook entries are documented, never written"

Superseded. They are documented **and** written, under the same contract. The preference order
(plugin > drop-in file > inline merge) exists precisely so that the riskiest route is the last
resort: on a machine with `claude` on `PATH`, `~/.claude/settings.json` is still never touched by us.

## Implementation

- Rewrite the three rules above in [AGENTS.md](../../../../AGENTS.md).
- Keep the `set-option` half of the format rule intact and explicit.
- Point at this plan file (`tasks/plans/011-install-command/`) where appropriate.
- Commit this on its own, before any code.

## Verification

Manual review that [AGENTS.md](../../../../AGENTS.md):

- no longer forbids the `install` subcommand from editing user config files;
- still forbids calling `set-option` on `window-status-format` / `window-status-current-format`;
- still forbids the hook commands from writing files.
