//! Finding the tmux config, finding the shipped snippet, and walking the one
//! for the other.
//!
//! The walk is the part worth reading. tmux executes config commands strictly
//! in the order it meets them, `source-file` included, and the last assignment
//! wins - so the file that owns `window-status-format` is decided by position,
//! not by depth. Verified on 3.6a: a main file that sets the format and then
//! sources a fragment that sets it ends up with the fragment's value, and
//! reversing the two reverses the winner. Editing anything but the last one
//! produces a line tmux discards, which is the worst outcome available: a
//! successful-looking install with no glyph and nothing to see in the diff.

use std::fs;
use std::path::{Path, PathBuf};

use super::format::{self, Candidate, LogicalLine};
use super::{Home, append_marked};

/// The basename that means "our snippet", wherever the user put it.
pub const SNIPPET_NAME: &str = "tmux-agent-status.conf";

/// The shipped snippet, embedded rather than looked up at runtime.
///
/// `cargo install` ships the binary and nothing else, so a runtime path lookup
/// would leave the largest install route unable to install anything.
pub const SNIPPET: &str = include_str!("../../share/tmux/tmux-agent-status.conf");

/// How deep a chain of `source-file` is followed before giving up.
///
/// A cycle is caught by the visited set; this catches a chain that is merely
/// silly, and keeps the walk's cost bounded on any config at all.
const MAX_DEPTH: usize = 16;

/// A file to edit, and whether it is there yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// An existing file.
    Existing(PathBuf),
    /// Nothing exists, so this one is offered for creation.
    Create(PathBuf),
}

impl Choice {
    pub fn path(&self) -> &Path {
        match self {
            Choice::Existing(path) | Choice::Create(path) => path,
        }
    }
}

/// Which config file the source-file line goes in.
///
/// "Prefer the user one" is not a good enough specification, so this is the
/// order, and `/etc/tmux.conf` is never in it: it needs root and it installs
/// the tool for every user of the machine, which is not what anyone typing this
/// command meant. It is reported instead, with the suggestion to pass
/// `--tmux-config` if that really was the intent.
pub fn discover_config(explicit: Option<&Path>, home: &Home) -> Choice {
    if let Some(path) = explicit {
        return match path.exists() {
            true => Choice::Existing(path.to_path_buf()),
            false => Choice::Create(path.to_path_buf()),
        };
    }
    let xdg = home.config("tmux/tmux.conf");
    let fallback = home.join(".config/tmux/tmux.conf");
    let mut candidates = vec![xdg.clone(), home.join(".tmux.conf")];
    if fallback != candidates[0] {
        candidates.insert(1, fallback);
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Choice::Existing(candidate);
        }
    }
    // The location tmux documents, and the one that does not clutter $HOME.
    Choice::Create(xdg)
}

