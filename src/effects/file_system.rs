//! Ports `Effects/FileSystem.hs` — the handful of fs operations deslop needs.
//! Thin wrappers over std::fs; kept as functions rather than traits since
//! Rust callers can already swap them in tests with tempfile.

use std::path::Path;

pub fn file_exists(path: &Path) -> bool {
    path.is_file()
}

pub fn read_file(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

pub fn write_file(path: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

pub fn mkdir_p(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}
