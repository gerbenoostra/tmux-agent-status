//! `tmux-agent-status install`: write the hooks and configs the README documents.
//!
//! Everything under here is the one subcommand that touches a user's files, and
//! only when a human types it. The hook commands are unchanged and still write
//! nothing but two tmux options and a bell. The contract that licences the
//! writing - resolve symlinks, lock, back up, never truncate, verify, restore -
//! is `tasks/plans/011-install-command.md`, and `write` is where it lives.

pub mod format;
pub mod probe;
pub mod tmux_conf;
pub mod write;

use std::path::PathBuf;

/// The comment that opens a block this tool manages.
///
/// Markers are what make the future `uninstall` a deletion rather than a second
/// parse, and they are the same two lines in tmux config and in TOML, because
/// both take `#` comments.
pub const MARKER_START: &str = "# >>> tmux-agent-status >>>";

/// The comment that closes one.
pub const MARKER_END: &str = "# <<< tmux-agent-status <<<";

/// Append a marked block, whatever the file already ends with.
///
/// Two mechanical details that are easy to get wrong, and so are written down:
/// the block is preceded by a newline when the file does not already end in
/// one, or the marker lands on the tail of the user's last line; and the block
/// ends in a newline of its own.
pub fn append_marked(text: &str, body: &str) -> String {
    let separator = match text.is_empty() || text.ends_with('\n') {
        true => "",
        false => "\n",
    };
    let body = body.strip_suffix('\n').unwrap_or(body);
    format!("{text}{separator}{MARKER_START}\n{body}\n{MARKER_END}\n")
}

/// Whether the text already carries a block this tool manages.
pub fn has_marked_block(text: &str) -> bool {
    text.lines().any(|line| line.trim() == MARKER_START)
}

/// Where the user's configuration lives.
///
/// Read from the environment once, at the edge, and passed down: the tests
/// point this at a temp directory, and a tool that cannot be redirected cannot
/// be tested. Never `getpwuid`, for the same reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`, which is not always `$HOME/.config` and is not
    /// always set.
    pub xdg_config: Option<PathBuf>,
}

impl Home {
    pub fn from_env() -> Option<Home> {
        Some(Home {
            home: PathBuf::from(std::env::var_os("HOME")?),
            xdg_config: std::env::var_os("XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        })
    }

    /// A path under `$HOME`.
    pub fn join(&self, tail: &str) -> PathBuf {
        self.home.join(tail)
    }

    /// A path under the config directory, whichever one that is.
    ///
    /// `$XDG_CONFIG_HOME` when it is set, and `$HOME/.config` otherwise, which
    /// is the default the specification gives and the one tmux documents.
    pub fn config(&self, tail: &str) -> PathBuf {
        match &self.xdg_config {
            Some(dir) => dir.join(tail),
            None => self.home.join(".config").join(tail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_appended_with_its_own_newlines() {
        assert_eq!(
            append_marked("set -g status on\n", "source-file x"),
            "set -g status on\n# >>> tmux-agent-status >>>\nsource-file x\n# <<< tmux-agent-status <<<\n"
        );
    }

    #[test]
    fn a_file_not_ending_in_a_newline_gains_one_first() {
        // Otherwise the marker lands on the tail of the user's last line and
        // takes that line's command with it.
        let out = append_marked("set -g status on", "source-file x");
        assert!(out.starts_with("set -g status on\n# >>>"), "{out}");
    }

    #[test]
    fn an_empty_file_gains_no_leading_blank_line() {
        assert!(append_marked("", "x").starts_with(MARKER_START));
    }

    #[test]
    fn a_body_that_already_ends_in_a_newline_does_not_gain_a_second() {
        assert_eq!(
            append_marked("", "one\ntwo\n"),
            append_marked("", "one\ntwo")
        );
    }

    #[test]
    fn a_marked_block_is_recognised_wherever_it_sits() {
        let text = append_marked("set -g status on\n", "source-file x");
        assert!(has_marked_block(&text));
        assert!(has_marked_block("  # >>> tmux-agent-status >>>  \n"));
        assert!(!has_marked_block("set -g status on\n"));
        assert!(!has_marked_block(
            "# a comment mentioning tmux-agent-status\n"
        ));
    }

    #[test]
    fn the_config_directory_follows_xdg_when_it_is_set() {
        let home = Home {
            home: PathBuf::from("/home/u"),
            xdg_config: None,
        };
        assert_eq!(home.join(".tmux.conf"), PathBuf::from("/home/u/.tmux.conf"));
        assert_eq!(
            home.config("tmux/tmux.conf"),
            PathBuf::from("/home/u/.config/tmux/tmux.conf")
        );

        let elsewhere = Home {
            xdg_config: Some(PathBuf::from("/elsewhere")),
            ..home
        };
        assert_eq!(
            elsewhere.config("tmux/tmux.conf"),
            PathBuf::from("/elsewhere/tmux/tmux.conf")
        );
    }
}
