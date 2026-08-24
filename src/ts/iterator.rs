//! Ports `TypeScript/Iterator.hs` + `FileSystem/Iterator.hs` — project file
//! discovery.

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::git_ignore::GitIgnore;

const TS_EXTENSIONS: &[&str] = &["ts", "tsx"];

/// Every `.ts`/`.tsx` file in the project that git would not ignore.
/// Ignored directories are pruned rather than descended into.
pub fn get_ts_files(git_ignore: &GitIgnore, root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                !git_ignore.is_ignored(root, e.path())
            } else {
                true
            }
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| TS_EXTENSIONS.contains(&x))
                .unwrap_or(false)
        })
        .map(walkdir::DirEntry::into_path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ts_skips_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let nm = tmp.path().join("node_modules/pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("dep.ts"), "").unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.ts"), "").unwrap();
        std::fs::write(tmp.path().join("src/readme.md"), "").unwrap();

        let files = get_ts_files(&GitIgnore::load(tmp.path()), tmp.path());
        let names: Vec<_> = files.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, ["a.ts"]);
    }
}
