//! The terminal bell.

use std::fs::OpenOptions;
use std::io::Write;

/// Ring the bell on the controlling terminal.
///
/// Not on stdout: agents capture a hook's stdout and would swallow the escape.
/// Best effort - a caller with no controlling terminal is not an error.
pub fn ring() {
    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.write_all(b"\x07");
        let _ = tty.flush();
    }
}