/// What tmux says it would load: candidates, not a decision.
///
/// Verified on 3.6a to return `/etc/tmux.conf,~/.tmux.conf,~/.config/tmux/tmux.conf`,
/// naming files that do not exist; and when the server was started with `-f`,
/// naming only that one. So it is worth reporting and is never the answer.
pub fn candidates(reported: &str) -> Vec<PathBuf> {
    reported
        .trim()
        .split(',')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Whether a path is the system-wide config, which is never chosen.
pub fn is_system_wide(path: &Path) -> bool {
    path == Path::new("/etc/tmux.conf")
}

/// Where the snippet to source lives, or where to put a copy of it.
///
/// In order, first hit wins: an explicit path, then the layout around the
/// running executable, then the usual prefixes, then a copy of the embedded
/// text. The executable's own neighbourhood covers the nix profile, the release
/// tarball and a binary run straight out of `target/release`; a dev-loop
/// `~/.local/bin` shadow pointing into a build directory finds nothing at
/// `../share` and must fall through rather than fail.
pub fn discover_snippet(
    explicit: Option<&Path>,
    exe: Option<&Path>,
    config: &Path,
    home: &Home,
    prefix: Option<&Path>,
) -> Choice {
    if let Some(path) = explicit {
        return match path.is_file() {
            true => Choice::Existing(path.to_path_buf()),
            false => Choice::Create(path.to_path_buf()),
        };
    }
    for candidate in search_path(exe, home, prefix) {
        if candidate.is_file() {
            return Choice::Existing(candidate);
        }
    }
    // Nothing found, which is what `cargo install` leaves behind: write the
    // embedded copy beside the config that will source it.
    Choice::Create(beside(config, home))
}

/// Everywhere a shipped snippet might already be.
fn search_path(exe: Option<&Path>, home: &Home, prefix: Option<&Path>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(exe) = exe.and_then(|exe| exe.canonicalize().ok()) {
        // Two levels above the binary is `<prefix>/share`, which covers the nix
        // profile and the release tarball; three covers a binary run straight
        // out of `target/release` in a checkout. A path too short to have them
        // simply contributes fewer roots.
        roots.extend(
            exe.ancestors()
                .skip(2)
                .take(2)
                .map(|prefix| prefix.join("share")),
        );
    }
    // `$PREFIX` is read at the edge and passed in, like `$HOME`: a search path
    // that reaches for the environment itself is one a test cannot steer.
    roots.extend(prefix.map(|prefix| prefix.join("share")));
    roots.push(home.join(".nix-profile/share"));
    roots.push(PathBuf::from("/usr/local/share"));
    roots.push(PathBuf::from("/opt/homebrew/share"));
    roots
        .into_iter()
        .map(|root| root.join("tmux").join(SNIPPET_NAME))
        .collect()
}

/// Where to write our own copy: beside the config that sources it.
fn beside(config: &Path, home: &Home) -> PathBuf {
    match config == home.join(".tmux.conf") {
        // The `~/.tmux.conf` layout keeps its fragments in `~/.tmux/`.
        true => home.join(".tmux").join(SNIPPET_NAME),
        false => config
            .parent()
            .map_or_else(|| home.config("tmux"), Path::to_path_buf)
            .join(SNIPPET_NAME),
    }
}

/// Whether this config already sources our snippet.
///
/// Any `source` or `source-file` whose last argument has our basename, at any
/// path: the location is the user's business, and a second source line is a
/// second set of hooks.
pub fn sources_snippet(text: &str) -> bool {
    format::logical_lines(text)
        .iter()
        .filter_map(|line| source_argument(&line.text))
        .any(|path| Path::new(&path).file_name() == Some(SNIPPET_NAME.as_ref()))
}

/// The path a `source`/`source-file` line names, if it is one.
fn source_argument(line: &str) -> Option<String> {
    match format::words(line)?.as_slice() {
        [command, rest @ ..] if matches!(command.as_str(), "source" | "source-file") => {
            // tmux's own flags here take no values, so the path is simply the
            // last word that is not one.
            rest.iter()
                .rev()
                .find(|word| !word.starts_with('-'))
                .cloned()
        }
        _ => None,
    }
}

/// The block that sources the snippet.
///
/// Appended at the end: the snippet sets hooks only, and a hook set late is a
/// hook set, so position does not matter here the way it does for the format.
/// `None` when the snippet's path cannot be spelled safely, which sends the
/// step to the manual path like any other line this module will not write.
pub fn with_source_block(text: &str, snippet: &Path) -> Option<String> {
    Some(append_marked(
        text,
        &format!("source-file {}", quote(snippet)?),
    ))
}

/// A path as a tmux config line should spell it.
///
/// Bare when nothing in it needs quoting, which is the spelling the README
/// asks the user to paste. Otherwise the choice `format::requote` makes for the
/// same reasons: single quotes first, because nothing is an escape inside them
/// and a path is full of things that would otherwise be one; double quotes for
/// a path that holds a single quote; and `None` for one that defeats both.
///
/// The bare case is not a formatting preference. A path with a space in it is
/// two words to tmux, and a stray second argument makes tmux abandon the whole
/// config file - so is an unterminated quote, which is what a bare path holding
/// a `'` is. Either costs the user their entire configuration rather than just
/// our glyph.
///
/// A newline is the same hazard and the easiest to forget, because it is the
/// one that does not look like a character: bare, it ends the `source-file`
/// command and leaves the rest of the path as a line of its own, which tmux
/// reads as an unknown command and answers by throwing the file away. Verified
/// on 3.6a, both halves: bare, a config that set `status-left` lost it; inside
/// single quotes, the same path sourced its fragment and `status-left` stood.
fn quote(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    if !text.contains([' ', '\t', '\n', '\r', ';', '#', '\'', '"', '\\', '$', '`']) {
        return Some(text.into_owned());
    }
    if !text.contains('\'') {
        return Some(format!("'{text}'"));
    }
    // Inside double quotes tmux resolves escapes and expands `$`, so a path
    // carrying any of those cannot go in them either.
    if !text.contains(['"', '$', '`', '\\']) {
        return Some(format!("\"{text}\""));
    }
    None
}

/// Replace a run of physical lines with one line.
///
/// A logical line may span several physical ones through trailing backslashes,
/// and the rewrite collapses the run - a formatting change the confirmation
/// discloses, and the only one an edit makes outside the value itself. The
/// presence or absence of a trailing newline is preserved.
pub fn replace_lines(text: &str, first: usize, last: usize, with: &str) -> String {
    let mut out = String::with_capacity(text.len() + with.len());
    for (index, line) in text.lines().enumerate() {
        if index < first || index > last {
            out.push_str(line);
            out.push('\n');
        } else if index == first {
            out.push_str(with);
            out.push('\n');
        }
    }
    if !text.ends_with('\n') && !out.is_empty() {
        out.pop();
    }
    out
}

/// One assignment of a format option, and where it lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub file: PathBuf,
    pub line: LogicalLine,
    pub candidate: Candidate,
}

