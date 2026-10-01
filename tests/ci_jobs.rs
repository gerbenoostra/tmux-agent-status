//! `just ci` runs the justfile's per-OS job lists; `ci.yml` runs its jobs. The
//! two are kept in step by hand, so a job added to one alone would let a local
//! run pass that CI fails. This holds them equal.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The runner labels `ci.yml` uses, and the justfile list each one maps to.
const RUNNERS: [(&str, &str); 2] = [
    ("ubuntu-latest", "ci_linux_jobs"),
    ("macos-latest", "ci_macos_jobs"),
];

#[test]
fn local_ci_job_lists_match_ci_yml() {
    let workflow = read(".github/workflows/ci.yml");
    let justfile = read("justfile");
    let jobs = workflow_jobs(&workflow);
    assert!(
        !jobs.is_empty(),
        "ci.yml names no jobs; the parser lost them"
    );
    for (runner, list) in RUNNERS {
        let in_ci: BTreeSet<&str> = jobs
            .iter()
            .filter(|job| job.runners.iter().any(|r| r == runner))
            .flat_map(|job| job.recipes.iter().map(String::as_str))
            .collect();
        let local = just_list(&justfile, list);
        let in_just: BTreeSet<&str> = local.iter().copied().collect();
        assert_eq!(
            in_just.len(),
            local.len(),
            "justfile `{list}` names a recipe twice"
        );
        assert_eq!(
            in_just, in_ci,
            "justfile `{list}` and ci.yml's {runner} jobs run different recipes"
        );
    }
}

struct Job {
    name: String,
    runners: Vec<String>,
    recipes: Vec<String>,
}

/// The jobs under `jobs:`, each with the runners it runs on and the recipes its
/// `run: just ...` steps call. Every job must run on a known runner and call at
/// least one recipe: a job that is only inline script cannot run locally.
fn workflow_jobs(workflow: &str) -> Vec<Job> {
    let mut jobs: Vec<Job> = Vec::new();
    let mut in_jobs = false;
    let mut matrix_os: Vec<String> = Vec::new();
    for line in workflow.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            in_jobs = line == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        let job_name = line
            .strip_prefix("  ")
            .and_then(|l| l.strip_suffix(':'))
            .filter(|name| !name.starts_with(' '));
        if let Some(name) = job_name {
            jobs.push(Job {
                name: name.to_owned(),
                runners: Vec::new(),
                recipes: Vec::new(),
            });
            matrix_os.clear();
            continue;
        }
        let Some(job) = jobs.last_mut() else { continue };
        // A step's `run:` is its first key (`- run:`) or follows a `- name:`.
        let key = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if let Some(list) = key.strip_prefix("os: [").and_then(|l| l.strip_suffix(']')) {
            matrix_os = list.split(',').map(|os| os.trim().to_owned()).collect();
        } else if let Some(runner) = key.strip_prefix("runs-on: ") {
            job.runners = if runner == "${{ matrix.os }}" {
                matrix_os.clone()
            } else {
                vec![runner.to_owned()]
            };
        } else if let Some(recipes) = key.strip_prefix("run: just ") {
            job.recipes
                .extend(recipes.split_whitespace().map(str::to_owned));
        }
    }
    for job in &jobs {
        assert!(
            !job.recipes.is_empty(),
            "ci.yml job `{}` calls no just recipe, so `just ci` cannot run it",
            job.name
        );
        assert!(
            !job.runners.is_empty(),
            "ci.yml job `{}` names no runner the parser understands",
            job.name
        );
        for runner in &job.runners {
            assert!(
                RUNNERS.iter().any(|(known, _)| known == runner),
                "ci.yml job `{}` runs on `{runner}`, which no local job list covers",
                job.name
            );
        }
    }
    jobs
}

/// The recipe names in the justfile variable `name := "..."`.
fn just_list<'a>(justfile: &'a str, name: &str) -> Vec<&'a str> {
    let prefix = format!("{name} := \"");
    let value = justfile
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
        .unwrap_or_else(|| panic!("justfile defines no `{name}`"));
    value.split_whitespace().collect()
}

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// A pre-push hook run from a linked worktree inherits an absolute `GIT_DIR`.
/// `_ci-snapshot` must not let it steer its git calls: they belong to the
/// snapshot, and otherwise detach the pushing worktree's HEAD.
#[test]
fn ci_snapshot_ignores_the_git_dir_a_worktree_hook_inherits() {
    // The recipe is a `#!/usr/bin/env bash` script, which the Nix build sandbox
    // on Linux cannot run; `just test` and CI's other jobs cover it.
    if !Path::new("/usr/bin/env").exists() {
        eprintln!("skipped: no /usr/bin/env in this sandbox");
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch dir");
    let root = scratch.path();
    let main = root.join("main");
    let worktree = root.join("worktree");
    git(root, &["init", "--quiet", "--initial-branch=main", "main"]);
    // The commit under snapshot carries the job lists the snapshot reads.
    fs::write(
        main.join("justfile"),
        "ci_macos_jobs := \"probe\"\nci_linux_jobs := \"probe\"\nprobe:\n    @true\n",
    )
    .expect("write justfile");
    git(&main, &["add", "justfile"]);
    git(&main, &["commit", "--quiet", "-m", "probe"]);
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "pushing",
            worktree.to_str().unwrap(),
        ],
    );
    let commit = git(&worktree, &["rev-parse", "HEAD"]);
    let git_dir = git(&worktree, &["rev-parse", "--absolute-git-dir"]);
    let dir = root.join("ci");

    let output = Command::new("just")
        .arg("--justfile")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("justfile"))
        .args(["_ci-snapshot"])
        .arg(&worktree)
        .arg(&commit)
        .arg(&dir)
        .arg("macos")
        .current_dir(&worktree)
        .env("GIT_DIR", &git_dir)
        .output()
        .expect("run just");
    assert!(
        output.status.success(),
        "_ci-snapshot failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        dir.join("src/.git").exists(),
        "the snapshot was not made in its own repository"
    );
    assert_eq!(
        git(&worktree, &["symbolic-ref", "--short", "HEAD"]),
        "pushing",
        "the pushing worktree's HEAD was moved"
    );
}

/// Run git in `dir`, without any repository variable of the test's own
/// environment, and return its trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
