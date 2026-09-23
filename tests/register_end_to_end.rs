//! `register` against a real tmux, finishing the job.
//!
//! The other suites prove each piece in isolation. These prove the thing the
//! plan actually promises: run `register`, start tmux on what it wrote, and a
//! real `@agent_status` renders in a real window entry - with no file edited by
//! hand anywhere along the way.

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

mod support;

use support::tempdir::TempDir;

/// A tmux server on a socket of its own, started on a given config.
struct Server {
    socket: String,
}

impl Server {
    fn start_on(config: &Path) -> Server {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let server = Server {
            socket: format!(
                "tmux-agent-status-register-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ),
        };
        server.run(&[
            "-f",
            &config.display().to_string(),
            "new-session",
            "-d",
            "-s",
            "t",
            "-x",
            "100",
            "-y",
            "20",
            "sleep 300",
        ]);
        server
    }

    fn run(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            // A client with no UTF-8 locale renders the glyphs as underscores,
            // and a build sandbox has no locale at all.
            .arg("-u")
            .arg("-L")
            .arg(&self.socket)
            .args(args)
            .env("PATH", bin_dir_first_on_path())
            .stdin(Stdio::null())
            .output()
            .expect("tmux is on PATH");
        assert!(
            out.status.success(),
            "tmux {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn option(&self, args: &[&str]) -> String {
        self.run(args).trim_end().to_owned()
    }

    fn socket_path(&self) -> String {
        self.option(&["display-message", "-p", "#{socket_path}"])
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The binary under test, ahead of anything installed on this machine.
fn bin_dir_first_on_path() -> String {
    let bin = Path::new(support::BIN)
        .parent()
        .expect("the test binary has a directory");
    match std::env::var_os("PATH") {
        Some(path) => format!("{}:{}", bin.display(), path.to_string_lossy()),
        None => bin.display().to_string(),
    }
}

fn register(home: &TempDir, args: &[&str]) -> Output {
    Command::new(support::BIN)
        .arg("register")
        .args(args)
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("TMUX")
        .env_remove("TMUX_AGENT_STATUS_TEST_FAULT")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Wait for a rendered status line to settle, because tmux redraws when it
/// feels like it rather than when a test would prefer.
fn wait_for(read: impl Fn() -> String, done: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let seen = read();
        if done(&seen) || Instant::now() >= deadline {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// 29. A config with a known format, registered into, then actually loaded: the
// term is in both options, every hook and `focus-events` are registered, and a hand-set
// `@agent_status` renders in the window entry.
#[test]
fn a_registered_config_puts_a_real_glyph_on_a_real_window() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-format");
    let config = home.write(
        ".config/tmux/tmux.conf",
        "set -g status on\n\
         set -g window-status-format '#I:#W#{?window_flags,#{window_flags}, }'\n\
         set -g window-status-current-format '#I:#W#{?window_flags,#{window_flags}, }'\n",
    );
    let snippet = home.join(".config/tmux/tmux-agent-status.conf");

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-format",
            "--tmux-hook",
            "--tmux-config",
            &config.display().to_string(),
            "--snippet",
            &snippet.display().to_string(),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));

    let server = Server::start_on(&config);

    // Both options, because a term in only one of them makes the glyph vanish
    // the moment the window becomes current.
    for option in ["window-status-format", "window-status-current-format"] {
        let value = server.option(&["show-options", "-gwv", option]);
        assert!(
            value.contains("@agent_status"),
            "{option} has no term: {value}"
        );
    }

    // Every hook, in the scope tmux keeps it in: `session-window-changed` is
    // global, the other two are window-scoped.
    let global = server.run(&["show-hooks", "-g"]);
    let window = server.run(&["show-hooks", "-gw"]);
    assert!(
        global.contains("session-window-changed"),
        "the shipped hook is not registered: {global}"
    );
    for hook in ["window-pane-changed", "pane-focus-in"] {
        assert!(
            window.contains(hook) && window.contains("clear-pane"),
            "the shipped {hook} hook is not registered: {window}"
        );
    }
    assert_eq!(
        server
            .option(&["show-options", "-gv", "focus-events"])
            .trim(),
        "on"
    );

    // And the whole point: a glyph, rendered, in a window entry.
    let pane = server
        .option(&["list-panes", "-t", "t", "-F", "#{pane_id}"])
        .lines()
        .next()
        .expect("the session has a pane")
        .to_owned();
    server.run(&["set-option", "-w", "-t", &pane, "@agent_status", "GLYPH"]);
    let rendered = wait_for(
        || {
            server.option(&[
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{W:#{E:window-status-format}}",
            ])
        },
        |seen| seen.contains("GLYPH"),
    );
    assert!(
        rendered.contains("GLYPH"),
        "no glyph in the entry: {rendered}"
    );
}

// 30. The same against a config with no format line, proving the default probe
// produces a working pair.
#[test]
fn a_config_with_no_format_line_gets_a_working_pair_from_tmuxs_own_default() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-default");
    let config = home.write(".config/tmux/tmux.conf", "set -g status on\n");
    let snippet = home.join(".config/tmux/tmux-agent-status.conf");

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-format",
            "--tmux-config",
            &config.display().to_string(),
            "--snippet",
            &snippet.display().to_string(),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));

    let written = fs::read_to_string(&config).expect("the config");
    assert!(written.contains("# >>> tmux-agent-status >>>"), "{written}");

    let server = Server::start_on(&config);
    for option in ["window-status-format", "window-status-current-format"] {
        let value = server.option(&["show-options", "-gwv", option]);
        assert!(value.contains("@agent_status"), "{option}: {value}");
        // The default carries the flags term, and the glyph goes before it.
        assert!(value.contains("window_flags"), "{option}: {value}");
    }

    let pane = server
        .option(&["list-panes", "-t", "t", "-F", "#{pane_id}"])
        .lines()
        .next()
        .expect("a pane")
        .to_owned();
    server.run(&["set-option", "-w", "-t", &pane, "@agent_status", "GLYPH"]);
    let rendered = wait_for(
        || {
            server.option(&[
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{W:#{E:window-status-format}}",
            ])
        },
        |seen| seen.contains("GLYPH"),
    );
    assert!(rendered.contains("GLYPH"), "{rendered}");
}

#[test]
fn registering_into_a_config_twice_leaves_exactly_one_of_everything() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-twice");
    let config = home.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    let snippet = home.join(".config/tmux/tmux-agent-status.conf");
    let args = [
        "-y",
        "--tmux-format",
        "--tmux-hook",
        "--tmux-config",
        &config.display().to_string(),
        "--snippet",
        &snippet.display().to_string(),
    ];
    let args: Vec<&str> = args.iter().map(|a| a.as_ref()).collect();

    assert_eq!(register(&home, &args).status.code(), Some(0));
    let after_first = fs::read_to_string(&config).expect("the config");

    let second = register(&home, &args);
    assert_eq!(second.status.code(), Some(0), "{}", stdout(&second));
    assert!(
        stdout(&second).contains("already registered"),
        "{}",
        stdout(&second)
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the config"),
        after_first,
        "the second run changed the file"
    );
    // The term itself names `@agent_status` twice, so the thing to count is
    // the term, once per option and no more.
    let term = "#{?@agent_status, #{@agent_status},}";
    assert_eq!(
        after_first.matches(term).count(),
        2,
        "the term was not registered exactly once per option:\n{after_first}"
    );
    assert_eq!(
        after_first.matches("source-file").count(),
        1,
        "a second set of hooks was sourced:\n{after_first}"
    );
}

// 23. The probe catches an abandoned config, the edit is rolled back, and the
// original config still produces its original options.
#[test]
fn a_malformed_splice_is_rolled_back_and_the_config_still_works() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-rollback");
    let config = home.write(
        ".config/tmux/tmux.conf",
        "set -g status-left 'LEFT'\n\
         set -g window-status-format '#I:#W'\n\
         set -g window-status-current-format '#I:#W'\n",
    );
    let before = fs::read_to_string(&config).expect("the config");

    let out = Command::new(support::BIN)
        .arg("register")
        .args([
            "-y",
            "--tmux-format",
            "--tmux-config",
            &config.display().to_string(),
        ])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("TMUX")
        // A stray argument after the value, which is what a quoting bug
        // produces and what tmux throws the whole file away over.
        .env("TMUX_AGENT_STATUS_TEST_FAULT", "bad-splice")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("tmux will not read the edited config"),
        "{text}"
    );
    assert!(text.contains("restored from"), "{text}");
    assert_eq!(
        fs::read_to_string(&config).expect("the config"),
        before,
        "the config was not rolled back"
    );

