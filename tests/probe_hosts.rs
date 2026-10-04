//! The `probe/hosts/<name>.sh` installers behind `probe-lifecycle.sh`.
//!
//! The harness sources one of these files and calls four functions on it;
//! these tests exercise that same interface in a scratch directory and pin
//! where the generated hook config lands. No agent binary is ever launched:
//! the probe's safety contract is that hook configuration lives only under
//! the scratch workspace, and a test that started the real host would break
//! it. `ANTHROPIC_API_KEY` is set to a dummy value because the Claude
//! installer probes the login keychain when no key is present, and a test
//! must never trigger that prompt.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tmux_agent_status::register::agents;

mod support;

use support::tempdir::TempDir;

/// (host, binary, generated config path relative to the scratch directory,
/// whether the host only discovers repo-scoped hooks from a Git worktree).
const HOSTS: &[(&str, &str, &str, bool)] = &[
    (
        "claude-code",
        "claude",
        "claude-config/settings.json",
        false,
    ),
    ("codex", "codex", "workspace/.codex/hooks.json", true),
    (
        "copilot",
        "copilot",
        "workspace/.github/hooks/tas-probe.json",
        true,
    ),
    (
        "cursor",
        "cursor-agent",
        "workspace/.cursor/hooks.json",
        true,
    ),
    ("devin", "devin", "workspace/.devin/hooks.v1.json", false),
    ("droid", "droid", "workspace/.factory/hooks.json", false),
    ("gemini", "gemini", "workspace/.gemini/settings.json", false),
    ("grok", "grok", "workspace/.grok/hooks/tas-probe.json", true),
    (
        "kiro",
        "kiro-cli",
        "kiro-home/.kiro/hooks/tas-probe.json",
        false,
    ),
    ("mistral-vibe", "vibe", "workspace/.vibe/hooks.toml", false),
];

fn probe_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("probe")
}

fn host_file(host: &str) -> PathBuf {
    probe_dir().join("hosts").join(format!("{host}.sh"))
}

/// Run `call` in a fresh shell after sourcing the installer, the way
/// probe-lifecycle.sh does: PROBE_DIR in the environment, the scratch
/// directory as `$2`. `--noprofile --norc` keeps the user's shell setup out
/// of the probe.
fn call(host: &str, call: &str, scratch: &Path) -> Output {
    Command::new("bash")
        .args(["--noprofile", "--norc", "-c"])
        .arg(format!("set -euo pipefail\n. \"$1\"\n{call}\n"))
        .arg("probe")
        .arg(host_file(host))
        .arg(scratch)
        .env("PROBE_DIR", probe_dir())
        .env("ANTHROPIC_API_KEY", "probe-test-dummy")
        .output()
        .expect("bash runs the probe function")
}

fn stdout_of(out: &Output, host: &str, what: &str) -> String {
    assert!(
        out.status.success(),
        "{host}: {what} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn every_probe_host_is_a_registered_agent_with_an_installer() {
    for (host, ..) in HOSTS {
        assert!(
            agents::names().contains(host),
            "`{host}` has a probe installer but is not a registered agent"
        );
        assert!(
            host_file(host).is_file(),
            "registered probe host `{host}` has no installer at {}",
            host_file(host).display()
        );
    }
    for name in agents::names() {
        assert_eq!(
            HOSTS.iter().filter(|(host, ..)| *host == name).count(),
            1,
            "registered agent `{name}` must appear exactly once in HOSTS"
        );
    }
}

#[test]
fn the_four_function_interface_produces_a_scratch_local_config() {
    for (host, binary, config, needs_git) in HOSTS {
        let scratch = TempDir::new(&format!("probe-{host}"));
        fs::create_dir_all(scratch.join("workspace")).unwrap();

        let out = call(host, "probe_binary", scratch.path());
        assert_eq!(stdout_of(&out, host, "probe_binary").trim(), *binary);

        let out = call(host, "probe_install_hooks \"$2\"", scratch.path());
        stdout_of(&out, host, "probe_install_hooks");
        let config_path = scratch.join(config);
        assert!(
            config_path.is_file(),
            "{host}: probe_install_hooks did not write {}",
            config_path.display()
        );
        assert_eq!(
            scratch.join("workspace/.git").is_dir(),
            *needs_git,
            "{host}: workspace worktree does not match its hook discovery"
        );

        let out = call(host, "probe_launch_command \"$2\"", scratch.path());
        let launch = stdout_of(&out, host, "probe_launch_command");
        let workspace = scratch.join("workspace");
        assert!(
            launch.contains(binary) && launch.contains(&*workspace.to_string_lossy()),
            "{host}: launch command `{launch}` does not name `{binary}` in {}",
            workspace.display()
        );

        let out = call(host, "probe_scenarios", scratch.path());
        let scenarios = stdout_of(&out, host, "probe_scenarios");
        assert!(
            scenarios.contains("S1") && scenarios.contains("S12"),
            "{host}: the scenario list must cover S1 through S12, \
             marking the ones the host cannot express"
        );

        let written = fs::read_to_string(&config_path).unwrap();
        let logger = probe_dir().join("log-hook.sh");
        assert!(
            written.contains(&*logger.to_string_lossy()),
            "{host}: {} does not invoke the probe logger",
            config_path.display()
        );
        let log = scratch.join("hooks.jsonl");
        assert!(
            written.contains(&*log.to_string_lossy()),
            "{host}: {} does not append to the probe log",
            config_path.display()
        );
    }
}
