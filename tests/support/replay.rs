//! Replay probed lifecycle fixtures through a shipped drop-in.
//!
//! A scenario's records are played back in order inside a pane of a
//! disposable tmux server, so `$TMUX`/`$TMUX_PANE` are set the way the host
//! sets them and the bell reaches a real tty where `window_bell_flag` can see
//! it. Every record yields one row, `NNN event command pane window bell`,
//! held in the scenario's `expected.tsv`. With `TAS_REPLAY_WRITE=1` (only
//! `1`) the rows are written instead of asserted. A command taking `--stdin`
//! is fed the record's `payload` from a file, recorded in the command column
//! as `< NNN-event`.
//!
//! Which commands an event runs is the host's own rule, so callers pass it
//! in; [`commands`] walks one event's hook entries once a caller has decided
//! how that host applies a matcher.

use std::fs;
use std::time::Duration;

use super::lifecycle::{Scenario, read_record};
use super::tempdir::TempDir;
use super::tmux::{Server, wait_for};

/// The pane's shell. Without `--noprofile --norc` it would source the
/// developer's startup files, which can put an installed `tmux-agent-status`
/// ahead of the build under test on `PATH`; without `HISTFILE=/dev/null` the
/// SIGHUP from `kill-server` makes it save every replayed command into the
/// developer's `~/.bash_history`. `PATH` itself comes from the server, which
/// `support::tmux` starts with the binary under test in front.
const SHELL: &str = "env HISTFILE=/dev/null bash --noprofile --norc";

const HEADER: &str = "# file\tevent\tcommand\tpane\twindow\tbell";

impl Server {
    /// Run a hook command the way the host would: inside the pane, so $TMUX
    /// and $TMUX_PANE are set and the bell lands on the pane's tty.
    ///
    /// Completion is a `wait-for` channel the pane signals after the
    /// command, never text on the screen: a long command wraps at the pane's
    /// width, and a wrapped piece of the typed input can read exactly like a
    /// marker the command has not printed yet. tmux remembers a signal sent
    /// before anyone waits, and the call is bounded like every test-side
    /// tmux call. The hook's bell is already readable on the pane's pty
    /// before the signalling `tmux` client is spawned, so tmux has drained
    /// it by the time that client connects. tmux documents no such ordering;
    /// measured, 1500 rounds on eight loaded servers never saw the bell flag
    /// lag the signal.
    fn run_hook(&self, pane: &str, command: &str, seq: usize) {
        let channel = format!("tas_replay_{seq}");
        self.tmux(&[
            "send-keys",
            "-t",
            pane,
            "-l",
            &format!("{command}; tmux wait-for -S {channel}"),
        ]);
        self.tmux(&["send-keys", "-t", pane, "Enter"]);
        self.tmux(&["wait-for", &channel]);
    }

    /// The pane's state, its window's rollup and the window's bell flag, as
    /// `pane\twindow\tbell`, read in one round trip.
    fn observed(&self, pane: &str) -> String {
        self.tmux(&[
            "display-message",
            "-p",
            "-t",
            pane,
            "#{@agent_pane_status}\t#{@agent_status}\t#{window_bell_flag}",
        ])
        .trim_end()
        .to_owned()
    }
}

/// The commands of one event's hook entries - a JSON array of
/// `{matcher?, hooks: [{command}]}` - whose matcher `applies` accepts. An
/// entry without a matcher always runs.
pub fn commands(entries: &serde_json::Value, applies: impl Fn(&str) -> bool) -> Vec<String> {
    let mut found = Vec::new();
    for entry in entries.as_array().into_iter().flatten() {
        let runs = entry
            .get("matcher")
            .and_then(|m| m.as_str())
            .is_none_or(&applies);
        if !runs {
            continue;
        }
        for hook in entry["hooks"].as_array().into_iter().flatten() {
            if let Some(command) = hook.get("command").and_then(|c| c.as_str()) {
                found.push(command.to_owned());
            }
        }
    }
    found
}

