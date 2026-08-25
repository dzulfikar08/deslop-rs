//! Ports `Effects/CLI.hs` — logging surface. Colors go to stdout; errors are
//! printed raw-ANSI to stderr like the original's redStderr. The original's
//! ansi-terminal `setSGR` emits codes unconditionally — no TTY check, no
//! NO_COLOR — and so does this.
//!
//! `DESLOP_TRANSCRIPT=1` swaps every style for the test double's `[Style] `
//! prefix on stdout, byte-comparable with the Haskell suite's goldens.

use std::io::Write;
use std::sync::OnceLock;

pub const DIVIDER: &str = "─────────────────────────────────────────";

fn transcript() -> bool {
    static TRANSCRIPT: OnceLock<bool> = OnceLock::new();
    *TRANSCRIPT.get_or_init(|| std::env::var("DESLOP_TRANSCRIPT").as_deref() == Ok("1"))
}

fn stdout_ln(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}

/// One styled line: its name in the original's `LogStyle`, its colour, its
/// stream. In transcript mode every style lands on stdout, tagged.
fn styled(name: &str, ansi: &str, text: &str) {
    if transcript() {
        stdout_ln(&format!("[{name}] {text}"));
        return;
    }
    if ansi.is_empty() {
        stdout_ln(text);
    } else {
        stdout_ln(&format!("{ansi}{text}\x1b[0m"));
    }
}

pub fn plain(text: &str) {
    styled("Plain", "", text);
}

pub fn blue_bold(text: &str) {
    styled("Title", "\x1b[1;34m", text);
}

pub fn green(text: &str) {
    styled("Success", "\x1b[32m", text);
}

pub fn yellow_bold(text: &str) {
    styled("Warning", "\x1b[1;33m", text);
}

pub fn cyan_bold(text: &str) {
    styled("Change", "\x1b[1;36m", text);
}

/// Red text on stderr, ANSI codes written raw (stderr is never queried for
/// color support by the original either).
pub fn error(text: &str) {
    if transcript() {
        stdout_ln(&format!("[Error] {text}"));
        return;
    }
    let mut err = std::io::stderr().lock();
    writeln!(err, "\x1b[31m{text}\x1b[0m").unwrap();
    let _ = err.flush();
}

#[cfg(test)]
mod tests {
    #[test]
    fn styled_prefixes_match_the_test_double() {
        let tag = |name: &str, text: &str| format!("[{name}] {text}");
        assert_eq!(tag("Title", "🚀 hi"), "[Title] 🚀 hi");
        assert_eq!(tag("Error", "boom"), "[Error] boom");
    }
}