impl Assignment {
    /// Which of the two options this line assigns.
    ///
    /// A line that is being handed back still assigns its option, which is what
    /// stops the caller appending a second assignment beside it.
    pub fn option(&self) -> Option<&str> {
        self.candidate.option()
    }
}

/// What walking a config found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Walked {
    /// Every format assignment tmux would execute, in the order it executes
    /// them. The last one for a given option is the one that wins, and so the
    /// one to edit.
    pub assignments: Vec<Assignment>,
    /// The relative `source-file` arguments met on the way.
    ///
    /// tmux resolves these against the working directory of whatever started
    /// the server, which is not knowable from here. They are followed from
    /// `$HOME`, which is where the probe puts its own cwd so that the walk and
    /// the check that marks its homework agree - and they are reported,
    /// because a server started from somewhere else read different files and
    /// may have a different winner.
    pub relative_sources: Vec<String>,
}

/// Walk a config the way tmux executes it.
///
/// Globs are expanded and sorted; a cycle is caught by the visited set.
pub fn walk(entry: &Path) -> Walked {
    let mut found = Walked::default();
    let mut visited = Vec::new();
    descend(entry, 0, &mut visited, &mut found);
    found
}

fn descend(file: &Path, depth: usize, visited: &mut Vec<PathBuf>, found: &mut Walked) {
    if depth > MAX_DEPTH {
        return;
    }
    let resolved = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if visited.contains(&resolved) {
        return;
    }
    visited.push(resolved.clone());
    let Ok(text) = fs::read_to_string(&resolved) else {
        return;
    };
    for line in format::logical_lines(&text) {
        // Descend at the point the `source-file` appears, because that is when
        // tmux runs it, and a fragment sourced early loses to a line below it.
        if let Some(argument) = source_argument(&line.text) {
            if is_relative(&argument) && !found.relative_sources.contains(&argument) {
                found.relative_sources.push(argument.clone());
            }
            for sourced in expand(&argument) {
                descend(&sourced, depth + 1, visited, found);
            }
            continue;
        }
        let candidate = format::parse(&line.text);
        if candidate != Candidate::NotOurs {
            found.assignments.push(Assignment {
                file: resolved.clone(),
                line,
                candidate,
            });
        }
    }
}