    // And the original config still does what it did: the rollback is only
    // worth anything if what comes back works.
    let server = Server::start_on(&config);
    assert_eq!(
        server.option(&["show-options", "-gv", "status-left"]),
        "LEFT"
    );
    assert_eq!(
        server.option(&["show-options", "-gwv", "window-status-format"]),
        "#I:#W"
    );
}

// The reload is the last thing a run offers, and it must only ever source a
// config the running server actually loads.
#[test]
fn the_reload_sources_the_config_into_the_server_that_loads_it() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-reload");
    let config = home.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    // A server already running on that config, which is the situation the
    // reload exists for.
    let server = Server::start_on(&config);
    let socket = server.socket_path();

    let out = Command::new(support::BIN)
        .arg("register")
        .args([
            "-y",
            "--tmux-format",
            "--tmux-config",
            &config.display().to_string(),
        ])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        // How tmux finds the server the caller is in.
        .env("TMUX", format!("{socket},0,0"))
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("tmux reloaded"), "{text}");
    // The running server picked the term up without being restarted.
    for option in ["window-status-format", "window-status-current-format"] {
        let value = server.option(&["show-options", "-gwv", option]);
        assert!(value.contains("@agent_status"), "{option}: {value}");
    }
}

#[test]
fn a_reload_tmux_refuses_is_reported_without_undoing_the_edit() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-reload-refused");
    let config = home.write(
        ".config/tmux/tmux.conf",
        "set -g window-status-format '#I:#W'\nset -g window-status-current-format '#I:#W'\n",
    );
    let server = Server::start_on(&config);
    let socket = server.socket_path();

    let out = Command::new(support::BIN)
        .arg("register")
        .args([
            "-y",
            "--tmux-format",
            "--tmux-config",
            &config.display().to_string(),
        ])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env("TMUX", format!("{socket},0,0"))
        .env("TMUX_AGENT_STATUS_TEST_FAULT", "bad-reload")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    let text = stdout(&out);
    // The edit stands: the reload is a convenience, not part of the write.
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("refused the reload"), "{text}");
    assert!(
        fs::read_to_string(&config)
            .expect("the config")
            .contains("@agent_status"),
        "the edit was undone by a failed reload"
    );
}