/// Replay one scenario and return its `NNN\tevent\tcommand\tpane\twindow\tbell`
/// rows; `commands_for` names the commands the drop-in runs for a record.
pub fn replay(
    scenario: &Scenario,
    commands_for: impl Fn(&serde_json::Value) -> Vec<String>,
) -> Vec<String> {
    let server = Server::start_running(SHELL);
    server.tmux(&["set-option", "-g", "monitor-bell", "on"]);
    server.tmux(&["set-option", "-g", "bell-action", "any"]);
    // A second window so selecting away and back clears the bell flag between
    // events without an attached client.
    server.tmux(&["new-window", "-d", "-n", "dummy", super::tmux::IDLE]);
    let pane = server
        .tmux(&["list-panes", "-t", "t:0", "-F", "#{pane_id}"])
        .trim_end()
        .to_owned();
    let window = server
        .tmux(&["display-message", "-p", "-t", &pane, "#{window_id}"])
        .trim_end()
        .to_owned();

    // Keys sent before the pane's shell is ready can be dropped, so handshake
    // first: resend until the pane echoes back. The typed line is far
    // shorter than the pane is wide, so only the echo's output is a line
    // that is exactly the marker.
    wait_for(
        || {
            server.tmux(&["send-keys", "-t", &pane, "-l", "echo __tas_ready__"]);
            server.tmux(&["send-keys", "-t", &pane, "Enter"]);
            std::thread::sleep(Duration::from_millis(50));
            server.tmux(&["capture-pane", "-t", &pane, "-p"])
        },
        |screen| screen.lines().any(|line| line.trim() == "__tas_ready__"),
    );

    let mut rows = Vec::new();
    let payloads = TempDir::new(&format!("replay-{}", scenario.name()));
    for (seq, path) in scenario.records.iter().enumerate() {
        let record = read_record(path);
        let event = record["event"].as_str().unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let commands = commands_for(&record);
        let command = if commands.is_empty() {
            "-".to_owned()
        } else {
            // Hook commands that take `--stdin` get the record's payload piped
            // in, the way the host delivers it. Written to a file rather than
            // inlined into a shell string, so payload quoting cannot decide
            // what the command does.
            let mut stdin_file = None;
            if commands.iter().any(|command| command.contains("--stdin")) {
                let file = payloads.write(
                    &format!("{stem}.payload.json"),
                    &serde_json::to_string(&record["payload"]).unwrap(),
                );
                stdin_file = Some(format!("'{}'", file.display()));
            }
            for (i, command) in commands.iter().enumerate() {
                let run = match &stdin_file {
                    Some(file) if command.contains("--stdin") => {
                        format!("{command} < {file}")
                    }
                    _ => command.clone(),
                };
                server.run_hook(&pane, &run, seq * 100 + i);
            }
            commands
                .iter()
                .map(|command| match &stdin_file {
                    Some(_) if command.contains("--stdin") => format!("{command} < {stem}"),
                    _ => command.clone(),
                })
                .collect::<Vec<_>>()
                .join(" && ")
        };
        let observed = server.observed(&pane);
        // Selecting the window marks it viewed and clears the bell flag.
        server.tmux(&["select-window", "-t", "t:dummy"]);
        server.tmux(&["select-window", "-t", &format!("t:{window}")]);
        rows.push(format!("{stem}\t{event}\t{command}\t{observed}"));
    }
    rows
}

/// Compare `rows` with the scenario's `expected.tsv`, or write them there
/// when `TAS_REPLAY_WRITE=1`.
pub fn check_or_write(scenario: &Scenario, rows: &[String]) {
    let expected_path = scenario.expected_path();
    if std::env::var_os("TAS_REPLAY_WRITE").is_some_and(|v| v == "1") {
        let mut text = String::from(HEADER);
        text.push('\n');
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        fs::write(&expected_path, text).unwrap();
        return;
    }
    let expected = fs::read_to_string(&expected_path).unwrap_or_else(|e| {
        panic!(
            "{} missing (run with TAS_REPLAY_WRITE=1 to record): {e}",
            expected_path.display()
        )
    });
    let expected_rows: Vec<&str> = expected.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(
        rows.len(),
        expected_rows.len(),
        "{}: fixture count changed - re-record with TAS_REPLAY_WRITE=1",
        scenario.name()
    );
    for (actual, wanted) in rows.iter().zip(&expected_rows) {
        assert_eq!(actual, wanted, "{}: replay drifted", scenario.name());
    }
}
