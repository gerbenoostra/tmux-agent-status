//! Behaviour of `install.sh`'s latest-release lookup, driven against stub
//! `curl` and `uname` commands so nothing depends on live GitHub timing.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The release the installer under test ships with.
fn min_version() -> String {
    let script = fs::read_to_string(script_path()).expect("install.sh");
    let line = script
        .lines()
        .find(|l| l.starts_with("MIN_VERSION="))
        .expect("MIN_VERSION line");
    line.trim_start_matches("MIN_VERSION=")
        .trim_matches('"')
        .to_string()
}

fn script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("install.sh")
}

fn write_stub(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Runs the installer with `redirect_tag` answering the `/releases/latest`
/// redirect and `api_tag` answering the API. Every download fails, so the run
/// ends right after choosing a version; returns the output and the URL log.
fn run_installer(redirect_tag: &str, api_tag: &str, pinned: Option<&str>) -> (Output, String) {
    let work = tempfile::tempdir().unwrap();
    let bin = work.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let log = work.path().join("urls.log");
    write_stub(
        &bin,
        "uname",
        r#"case "$1" in -s) echo Darwin ;; -m) echo arm64 ;; esac"#,
    );
    write_stub(
        &bin,
        "curl",
        &format!(
            r#"echo "$*" >> '{log}'
case "$*" in
  *-fsSI*releases/latest*) printf 'HTTP/2 302\r\nlocation: https://github.com/o/r/releases/tag/{redirect_tag}\r\n\r\n' ;;
  *api.github.com*)
    out=""; while [ $# -gt 0 ]; do [ "$1" = -o ] && out=$2; shift; done
    printf '{{"tag_name": "{api_tag}"}}' > "$out" ;;
  *) exit 22 ;;
esac"#,
            log = log.display(),
        ),
    );
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut cmd = Command::new("sh");
    cmd.arg(script_path())
        .env("PATH", path)
        .env("HOME", work.path())
        .env_remove("TMUX_AGENT_STATUS_VERSION");
    if let Some(v) = pinned {
        cmd.env("TMUX_AGENT_STATUS_VERSION", v);
    }
    let output = cmd.output().unwrap();
    let urls = fs::read_to_string(&log).unwrap_or_default();
    (output, urls)
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

#[test]
fn fresh_redirect_is_used_without_touching_the_api() {
    let min = min_version();
    let tag = format!("v{min}");
    let (out, urls) = run_installer(&tag, "v0.0.1", None);
    assert!(text(&out.stdout).contains(&format!("Installing version: {tag}")));
    assert!(!urls.contains("api.github.com"), "{urls}");
}

#[test]
fn stale_redirect_falls_back_to_the_api() {
    let (out, urls) = run_installer("v0.0.1", "v99.0.0", None);
    let stdout = text(&out.stdout);
    assert!(stdout.contains("Installing version: v99.0.0"), "{stdout}");
    assert!(urls.contains("api.github.com"), "{urls}");
    assert!(urls.contains("releases/download/v99.0.0/"), "{urls}");
}

#[test]
fn stale_redirect_and_stale_api_install_the_api_version_with_a_warning() {
    let (out, urls) = run_installer("v0.0.1", "v0.0.2", None);
    assert!(text(&out.stdout).contains("Installing version: v0.0.2"));
    assert!(text(&out.stdout).contains("may not be published yet"));
    assert!(urls.contains("releases/download/v0.0.2/"), "{urls}");
}

#[test]
fn redirect_newer_than_the_installer_is_used_without_touching_the_api() {
    let (out, urls) = run_installer("v99.0.0", "v0.0.1", None);
    assert!(text(&out.stdout).contains("Installing version: v99.0.0"));
    assert!(!urls.contains("api.github.com"), "{urls}");
}

#[test]
fn pinned_version_skips_the_lookup_even_when_older() {
    let (out, urls) = run_installer("v99.0.0", "v99.0.0", Some("0.0.1"));
    assert!(text(&out.stdout).contains("Using pinned version: v0.0.1"));
    assert!(urls.contains("releases/download/v0.0.1/"), "{urls}");
    assert!(!urls.contains("releases/latest"), "{urls}");
    assert!(!urls.contains("api.github.com"), "{urls}");
}
