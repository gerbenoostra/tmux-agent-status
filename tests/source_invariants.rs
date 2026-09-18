//! Invariants about the sources themselves rather than about behaviour.

use std::fs;
use std::path::{Path, PathBuf};

/// The window formats this tool must never write to.
const WINDOW_FORMATS: [&str; 2] = ["window-status-format", "window-status-current-format"];

/// The exact boundary of the unit-test module every file's `#[cfg(test)]`
/// attribute is expected to sit on. Private items can only be unit-tested
/// from inside their own file (an integration test under `tests/` only sees
/// `pub` items), so parser fixtures that name these formats as test data live
/// in the same file as the real `set-option` calls this test scans for.
const TEST_MODULE_MARKER: &str = "#[cfg(test)]\nmod tests";

/// Writing a spliced copy of a window format to a window-local option freezes
/// that window's format forever, so no `set-option` this tool runs may name one.
/// Reading them with `show-options` stays allowed, and so does writing the text
/// of a config file the user maintains; only the option write is the hazard.
///
/// The check is textual and deliberately blunt: everything from a `set-option`
/// to the end of its statement must not mention a window format.
#[test]
fn no_set_option_argument_names_a_window_format() {
    let mut checked = 0;
    for path in rust_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")) {
        let text = fs::read_to_string(&path).expect("a source file this crate owns");
        // Every `#[cfg(test)]` in a source file is expected to be this exact
        // module boundary; a stray one elsewhere (a cfg-gated helper, a `#[cfg(test)]`
        // mentioned in a comment) would otherwise let real code past it slip by
        // unscanned, so treat a mismatch as a failure rather than ignoring it.
        let cfg_test_count = text.matches("#[cfg(test)]").count();
        let test_module_count = text.matches(TEST_MODULE_MARKER).count();
        assert_eq!(
            cfg_test_count,
            test_module_count,
            "{}: a #[cfg(test)] attribute isn't on the file's `mod tests` boundary; \
             the production-code scan below only excludes that boundary",
            path.display()
        );
        assert!(
            test_module_count <= 1,
            "{}: more than one `{TEST_MODULE_MARKER}` block; \
             the production-code scan below only excludes the first one",
            path.display()
        );
        let production = match text.find(TEST_MODULE_MARKER) {
            Some(at) => &text[..at],
            None => &text,
        };
        for statement in set_option_statements(production) {
            for format in WINDOW_FORMATS {
                assert!(
                    !statement.contains(format),
                    "{}: a set-option names {format}, which freezes that window's format forever:\n{statement}",
                    path.display()
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "no set-option call was found to check");
}

/// Every `.rs` file under `dir`, recursively.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).expect("the source directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            found.extend(rust_sources(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            found.push(path);
        }
    }
    found
}

/// The text from each `set-option` to the end of its statement.
fn set_option_statements(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("set-option") {
        let statement = &rest[at..];
        let end = statement.find(';').unwrap_or(statement.len());
        found.push(&statement[..end]);
        rest = &rest[at + "set-option".len()..];
    }
    found
}
