//! Config discovery and the config-order walk, against real files.
//!
//! The walk is what `011` calls its own defect: an early draft searched the
//! main file and only descended into sourced ones if it found nothing, which
//! gets the winner wrong whenever a fragment is sourced above a later
//! assignment. tmux runs the commands in the order it meets them and the last
//! assignment wins, so that is the order these tests assert.

use std::fs;
use std::path::{Path, PathBuf};

use tmux_agent_status::install::format::Candidate;
use tmux_agent_status::install::tmux_conf::{self, Choice};
use tmux_agent_status::install::{Home, has_marked_block};

mod support;

use support::tempdir::TempDir;

fn home_in(dir: &TempDir) -> Home {
    Home {
        home: dir.path().to_path_buf(),
        xdg_config: None,
    }
}

/// The values assigned by every format line the walk found, in order.
fn values(entry: &Path) -> Vec<String> {
    tmux_conf::assignments(entry)
        .into_iter()
        .filter_map(|found| found.candidate.line())
        .map(|line| line.raw)
        .collect()
}

#[test]
fn the_xdg_config_wins_when_it_exists() {
    let dir = TempDir::new("config-xdg");
    dir.write(".config/tmux/tmux.conf", "");
    dir.write(".tmux.conf", "");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_config(None, &home),
        Choice::Existing(dir.join(".config/tmux/tmux.conf"))
    );
}

#[test]
fn the_home_config_is_used_when_it_is_the_only_one() {
    let dir = TempDir::new("config-home");
    dir.write(".tmux.conf", "");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_config(None, &home),
        Choice::Existing(dir.join(".tmux.conf"))
    );
}

#[test]
fn an_xdg_config_home_elsewhere_is_looked_at_first() {
    let dir = TempDir::new("config-xdg-elsewhere");
    dir.write("elsewhere/tmux/tmux.conf", "");
    dir.write(".tmux.conf", "");
    let home = Home {
        home: dir.path().to_path_buf(),
        xdg_config: Some(dir.join("elsewhere")),
    };

    assert_eq!(
        tmux_conf::discover_config(None, &home),
        Choice::Existing(dir.join("elsewhere/tmux/tmux.conf"))
    );
}

#[test]
fn an_existing_explicit_config_is_taken_as_it_is() {
    let dir = TempDir::new("config-explicit");
    let explicit = dir.write("odd/place.conf", "");
    dir.write(".tmux.conf", "");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_config(Some(&explicit), &home),
        Choice::Existing(explicit)
    );
}

#[test]
fn a_snippet_beside_the_binary_is_found() {
    let dir = TempDir::new("snippet");
    let bin = dir.join("prefix/bin/tmux-agent-status");
    fs::create_dir_all(bin.parent().expect("a bin directory")).expect("the directory");
    fs::write(&bin, "not really a binary").expect("the file");
    let snippet = dir.write("prefix/share/tmux/tmux-agent-status.conf", "# ours\n");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_snippet(None, Some(&bin), &dir.join(".tmux.conf"), &home),
        Choice::Existing(snippet)
    );
}

#[test]
fn a_checkout_layout_two_levels_up_is_found_too() {
    let dir = TempDir::new("snippet-checkout");
    let bin = dir.join("target/release/tmux-agent-status");
    fs::create_dir_all(bin.parent().expect("a bin directory")).expect("the directory");
    fs::write(&bin, "not really a binary").expect("the file");
    let snippet = dir.write("share/tmux/tmux-agent-status.conf", "# ours\n");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_snippet(None, Some(&bin), &dir.join(".tmux.conf"), &home),
        Choice::Existing(snippet)
    );
}

#[test]
fn a_binary_with_no_share_beside_it_falls_through_to_a_copy() {
    // The dev-loop case: a `~/.local/bin` shadow pointing into a build
    // directory finds nothing at `../share`, and must fall through rather than
    // fail. Where the copy goes follows the config in use.
    let dir = TempDir::new("snippet-none");
    let bin = dir.write("local/bin/tmux-agent-status", "not really a binary");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_snippet(None, Some(&bin), &dir.join(".tmux.conf"), &home),
        Choice::Create(dir.join(".tmux/tmux-agent-status.conf"))
    );
    assert_eq!(
        tmux_conf::discover_snippet(None, Some(&bin), &dir.join(".config/tmux/tmux.conf"), &home),
        Choice::Create(dir.join(".config/tmux/tmux-agent-status.conf"))
    );
}

