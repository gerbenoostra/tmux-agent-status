//! `tmux-agent-status install`: write the hooks and configs the README documents.
//!
//! Everything under here is the one subcommand that touches a user's files, and
//! only when a human types it. The hook commands are unchanged and still write
//! nothing but two tmux options and a bell. The contract that licences the
//! writing - resolve symlinks, lock, back up, never truncate, verify, restore -
//! is `tasks/plans/011-install-command.md`, and `write` is where it lives.

pub mod format;
pub mod write;
