# Step 10: `Cargo.toml` and docs

## Scope

Add the required dependencies and update the documentation so the `install` subcommand is the
recommended setup path while keeping the manual steps documented.

## Relevant decisions

From [decisions.md](../decisions.md):

- **JSON**: `serde_json` with `preserve_order` - a merge that reorders a user's keys is a diff they
  did not ask for.
- **prompts**: `dialoguer` (`MultiSelect`, `Confirm`, `Editor`) - the agent list is genuinely a
  checkbox and the format string genuinely needs an editor; `inquire` is the equivalent alternative
  if `dialoguer`'s `Editor` disappoints.

## Implementation

### `Cargo.toml`

- Enable or add `serde_json` with `preserve_order`.
- Add `dialoguer` (or `inquire` if that path is chosen).

### README

Make `tmux-agent-status install` the lede of the setup section, with the manual four steps kept
below it for transparency.

### `docs/agents/README.md`

Note which agents the installer covers.

### `docs/install.md`

End each install route with the one-liner (`tmux-agent-status install`).

### Plugin `/tmux-agent-status:doctor`

Suggest `install` for each failing check.

## Verification

- Review `Cargo.toml` to confirm `serde_json` has `preserve_order` and the prompt crate is declared.
- Review docs to confirm `install` is documented as the primary setup path and manual steps are still
  reachable.