/// Whether a `source-file` argument is one tmux has to resolve against a
/// working directory.
fn is_relative(argument: &str) -> bool {
    !argument.starts_with('/') && !argument.starts_with("~/")
}

/// The files a `source-file` argument names, sorted.
///
/// `~` is expanded and a trailing-component glob is expanded by reading the
/// directory and sorting, the way tmux sorts it.
///
/// A relative path is resolved against `$HOME`. Verified: tmux resolves it
/// against the working directory of the process that started the server, not
/// against the config file's own directory, so the config's directory would be
/// wrong for every layout but `~/.tmux.conf`. `$HOME` is both the common case
/// for a server started from a login shell and the cwd the probe runs with, so
/// the walk and the check that marks its homework read the same files. The
/// guess is reported either way; see `Walked::relative_sources`.
fn expand(argument: &str) -> Vec<PathBuf> {
    let path = match std::env::var_os("HOME") {
        // `join` with an absolute path discards the base, so an absolute
        // argument needs no arm of its own: `~/x`, `x` and `/x` are all this
        // one line.
        Some(home) => PathBuf::from(home).join(argument.strip_prefix("~/").unwrap_or(argument)),
        // A process with no `$HOME` has nothing to resolve against, so the
        // path stands as the config wrote it.
        None => PathBuf::from(argument),
    };
    let Some(pattern) = glob_pattern(&path) else {
        return vec![path];
    };
    let Ok(entries) = fs::read_dir(path.parent().unwrap_or(Path::new("."))) else {
        return Vec::new();
    };
    let mut matches: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| matches_glob(&pattern, &entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect();
    matches.sort();
    matches
}

/// A path's last component, when it carries a wildcard.
///
/// Only the last component: a glob in the middle of a path is not something
/// people write in a tmux config, and guessing at one is how a walk starts
/// reading directories nobody asked about.
fn glob_pattern(path: &Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.contains(['*', '?']))
}