#[test]
fn an_existing_explicit_snippet_is_taken_as_it_is() {
    let dir = TempDir::new("snippet-explicit");
    let explicit = dir.write("odd/place.conf", "# ours\n");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_snippet(Some(&explicit), None, &dir.join(".tmux.conf"), &home),
        Choice::Existing(explicit)
    );
}

#[test]
fn a_home_relative_source_is_expanded_rather_than_taken_literally() {
    let dir = TempDir::new("order-tilde");
    let entry = dir.write(
        "tmux.conf",
        "source-file ~/tmux-agent-status-no-such-fragment.conf
set -g window-status-format 'still read'
",
    );

    // The `~` resolves against the real $HOME, where that file does not exist;
    // what matters is that it is not looked for in a directory called `~`.
    assert!(!dir.join("~").exists());
    assert_eq!(values(&entry), ["still read"]);
}

#[test]
fn a_glob_over_a_directory_that_is_not_there_yields_nothing() {
    let dir = TempDir::new("order-glob-missing");
    let entry = dir.write(
        "tmux.conf",
        &format!(
            "source-file {}/nowhere/*.conf
set -g window-status-format 'still read'\n",
            dir.path().display()
        ),
    );
    assert_eq!(values(&entry), ["still read"]);
}

// 5. Config-order resolution: the defect a draft of this plan had, in both
// directions, so the fix stays fixed.
#[test]
fn the_last_assignment_in_tmuxs_own_order_is_the_one_found_last() {
    let dir = TempDir::new("order-fragment-wins");
    dir.write(
        "fragment.conf",
        "set -g window-status-format 'from the fragment'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        &format!(
            "set -g window-status-format 'from the main file'\nsource-file {}\n",
            dir.join("fragment.conf").display()
        ),
    );

    assert_eq!(values(&entry), ["from the main file", "from the fragment"]);
}

#[test]
fn reversing_the_two_reverses_the_winner() {
    let dir = TempDir::new("order-main-wins");
    dir.write(
        "fragment.conf",
        "set -g window-status-format 'from the fragment'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        &format!(
            "source-file {}\nset -g window-status-format 'from the main file'\n",
            dir.join("fragment.conf").display()
        ),
    );

    assert_eq!(values(&entry), ["from the fragment", "from the main file"]);
}

#[test]
fn a_refused_line_is_still_reported_so_it_can_be_handed_back() {
    let dir = TempDir::new("order-refused");
    let entry = dir.write(
        "tmux.conf",
        "set -g window-status-format 'a' ; set -g status on\n",
    );

    let found = tmux_conf::assignments(&entry);
    assert_eq!(found.len(), 1);
    assert!(matches!(&found[0].candidate, Candidate::Refused { .. }));
    assert_eq!(found[0].option(), Some("window-status-format"));
    assert_eq!(found[0].file, entry);
    assert_eq!((found[0].line.first, found[0].line.last), (0, 0));
}

#[test]
fn a_cycle_of_sourced_files_terminates() {
    let dir = TempDir::new("order-cycle");
    let a = dir.join("a.conf");
    let b = dir.join("b.conf");
    fs::write(
        &a,
        format!(
            "source-file {}\nset -g window-status-format 'a'\n",
            b.display()
        ),
    )
    .expect("a.conf");
    fs::write(
        &b,
        format!(
            "source-file {}\nset -g window-status-format 'b'\n",
            a.display()
        ),
    )
    .expect("b.conf");

    assert_eq!(values(&a), ["b", "a"]);
}

