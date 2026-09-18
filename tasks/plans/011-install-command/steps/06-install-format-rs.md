# Step 6: `src/install/format.rs`

## Scope

The tmux format line parser, the splice, and the requoting.

## Relevant design

The parser is intentionally narrow and bails to a manual path when unsure. A general tmux config
parser is a project; a parser that knows when to stop is a feature.

## Relevant decisions

From [decisions.md](../decisions.md):

### Format term

The one term, unchanged from 001 and the README:

```tmux
#{?@agent_status, #{@agent_status},}
```

### Idempotency

If the value already references `@agent_status`, leave it exactly as it is. A reference is a token
match (`@agent_status` not followed by `[A-Za-z0-9_]`), not a substring match: a user's unrelated
`#{@agent_status_colour}` must not read as ours. Matching the full literal term instead would miss
anyone who dropped the space or wrapped the term in styling, and then install a second copy.

### Which line to edit

tmux executes config commands strictly in the order it encounters them, including inside
`source-file`; the last assignment wins. So the search reconstructs tmux's own command order: walk
the config from the top, descend into each `source-file` at the point it appears, depth-limited with
a visited set against a cycle, and collect every matching line in that order. The last one is the
one to edit. Editing an earlier one produces a line tmux discards.

### Quoting

The term is inserted before the first `#{?window_flags` or at the end of the value. A bare value must
be requoted, always: the term contains a space, and a bare value ends at the first space. Single
quotes are always safe because the term contains none; a value that already contains a single quote
is re-emitted double-quoted, and a value that defeats both falls to the manual path.

### No existing line

When there is no existing format line, read tmux's compiled-in default by starting a throwaway
server with `-f /dev/null` and asking `show-options -gwv`. That default goes through the same splice
and the step writes a new pair of lines - both options, because a term in only one of them makes the
glyph vanish when the window becomes current.

### Manual fallback

If the parser cannot requote safely or the user declines the edit, the step does not fail and nothing
is written. The tool prints the term, the file and line, and the proposed line where it got far enough
to build one. The summary marks the format step as not installed, with instructions, and the run
exits 0.

### Reloading

Never `set-option`. After a successful format or hook step, offer to run `tmux source-file <config>`
if a server is running, confirmed like everything else. Agents still need their own restart.

## Relevant findings

From [findings.md](../findings.md):

- tmux executes config commands strictly in the order it encounters them, including inside
  `source-file`; the last assignment of an option wins.
- tmux expands and sorts globs for `source-file` in a deterministic way.
- The compiled-in default format on tmux 3.6a is
  `#I:#W#{?window_flags,#{window_flags}, }`.
- A bare value containing a space is discarded by tmux and the option keeps its default.
- A `;` met outside any quote separates commands; a `;` inside single or double quotes is part of the
  value.

## Implementation

- Implement a tmux config tokenizer that handles single/double/bare values, trailing-backslash
  continuations, and `;` outside quotes as command separators.
- Implement the line walker that reconstructs tmux's command order, descends into `source-file`,
  expands/sorts globs, and depth-limits with a visited set.
- Implement the splice: insert the term immediately before the first `#{?window_flags`, or at the
  end of the value if absent.
- Re-emit in the original quoting style; bare values must be requoted (prefer single quotes, fall back
  to double, then manual fallback).
- If no format line exists, start a throwaway tmux server with `-f /dev/null` to read the
  compiled-in default, then splice and write a new marked pair covering both options.
- Implement the confirmation: show current and proposed values for both options; offer accept,
  edit in `$EDITOR` (long values) or inline (short values), or skip. Re-check for the term before
  writing.
- Implement the manual fallback: print the term, file, line, and proposed line, report not installed
  in the summary, exit 0.

## Verification

Pure, no filesystem:

1. **Format line parser** against a corpus of real lines: bare, single-quoted, double-quoted,
   `setw`, `set -gw`, backslash continuation, `;`-joined (refused), a value containing `#(...)` with
   embedded double quotes and nested `#{}` (the shape a real config has), a value already carrying
   `@agent_status` (untouched), no line at all.
2. **Splice**: before `#{?window_flags`, at the end when absent, requoting each way, and the
   round-trip property *parse then emit with no change is byte-identical*.
5. **Config-order resolution**: a main file that sets the format then sources a fragment that sets
   it, and the reverse; the last assignment in tmux's own order is the one selected.
7. **Tokenizer**: a `;` inside single quotes is part of the value, a `;` outside separates commands
   and refuses the line.

Real tmux, extending `tests/tmux_server.rs`:

29. Write a temp config with a known format, `install --tmux-format --tmux-hook -y
    --tmux-config <path>`, then start `tmux -L <name> -f <path>` and assert: the term is in both
    options and a hand-set `@agent_status` renders in the window entry.
30. The same against a config with **no** format line, proving the default probe produces a working
    pair of lines.
31. The `set-option`-never test: `window-status-format` never appears as a `set-option` argument
    anywhere in `src/`.

By hand, on a real machine:

- A home-manager machine, the store-resident file chain: the file is refused and prints something the
  user can paste into their generator.