/// `*` and `?`, which is the whole of what a config file uses.
fn matches_glob(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    // The usual two-cursor walk with one backtrack point, which is linear and
    // needs no allocation per step.
    let (mut p, mut n) = (0, 0);
    let (mut star, mut resume) = (None, 0);
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some(p);
                resume = n;
                p += 1;
            }
            Some('?') => {
                p += 1;
                n += 1;
            }
            Some(c) if *c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some(at) => {
                    p = at + 1;
                    resume += 1;
                    n = resume;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_at(path: &str) -> Home {
        Home {
            home: PathBuf::from(path),
            xdg_config: None,
        }
    }

    #[test]
    fn the_shipped_snippet_is_embedded_and_sets_the_hooks() {
        assert!(SNIPPET.contains("set-hook -g 'session-window-changed[50]'"));
        assert!(SNIPPET.contains("tmux-agent-status clear-window"));
    }

    #[test]
    fn an_explicit_config_overrides_everything_below_it() {
        let home = home_at("/home/u");
        let explicit = PathBuf::from("/nowhere/at/all.conf");
        assert_eq!(
            discover_config(Some(&explicit), &home),
            Choice::Create(explicit)
        );
    }

    #[test]
    fn nothing_existing_offers_the_documented_location() {
        let home = home_at("/nonexistent-home");
        assert_eq!(
            discover_config(None, &home),
            Choice::Create(PathBuf::from("/nonexistent-home/.config/tmux/tmux.conf"))
        );
    }

    #[test]
    fn the_system_config_is_named_but_never_chosen() {
        assert!(is_system_wide(Path::new("/etc/tmux.conf")));
        assert!(!is_system_wide(Path::new("/home/u/.tmux.conf")));
    }

    #[test]
    fn tmuxs_config_list_is_read_as_candidates() {
        assert_eq!(
            candidates("/etc/tmux.conf,~/.tmux.conf,~/.config/tmux/tmux.conf\n"),
            vec![
                PathBuf::from("/etc/tmux.conf"),
                PathBuf::from("~/.tmux.conf"),
                PathBuf::from("~/.config/tmux/tmux.conf"),
            ]
        );
        // A server started with `-f` names only that file.
        assert_eq!(
            candidates("/tmp/one.conf"),
            vec![PathBuf::from("/tmp/one.conf")]
        );
        assert!(candidates("").is_empty());
        assert!(candidates("  ,  \n").is_empty());
    }

    #[test]
    fn a_source_line_is_recognised_by_the_basename_wherever_it_points() {
        for line in [
            "source-file ~/.tmux/tmux-agent-status.conf",
            "source ~/.config/tmux/tmux-agent-status.conf",
            "source-file -q '/opt/odd path/tmux-agent-status.conf'",
            "source-file \"/x/tmux-agent-status.conf\"",
        ] {
            assert!(sources_snippet(line), "line {line:?}");
        }
    }

    #[test]
    fn a_line_that_sources_something_else_is_not_ours() {
        for line in [
            "source-file ~/.tmux/other.conf",
            "set -g status on",
            "# source-file ~/.tmux/tmux-agent-status.conf",
            "run-shell tmux-agent-status.conf",
            "source-file",
            "source-file -q",
        ] {
            assert!(!sources_snippet(line), "line {line:?}");
        }
    }

    #[test]
    fn the_source_block_is_marked_and_appended() {
        let out = with_source_block(
            "set -g status on\n",
            Path::new("/home/u/.config/tmux/tmux-agent-status.conf"),
        )
        .expect("an ordinary path can be spelled");
        // Bare, which is the spelling the README asks the user to paste.
        assert!(out.contains("source-file /home/u/.config/tmux/tmux-agent-status.conf"));
        assert!(sources_snippet(&out));
        assert!(super::super::has_marked_block(&out));
    }

    #[test]
    fn a_path_tmux_would_read_as_two_words_is_quoted() {
        // A space makes it two arguments, and a stray second argument makes
        // tmux abandon the whole config file.
        let spaced = quote(Path::new("/odd path/tmux-agent-status.conf")).expect("single quotes");
        assert_eq!(spaced, "'/odd path/tmux-agent-status.conf'");

        // A bare `'` opens a quoted section that never closes, which costs the
        // same config file whether or not there is also a space.
        let quoted = quote(Path::new("/it's odd/tmux-agent-status.conf")).expect("double quotes");
        assert_eq!(quoted, "\"/it's odd/tmux-agent-status.conf\"");
        assert_eq!(
            quote(Path::new("/it's/tmux-agent-status.conf")),
            Some("\"/it's/tmux-agent-status.conf\"".to_owned())
        );

        // A newline is the same hazard and the easiest to forget: bare, it
        // ends the `source-file` command and the rest of the path becomes a
        // line tmux reads as an unknown command, which costs the whole file.
        // Verified on 3.6a that single quotes carry one.
        assert_eq!(
            quote(Path::new("/we\nird/tmux-agent-status.conf")),
            Some("'/we\nird/tmux-agent-status.conf'".to_owned())
        );

        // So do the rest of what tmux gives a meaning to.
        for odd in [
            "/a;b/x.conf",
            "/a#b/x.conf",
            "/a\\b/x.conf",
            "/a$b/x.conf",
            "/a\rb/x.conf",
        ] {
            let spelled = quote(Path::new(odd)).expect("single quotes");
            assert_eq!(spelled, format!("'{odd}'"), "{odd}");
        }

        // And a path that defeats both quotings is handed back rather than
        // written: inside double quotes tmux resolves escapes and expands `$`.
        for hopeless in [
            "/it's $HOME/x.conf",
            "/it's \"quoted\"/x.conf",
            "/it's `x`/x.conf",
        ] {
            assert_eq!(quote(Path::new(hopeless)), None, "{hopeless}");
            assert_eq!(
                with_source_block("", Path::new(hopeless)),
                None,
                "{hopeless}"
            );
        }
    }

    #[test]
    fn the_snippet_goes_beside_the_config_that_sources_it() {
        let home = home_at("/home/u");
        assert_eq!(
            beside(&home.join(".tmux.conf"), &home),
            PathBuf::from("/home/u/.tmux/tmux-agent-status.conf")
        );
        assert_eq!(
            beside(&home.config("tmux/tmux.conf"), &home),
            PathBuf::from("/home/u/.config/tmux/tmux-agent-status.conf")
        );
        // A config with no parent at all falls back to the documented
        // location rather than to the root directory.
        assert_eq!(
            beside(Path::new("/"), &home),
            PathBuf::from("/home/u/.config/tmux/tmux-agent-status.conf")
        );
    }

    #[test]
    fn the_search_path_covers_the_layouts_the_binary_ships_in() {
        let home = home_at("/home/u");
        let exe = std::env::current_exe().expect("the test binary has a path");
        let found = search_path(Some(&exe), &home, Some(Path::new("/opt/prefix")));
        assert!(
            found
                .iter()
                .any(|p| p.ends_with("share/tmux/tmux-agent-status.conf")),
            "{found:?}"
        );
        assert!(found.contains(&PathBuf::from(
            "/home/u/.nix-profile/share/tmux/tmux-agent-status.conf"
        )));
        // The prefix handed in is a root of its own, or passing it would be
        // free of consequence and this test would prove nothing about it.
        assert!(found.contains(&PathBuf::from(
            "/opt/prefix/share/tmux/tmux-agent-status.conf"
        )));
        assert!(found.contains(&PathBuf::from(
            "/usr/local/share/tmux/tmux-agent-status.conf"
        )));
        assert!(found.contains(&PathBuf::from(
            "/opt/homebrew/share/tmux/tmux-agent-status.conf"
        )));
        // A binary whose path cannot be resolved simply contributes nothing.
        assert!(!search_path(None, &home, None).is_empty());
        assert!(!search_path(Some(Path::new("/nowhere/at/all")), &home, None).is_empty());
    }

    #[test]
    fn an_explicit_snippet_is_taken_as_given() {
        let home = home_at("/home/u");
        let explicit = PathBuf::from("/nowhere/snippet.conf");
        assert_eq!(
            discover_snippet(Some(&explicit), None, Path::new("/x"), &home, None),
            Choice::Create(explicit)
        );
    }

    #[test]
    fn replacing_a_line_leaves_its_neighbours_alone() {
        let text = "one\ntwo\nthree\n";
        assert_eq!(replace_lines(text, 1, 1, "TWO"), "one\nTWO\nthree\n");
        // A continuation run collapses into the single line that replaces it.
        assert_eq!(replace_lines(text, 0, 1, "ONE"), "ONE\nthree\n");
        assert_eq!(replace_lines(text, 2, 2, "THREE"), "one\ntwo\nTHREE\n");
        // A file with no trailing newline keeps none.
        assert_eq!(replace_lines("one\ntwo", 1, 1, "TWO"), "one\nTWO");
    }

    #[test]
    fn a_glob_matches_the_way_a_config_file_uses_one() {
        assert!(matches_glob("*.conf", "a.conf"));
        assert!(matches_glob("*.conf", ".conf"));
        assert!(matches_glob("*", "anything"));
        assert!(matches_glob("a?c.conf", "abc.conf"));
        assert!(matches_glob("a*c*e", "abcde"));
        assert!(matches_glob("**", "x"));
        assert!(matches_glob("x", "x"));
        assert!(!matches_glob("*.conf", "a.toml"));
        assert!(!matches_glob("a?c", "ac"));
        assert!(!matches_glob("abc", "ab"));
        assert!(!matches_glob("ab", "abc"));
        assert!(!matches_glob("a*d", "abc"));
    }

    #[test]
    fn only_a_trailing_component_glob_is_expanded() {
        assert_eq!(
            glob_pattern(Path::new("/a/b/*.conf")).as_deref(),
            Some("*.conf")
        );
        assert_eq!(
            glob_pattern(Path::new("/a/b/c?.conf")).as_deref(),
            Some("c?.conf")
        );
        assert_eq!(glob_pattern(Path::new("/a/*/c.conf")), None);
        assert_eq!(glob_pattern(Path::new("/a/b/c.conf")), None);
        assert_eq!(glob_pattern(Path::new("/")), None);
    }
}
