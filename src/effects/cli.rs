//! Ports `Effects/CLI.hs` — logging surface. Colors go to stdout; errors are
//! printed raw-ANSI to stderr like the original's redStderr.

use std::io::Write;

pub const DIVIDER: &str = "─────────────────────────────────────────";

fn stdout_ln(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}

pub fn plain(text: &str) {
    stdout_ln(text);
}

pub fn blue_bold(text: &str) {
    stdout_ln(&format!("\x1b[1;34m{text}\x1b[0m"));
}

pub fn green(text: &str) {
    stdout_ln(&format!("\x1b[32m{text}\x1b[0m"));
}

pub fn yellow_bold(text: &str) {
    stdout_ln(&format!("\x1b[1;33m{text}\x1b[0m"));
}

pub fn cyan_bold(text: &str) {
    stdout_ln(&format!("\x1b[1;36m{text}\x1b[0m"));
}

/// Red text on stderr, ANSI codes written raw (stderr is never queried for
/// color support by the original either).
pub fn error(text: &str) {
    let mut err = std::io::stderr().lock();
    let _ = write!(err, "\x1b[31m{text}\x1b[0m\n");
    let _ = err.flush();
}

// TODO(port): honor NO_COLOR / TTY detection if the Haskell side does via
// ansi-terminal.