// 31. The rule `AGENTS.md` keeps its teeth on: the format option's *text* may
// be edited in a config file, but it must never be written through tmux.
//
// Writing a spliced copy back has to go to a window-local option, which freezes
// that window's format forever. The hazard is a property of the tmux *option*,
// not of the format string, which is why the strings themselves are all over
// `format.rs` and are not what this looks for.
#[test]
fn window_status_format_is_never_an_argument_to_set_option() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for file in rust_files(&src) {
        let text = fs::read_to_string(&file).expect("the source can be read");
        checked += 1;
        for group in bracket_groups(&text) {
            assert!(
                !(group.contains("\"set-option\"") && group.contains("window-status")),
                "{}: `window-status-format` must never be written through tmux, at any \
                 scope. A spliced copy on a window-local option freezes that window's \
                 format forever; edit the text of the user's config file instead.\n  {group}",
                file.display()
            );
        }
    }
    assert!(checked > 0, "no sources were scanned");
}

/// The scan is only worth having if it catches the thing it forbids, and only
/// usable if it does not catch the things it must not.
#[test]
fn the_set_option_scan_catches_a_violation_and_nothing_else() {
    let violation = r#"tmux(&["set-option", "-w", "-t", target, "window-status-format", value])"#;
    assert!(
        bracket_groups(violation)
            .iter()
            .any(|group| group.contains("\"set-option\"") && group.contains("window-status")),
        "the scan would not catch a real violation"
    );

    // Reading it is fine, and is how a setup check tells the user whether the
    // term is present.
    let reading = r#"run_here(&["show-options", "-gwv", "window-status-format"])"#;
    assert!(
        bracket_groups(reading)
            .iter()
            .all(|group| !group.contains("\"set-option\"")),
        "reading the option must not trip the scan"
    );

    // And the parser's own list of command names, which sits in the same file
    // as the option names and is what an earlier version of this test tripped
    // over.
    let parser = "const SET_COMMANDS: [&str; 2] = [\"set\", \"set-option\"];\n\
                  const OPTIONS: [&str; 1] = [\"window-status-format\"];";
    assert!(
        bracket_groups(parser)
            .iter()
            .all(|group| !(group.contains("\"set-option\"") && group.contains("window-status"))),
        "the scan trips over the parser's own tables"
    );
}

/// Every `[...]` group in some source, which is the shape a tmux argument list
/// always takes in this crate.
fn bracket_groups(text: &str) -> Vec<String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut groups = Vec::new();
    let mut opens: Vec<usize> = Vec::new();
    for (at, c) in bytes.iter().enumerate() {
        match c {
            '[' => opens.push(at),
            ']' => {
                if let Some(start) = opens.pop() {
                    groups.push(bytes[start..=at].iter().collect());
                }
            }
            _ => {}
        }
    }
    groups
}

fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            found.push(path);
        }
    }
    found
}

// An edit that changes the file and changes nothing about tmux. Checking that
// nothing *unexpected* moved passes such an edit perfectly, so the probe is
// asked for what the edit was for as well: without that, this run reported
// both options registered, exited 0, and left a config whose effective format
// carries no term - a successful-looking registration with no glyph and nothing to
// see in the diff.
#[test]
fn a_splice_a_later_assignment_overrides_is_rolled_back_and_reported() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-overridden");
    let config = home.write(
        ".config/tmux/tmux.conf",
        "set -g status on\n\
         set -g window-status-format '#I:#W'\n\
         set -g window-status-current-format '#I:#W'\n\
         if-shell 'true' 'set -g window-status-format \"#I:#W-late\"'\n\
         if-shell 'true' 'set -g window-status-current-format \"#I:#W-late\"'\n",
    );
    let before = fs::read_to_string(&config).expect("the config");

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-format",
            "--tmux-config",
            &config.display().to_string(),
        ],
    );

    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(
        text.contains("still reads window-status-format without the term"),
        "{text}"
    );
    // The term to add, so the user is left holding what they need.
    assert!(
        text.contains("#{?@agent_status, #{@agent_status},}"),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the config"),
        before,
        "the rejected edit was not rolled back"
    );
}

