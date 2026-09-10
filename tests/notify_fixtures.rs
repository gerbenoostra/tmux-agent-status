//! The `notify` mapping, driven by fixtures in `tests/fixtures/mistral-vibe/`.
//!
//! Each `.json` file is a payload. The filename prefix is the expected result:
//! `working-`, `done-`, or `dropped-`.

use std::fs;
use std::path::Path;
use tmux_agent_status::notify::dispatch;
use tmux_agent_status::state::State;

fn expected_state(name: &str) -> Option<State> {
    if name.starts_with("working-") {
        Some(State::Working)
    } else if name.starts_with("done-") {
        Some(State::Done)
    } else if name.starts_with("dropped-") {
        None
    } else {
        panic!("unknown fixture prefix in {name}");
    }
}

#[test]
fn every_fixture_maps_to_its_expected_state() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mistral-vibe");
    assert!(dir.is_dir(), "fixtures directory missing");

    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let payload = fs::read_to_string(&path).unwrap();
        let name = path.file_stem().unwrap().to_str().unwrap();
        let expected = expected_state(name);
        let actual = dispatch("mistral-vibe", &payload);
        assert_eq!(actual, expected, "{}", path.display());
        checked += 1;
    }

    assert!(checked > 0, "no fixtures were checked");
}
