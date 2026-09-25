//! Config discovery and the config-order walk, against real files.
//!
//! The walk is where an early draft went wrong: it searched the main file and
//! only descended into sourced ones if it found nothing, which gets the winner
//! wrong whenever a fragment is sourced above a later assignment. tmux runs the commands in the order it meets them and the last
//! assignment wins, so that is the order these tests assert.

use std::fs;
use std::path::{Path, PathBuf};

use tmux_agent_status::register::format::Candidate;
use tmux_agent_status::register::tmux_conf::{self, Choice};
use tmux_agent_status::register::{Home, has_marked_block};

mod support;

use support::tempdir::TempDir;

fn home_in(dir: &TempDir) -> Home {
    Home {
        home: dir.path().to_path_buf(),
        xdg_config: None,
    }
}

/// The values assigned by every format line the walk found, in order.
fn values(entry: &Path, home: &Home) -> Vec<String> {
    tmux_conf::walk(entry, home)
        .assignments
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
        tmux_conf::discover_snippet(None, Some(&bin), &dir.join(".tmux.conf"), &home, None),
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
        tmux_conf::discover_snippet(None, Some(&bin), &dir.join(".tmux.conf"), &home, None),
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
        tmux_conf::discover_snippet(None, Some(&bin), &dir.join(".tmux.conf"), &home, None),
        Choice::Create(dir.join(".tmux/tmux-agent-status.conf"))
    );
    assert_eq!(
        tmux_conf::discover_snippet(
            None,
            Some(&bin),
            &dir.join(".config/tmux/tmux.conf"),
            &home,
            None
        ),
        Choice::Create(dir.join(".config/tmux/tmux-agent-status.conf"))
    );
}