// The same half of the question for the hook step: a `source-file` line
// pointing at a file that registers no hooks is a line that does nothing.
#[test]
fn a_source_line_pointing_at_a_snippet_that_sets_no_hooks_is_rolled_back() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-hookless");
    let config = home.write(".config/tmux/tmux.conf", "set -g status on\n");
    let snippet = home.write(
        ".config/tmux/tmux-agent-status.conf",
        "# a snippet that sets no hooks at all\n",
    );
    let before = fs::read_to_string(&config).expect("the config");

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-hook",
            "--tmux-config",
            &config.display().to_string(),
            "--snippet",
            &snippet.display().to_string(),
        ],
    );

    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("without the session-window-changed hook"),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the config"),
        before,
        "the rejected edit was not rolled back"
    );
}

// The option is half of what the snippet is for: hooks that land without
// `focus-events on` silently lose the one that sees the terminal regain focus.
#[test]
fn a_snippet_that_sets_the_hooks_but_not_focus_events_is_rolled_back() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-no-focus-events");
    let config = home.write(".config/tmux/tmux.conf", "set -g status on\n");
    let snippet = home.write(
        ".config/tmux/tmux-agent-status.conf",
        "set-hook -g 'pane-focus-in[50]' 'run-shell -b \"tmux-agent-status clear-pane #{pane_id}\"'\n\
         set-hook -g 'session-window-changed[50]' 'run-shell -b \"tmux-agent-status clear-pane #{pane_id}\"'\n\
         set-hook -g 'window-pane-changed[50]' 'run-shell -b \"tmux-agent-status clear-pane #{pane_id}\"'\n",
    );
    let before = fs::read_to_string(&config).expect("the config");

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-hook",
            "--tmux-config",
            &config.display().to_string(),
            "--snippet",
            &snippet.display().to_string(),
        ],
    );

    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("does not turn focus-events on"),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the config"),
        before,
        "the rejected edit was not rolled back"
    );
}

// The documented way to decline `focus-events` is a line of the user's own,
// after the source-file line. That is a choice, not a snippet that failed to
// land, so an upgrade still replaces the older copy the config sources.
#[test]
fn a_declined_focus_events_does_not_stop_an_older_snippet_being_replaced() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-declined-focus-events");
    let snippet = home.write(
        ".config/tmux/tmux-agent-status.conf",
        "set-hook -g 'session-window-changed[50]' 'run-shell -b \"tmux-agent-status clear-window #{pane_id}\"'\n",
    );
    let config = home.write(
        ".config/tmux/tmux.conf",
        &format!(
            "source-file {}\nset -g focus-events off\n",
            snippet.display()
        ),
    );

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-hook",
            "--tmux-config",
            &config.display().to_string(),
        ],
    );

    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert_eq!(
        fs::read_to_string(&snippet).expect("the snippet"),
        include_str!("../share/tmux/tmux-agent-status.conf")
    );
}

// tmux expands `$NAME` and `${NAME}` in a source-file argument (verified on
// 3.6a), so a config that names its snippet through `$HOME` sources the real
// file. The upgrade has to replace that file, not create one under a
// directory literally called `$HOME`.
#[test]
fn a_snippet_sourced_through_a_variable_is_the_one_replaced() {
    if !support::tmux_or_skip() {
        return;
    }
    let home = TempDir::new("e2e-variable-source");
    let snippet = home.write(
        ".config/tmux/tmux-agent-status.conf",
        "set-hook -g 'session-window-changed[50]' 'run-shell -b \"tmux-agent-status clear-window #{pane_id}\"'\n",
    );
    let config = home.write(
        ".config/tmux/tmux.conf",
        "source-file ${HOME}/.config/tmux/tmux-agent-status.conf\n",
    );

    let out = register(
        &home,
        &[
            "-y",
            "--tmux-hook",
            "--tmux-config",
            &config.display().to_string(),
        ],
    );

    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(!stdout(&out).contains("relative path"), "{}", stdout(&out));
    assert_eq!(
        fs::read_to_string(&snippet).expect("the snippet"),
        include_str!("../share/tmux/tmux-agent-status.conf")
    );
    assert!(!home.join("${HOME}").exists());
}
