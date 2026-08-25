//! Ports `TypeScript/Iterator.hs` + `FileSystem/Iterator.hs` — project file
//! discovery.

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::git_ignore::GitIgnore;

const TS_EXTENSIONS: &[&str] = &["ts", "tsx"];

/// Every `.ts`/`.tsx` file in the project that git would not ignore.
/// Always-ignored directories are pruned regardless of what any `.gitignore`
/// says, and ignored entries — directories and files alike — are pruned
/// rather than descended into.
///
/// Order is the original's walk: depth-first pre-order, each directory's
/// children sorted, which is what feeds problem ordering.
pub fn get_ts_files(git_ignore: &GitIgnore, root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .sort_by_key(|e| e.file_name().to_os_string())
        .into_iter()
        .filter_entry(|e| !git_ignore.is_ignored(root, e.path(), entry_is_dir(e)))
        .filter_map(Result::ok)
        // `select` in the original: not a directory, and a TS extension — a
        // symlink to a file passes both, a symlink to a directory neither.
        .filter(|e| !entry_is_dir(e))
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

/// Whether an entry is a directory, following symlinks as `fsDirectoryExists`
/// does — a symlink to a directory prunes like the directory it names, even
/// though the walk itself never descends into it.
fn entry_is_dir(e: &walkdir::DirEntry) -> bool {
    std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false)
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

    /// The original's walk is pre-order with sorted children, not a global
    /// sort: `src/a.ts` sorts before sibling `src/aaa/`, whose subtree follows,
    /// while a global sort would place `src/a.ts` between `src/a` entries.
    #[test]
    fn walks_pre_order_with_sorted_children() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("a")).unwrap();
        std::fs::create_dir_all(src.join("z")).unwrap();
        std::fs::write(src.join("a/inner.ts"), "").unwrap();
        std::fs::write(src.join("a.ts"), "").unwrap();
        std::fs::write(src.join("z/late.ts"), "").unwrap();

        let files = get_ts_files(&GitIgnore::load(tmp.path()), tmp.path());
        let rel: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(tmp.path()).unwrap().to_str().unwrap().replace('\\', "/"))
            .collect();
        // Pre-order: `a` (dir) subtree first, then `a.ts`, then `z` — sibling
        // order `a` < `a.ts` < `z`, each directory followed by its children.
        assert_eq!(rel, ["src/a/inner.ts", "src/a.ts", "src/z/late.ts"]);
    }
}