#[test]
fn an_existing_explicit_snippet_is_taken_as_it_is() {
    let dir = TempDir::new("snippet-explicit");
    let explicit = dir.write("odd/place.conf", "# ours\n");
    let home = home_in(&dir);

    assert_eq!(
        tmux_conf::discover_snippet(Some(&explicit), None, &dir.join(".tmux.conf"), &home, None),
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

    // The `~` resolves against the `Home` the walk was given, where that file
    // does not exist; what matters is that it is not looked for in a
    // directory called `~`.
    assert!(!dir.join("~").exists());
    assert_eq!(values(&entry, &home_in(&dir)), ["still read"]);
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
    assert_eq!(values(&entry, &home_in(&dir)), ["still read"]);
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

    assert_eq!(
        values(&entry, &home_in(&dir)),
        ["from the main file", "from the fragment"]
    );
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

    assert_eq!(
        values(&entry, &home_in(&dir)),
        ["from the fragment", "from the main file"]
    );
}

#[test]
fn a_refused_line_is_still_reported_so_it_can_be_handed_back() {
    let dir = TempDir::new("order-refused");
    let entry = dir.write(
        "tmux.conf",
        "set -g window-status-format 'a' ; set -g status on\n",
    );

    let found = tmux_conf::walk(&entry, &home_in(&dir)).assignments;
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

    assert_eq!(values(&a, &home_in(&dir)), ["b", "a"]);
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

    assert_eq!(values(&entry, &home_in(&dir)), ["earlier", "later"]);
}

#[test]
fn a_relative_source_is_reported_and_never_read_from_beside_the_config() {
    // tmux resolves a relative `source-file` against the directory the server
    // was started in, not against the config file's own. Reading it from
    // beside the config is the one answer that is wrong for every layout but
    // `~/.tmux.conf`, and it is wrong silently: the walk picks a winner out of
    // a file tmux never read. So it is resolved against the `Home` the walk
    // was given - in production `$HOME`, which is where the probe puts its
    // cwd - and the guess is reported. The `Home` here is a second, empty
    // directory: pointing it at the config's own would resolve the fragment
    // beside the config and prove nothing.
    let dir = TempDir::new("order-relative");
    let elsewhere = TempDir::new("order-relative-home");
    dir.write(
        "order-relative-fragment.conf",
        "set -g window-status-format 'beside the config'\n",
    );
    let entry = dir.write("tmux.conf", "source-file order-relative-fragment.conf\n");

    let found = tmux_conf::walk(&entry, &home_in(&elsewhere));
    assert!(
        found.assignments.is_empty(),
        "the fragment beside the config was read: {:?}",
        found.assignments
    );
    assert_eq!(found.relative_sources, ["order-relative-fragment.conf"]);
}

#[test]
fn an_absolute_or_tilde_source_is_not_reported_as_a_guess() {
    let dir = TempDir::new("order-absolute");
    let fragment = dir.write("fragment.conf", "set -g window-status-format 'absolute'\n");
    let entry = dir.write(
        "tmux.conf",
        &format!("source-file {}\n", fragment.display()),
    );

    let home = home_in(&dir);
    let found = tmux_conf::walk(&entry, &home);
    assert_eq!(values(&entry, &home), ["absolute"]);
    assert!(
        found.relative_sources.is_empty(),
        "{:?}",
        found.relative_sources
    );
}

// tmux expands `$NAME`/`${NAME}` in a `source-file` argument outside single
// quotes before resolving it (probed on 3.6a), so a fragment reached only
// through a variable is a file the walk must find.
#[test]
fn a_source_named_through_home_is_descended_into() {
    let dir = TempDir::new("order-home-var");
    dir.write(
        "fragment.conf",
        "set -g window-status-format 'through a variable'\n",
    );
    let entry = dir.write("tmux.conf", "source-file $HOME/fragment.conf\n");
    let home = home_in(&dir);

    let found = tmux_conf::walk(&entry, &home);
    assert_eq!(values(&entry, &home), ["through a variable"]);
    // Expanded it is an absolute path, so there is no cwd guess to report.
    assert!(
        found.relative_sources.is_empty(),
        "{:?}",
        found.relative_sources
    );
    assert!(
        found.unresolved_sources.is_empty(),
        "{:?}",
        found.unresolved_sources
    );
}

#[test]
fn a_source_named_through_xdg_config_home_is_descended_into() {
    let dir = TempDir::new("order-xdg-var");
    dir.write(
        "elsewhere/tmux/fragment.conf",
        "set -g window-status-format 'through xdg'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        "source-file ${XDG_CONFIG_HOME}/tmux/fragment.conf\n",
    );
    let home = Home {
        home: dir.path().to_path_buf(),
        xdg_config: Some(dir.join("elsewhere")),
    };

    assert_eq!(values(&entry, &home), ["through xdg"]);
}

// A variable with no value here leaves nothing to follow: the argument is
// neither descended into nor reported as a relative-path guess, which is a
// different thing that did not happen. It is reported on its own, because the
// file it names may hold the winning assignment. The line is there twice: the
// report deduplicates the way `relative_sources` does.
#[test]
fn a_source_named_through_an_unset_variable_is_reported_not_followed() {
    let dir = TempDir::new("order-unset-var");
    // The fragment is there; what stops the descent is the variable, not the file.
    dir.write(
        "fragment.conf",
        "set -g window-status-format 'never read'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        "source-file $TMUX_AGENT_STATUS_UNSET_FOR_TEST/fragment.conf\n\
         source-file $TMUX_AGENT_STATUS_UNSET_FOR_TEST/fragment.conf\n\
         set -g window-status-format 'still read'\n",
    );
    let home = home_in(&dir);

    let found = tmux_conf::walk(&entry, &home);
    assert_eq!(values(&entry, &home), ["still read"]);
    assert!(
        found.relative_sources.is_empty(),
        "{:?}",
        found.relative_sources
    );
    assert_eq!(
        found.unresolved_sources,
        ["$TMUX_AGENT_STATUS_UNSET_FOR_TEST/fragment.conf"]
    );
}

// Only the word the walk follows decides which file tmux reads: a variable
// anywhere else on the line is tmux's own concern. A second path argument
// that cannot be resolved does not stop the first being followed - or being
// reported as the relative-path guess it is - because this is a config walk,
// not a config validator.
#[test]
fn an_unresolvable_word_elsewhere_on_the_line_does_not_hide_the_path() {
    let dir = TempDir::new("order-other-arg-var");
    dir.write(
        "fragment.conf",
        "set -g window-status-format 'still followed'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        "source-file $TMUX_AGENT_STATUS_UNSET_FOR_TEST/x.conf fragment.conf\n",
    );
    let home = home_in(&dir);

    let found = tmux_conf::walk(&entry, &home);
    assert_eq!(values(&entry, &home), ["still followed"]);
    assert_eq!(found.relative_sources, ["fragment.conf"]);
    assert!(
        found.unresolved_sources.is_empty(),
        "{:?}",
        found.unresolved_sources
    );
}

// And when it is the followed word that cannot be resolved, it is the one
// named in `unresolved_sources` - the report is about the word the walk acted
// on, whatever else the line carries. The earlier path tmux would also read
// is a single-path limitation the walk does not pretend away.
#[test]
fn the_unresolvable_report_names_the_path_the_walk_followed() {
    let dir = TempDir::new("order-last-arg-var");
    dir.write(
        "fragment.conf",
        "set -g window-status-format 'tmux reads this, the walk does not'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        "source-file fragment.conf $TMUX_AGENT_STATUS_UNSET_FOR_TEST/x.conf\n",
    );
    let home = home_in(&dir);

    let found = tmux_conf::walk(&entry, &home);
    assert_eq!(
        found.unresolved_sources,
        ["$TMUX_AGENT_STATUS_UNSET_FOR_TEST/x.conf"]
    );
    assert!(
        found.relative_sources.is_empty(),
        "{:?}",
        found.relative_sources
    );
    assert!(found.assignments.is_empty(), "{:?}", found.assignments);
}

// Inside single quotes tmux expands nothing (probed on 3.6a), so a quoted
// `$HOME` is a literal, relative, path - reported as the relative-source
// guess it is, not as a variable that failed to resolve. The two reports
// answer different questions and must not share a bucket.
#[test]
fn a_single_quoted_variable_is_a_literal_relative_path() {
    let dir = TempDir::new("order-quoted-var");
    let entry = dir.write("tmux.conf", "source-file '$HOME/x.conf'\n");
    let home = home_in(&dir);

    let found = tmux_conf::walk(&entry, &home);
    assert_eq!(found.relative_sources, ["$HOME/x.conf"]);
    assert!(
        found.unresolved_sources.is_empty(),
        "{:?}",
        found.unresolved_sources
    );
    assert!(found.assignments.is_empty(), "{:?}", found.assignments);
}

// A `$` no name follows is a literal dollar, not a variable (probed: tmux
// reads `source-file frag$.conf` as the file `frag$.conf`), so the word keeps
// it - the file is followed, and its relativeness is reported the way any
// written relative argument is.
#[test]
fn a_dollar_that_names_no_variable_is_a_literal_in_the_path() {
    let dir = TempDir::new("order-literal-dollar");
    dir.write(
        "frag$.conf",
        "set -g window-status-format 'through a literal dollar'\n",
    );
    let entry = dir.write("tmux.conf", "source-file frag$.conf\n");
    let home = home_in(&dir);

    let found = tmux_conf::walk(&entry, &home);
    assert_eq!(values(&entry, &home), ["through a literal dollar"]);
    assert_eq!(found.relative_sources, ["frag$.conf"]);
    assert!(
        found.unresolved_sources.is_empty(),
        "{:?}",
        found.unresolved_sources
    );
}

// `~` resolves against the `Home` the walk was given, not the process's real
// `$HOME`: a fragment that exists only in the given one is still found.
#[test]
fn a_tilde_source_is_resolved_against_the_home_the_walk_was_given() {
    let dir = TempDir::new("order-tilde-home");
    dir.write(
        "tmux-agent-status-tilde-fragment.conf",
        "set -g window-status-format 'from the fragment'\n",
    );
    let entry = dir.write(
        "tmux.conf",
        "source-file ~/tmux-agent-status-tilde-fragment.conf\n",
    );

    assert_eq!(values(&entry, &home_in(&dir)), ["from the fragment"]);
}

// A glob under `~` resolves against the same `Home`, and still sorts the way
// tmux sorts it.
#[test]
fn a_glob_under_home_is_expanded_against_the_home_the_walk_was_given() {
    let dir = TempDir::new("order-tilde-glob");
    dir.write(
        "frag.d/20-later.conf",
        "set -g window-status-format 'later'\n",
    );
    dir.write(
        "frag.d/10-earlier.conf",
        "set -g window-status-format 'earlier'\n",
    );
    let entry = dir.write("tmux.conf", "source-file ~/frag.d/*.conf\n");

    assert_eq!(values(&entry, &home_in(&dir)), ["earlier", "later"]);
}

#[test]
fn a_source_of_something_that_is_not_there_is_not_an_error() {
    let dir = TempDir::new("order-missing");
    let entry = dir.write(
        "tmux.conf",
        "source-file /nowhere/at/all.conf\nsource-file /nowhere/*.conf\nset -g window-status-format 'still read'\n",
    );

    assert_eq!(values(&entry, &home_in(&dir)), ["still read"]);
}

#[test]
fn a_config_that_is_not_there_at_all_yields_nothing() {
    let home = Home {
        home: PathBuf::from("/nowhere"),
        xdg_config: None,
    };
    assert!(
        tmux_conf::walk(Path::new("/nowhere/at/all.conf"), &home)
            .assignments
            .is_empty()
    );
}

#[test]
fn a_config_that_assigns_nothing_yields_nothing() {
    let dir = TempDir::new("order-empty");
    let entry = dir.write("tmux.conf", "set -g status on\n# a comment\n\n");
    assert!(
        tmux_conf::walk(&entry, &home_in(&dir))
            .assignments
            .is_empty()
    );
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
    let found = values(&dir.join("0.conf"), &home_in(&dir));
    assert!(!found.is_empty(), "nothing was read at all");
    assert!(found.len() < 20, "the depth limit did not hold: {found:?}");
}

#[test]
fn the_source_block_round_trips_through_the_reader_that_looks_for_it() {
    let dir = TempDir::new("source-block");
    let snippet = dir.join(".config/tmux/tmux-agent-status.conf");
    let before = "set -g status on\n";

    let after = tmux_conf::with_source_block(before, &snippet).expect("the path can be spelled");

    assert!(!tmux_conf::sources_snippet(before));
    assert!(tmux_conf::sources_snippet(&after));
    assert!(has_marked_block(&after));
    // Appending twice would be two sets of hooks, which is what the
    // idempotency check exists to prevent.
    assert!(tmux_conf::sources_snippet(
        &tmux_conf::with_source_block(&after, &snippet).expect("the path can be spelled")
    ));
}

#[test]
fn a_sourced_snippet_is_recognised_through_the_walk_as_well() {
    let dir = TempDir::new("source-through-walk");
    let snippet = dir.write(".tmux/tmux-agent-status.conf", tmux_conf::SNIPPET);
    let entry = dir.write(
        "tmux.conf",
        &tmux_conf::with_source_block("set -g status on\n", &snippet)
            .expect("the path can be spelled"),
    );

    let text = fs::read_to_string(&entry).expect("the config can be read");
    assert!(tmux_conf::sources_snippet(&text));
    // The shipped snippet sets hooks and no format, so the walk finds nothing
    // to edit in it: the two steps really are independent.
    assert!(values(&entry, &home_in(&dir)).is_empty());
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