#[test]
fn a_glob_is_expanded_and_sorted() {
    let dir = TempDir::new("order-glob");
    dir.write(
        "conf.d/20-later.conf",
        "set -g window-status-format 'later'\n",
    );
    dir.write(
        "conf.d/10-earlier.conf",
        "set -g window-status-format 'earlier'\n",
    );
    dir.write(
        "conf.d/notes.txt",
        "set -g window-status-format 'not sourced'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        &format!("source-file {}/conf.d/*.conf\n", dir.path().display()),
    );

    assert_eq!(values(&entry), ["earlier", "later"]);
}

#[test]
fn a_relative_source_resolves_against_the_config_it_sits_in() {
    let dir = TempDir::new("order-relative");
    dir.write("fragment.conf", "set -g window-status-format 'relative'\n");
    let entry = dir.write("tmux.conf", "source-file fragment.conf\n");

    assert_eq!(values(&entry), ["relative"]);
}

#[test]
fn a_source_of_something_that_is_not_there_is_not_an_error() {
    let dir = TempDir::new("order-missing");
    let entry = dir.write(
        "tmux.conf",
        "source-file /nowhere/at/all.conf\nsource-file /nowhere/*.conf\nset -g window-status-format 'still read'\n",
    );

    assert_eq!(values(&entry), ["still read"]);
}

#[test]
fn a_config_that_is_not_there_at_all_yields_nothing() {
    assert!(tmux_conf::assignments(Path::new("/nowhere/at/all.conf")).is_empty());
}

#[test]
fn a_config_that_assigns_nothing_yields_nothing() {
    let dir = TempDir::new("order-empty");
    let entry = dir.write("tmux.conf", "set -g status on\n# a comment\n\n");
    assert!(tmux_conf::assignments(&entry).is_empty());
}

#[test]
fn a_chain_deeper_than_the_limit_stops_rather_than_running_away() {
    let dir = TempDir::new("order-deep");
    // Twenty files, each sourcing the next; the walk gives up partway and the
    // assignments it did reach are still reported.
    for step in 0..20 {
        let next = dir.join(&format!("{}.conf", step + 1));
        dir.write(
            &format!("{step}.conf"),
            &format!(
                "set -g window-status-format 'step {step}'\nsource-file {}\n",
                next.display()
            ),
        );
    }
    let found = values(&dir.join("0.conf"));
    assert!(!found.is_empty(), "nothing was read at all");
    assert!(found.len() < 20, "the depth limit did not hold: {found:?}");
}

#[test]
fn the_source_block_round_trips_through_the_reader_that_looks_for_it() {
    let dir = TempDir::new("source-block");
    let snippet = dir.join(".config/tmux/tmux-agent-status.conf");
    let before = "set -g status on\n";

    let after = tmux_conf::with_source_block(before, &snippet);

    assert!(!tmux_conf::sources_snippet(before));
    assert!(tmux_conf::sources_snippet(&after));
    assert!(has_marked_block(&after));
    // Appending twice would be two sets of hooks, which is what the
    // idempotency check exists to prevent.
    assert!(tmux_conf::sources_snippet(&tmux_conf::with_source_block(
        &after, &snippet
    )));
}

#[test]
fn a_sourced_snippet_is_recognised_through_the_walk_as_well() {
    let dir = TempDir::new("source-through-walk");
    let snippet = dir.write(".tmux/tmux-agent-status.conf", tmux_conf::SNIPPET);
    let entry = dir.write(
        "tmux.conf",
        &tmux_conf::with_source_block("set -g status on\n", &snippet),
    );

    let text = fs::read_to_string(&entry).expect("the config can be read");
    assert!(tmux_conf::sources_snippet(&text));
    // The shipped snippet sets hooks and no format, so the walk finds nothing
    // to edit in it: the two steps really are independent.
    assert!(values(&entry).is_empty());
}

#[test]
fn tmuxs_candidate_list_names_the_system_config_without_choosing_it() {
    let listed = tmux_conf::candidates("/etc/tmux.conf,~/.tmux.conf");
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|path| tmux_conf::is_system_wide(path)));

    let dir = TempDir::new("config-no-system");
    let home = home_in(&dir);
    let chosen = tmux_conf::discover_config(None, &home);
    assert!(
        !tmux_conf::is_system_wide(chosen.path()),
        "the system config must never be chosen: {chosen:?}"
    );
    assert_eq!(chosen, Choice::Create(dir.join(".config/tmux/tmux.conf")));
}

#[test]
fn a_choice_names_its_path_either_way() {
    let path = PathBuf::from("/tmp/x.conf");
    assert_eq!(Choice::Existing(path.clone()).path(), path);
    assert_eq!(Choice::Create(path.clone()).path(), path);
}
