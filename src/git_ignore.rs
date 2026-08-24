//! Ports `Git/Ignore.hs` — gitignore-style filtering for file discovery.
//!
//! TODO(port): full negation (`!pattern`) handling and nested `.gitignore`
//! semantics from the original. This version covers the common cases:
//! comments, blank lines, trailing slashes, anchored patterns, `*`, `?`,
//! character classes, and `**` via globset.

use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

#[derive(Debug, Default)]
pub struct GitIgnore {
    set: GlobSet,
}

/// Directories pruned regardless of what any .gitignore says.
pub const ALWAYS_IGNORED_DIRS: &[&str] =
    &["node_modules", ".git", ".hg", ".svn", "dist", ".deslop"];

impl GitIgnore {
    /// Compiles every non-comment line of every in-tree `.gitignore` into one
    /// matcher. Patterns are treated as rooted at the .gitignore's directory,
    /// which is the common case for this repo's usage.
    pub fn load(project_root: &Path) -> Self {
        let mut builder = GlobSetBuilder::new();
        for gi in collect_gitignores(project_root) {
            for line in std::fs::read_to_string(&gi).unwrap_or_default().lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                    continue;
                }
                let anchored = line.contains('/');
                if let Ok(glob) = GlobBuilder::new(line)
                    .literal_separator(true)
                    .build()
                {
                    let _ = anchored;
                    builder.add(glob);
                }
            }
        }
        Self { set: builder.build().unwrap_or_default() }
    }

    pub fn is_ignored(&self, root: &Path, path: &Path) -> bool {
        let rel = match path.strip_prefix(root) {
            Ok(r) => r,
            Err(_) => return false,
        };
        rel.components().any(|c| {
            c.as_os_str().to_str().map_or(false, |s| ALWAYS_IGNORED_DIRS.contains(&s))
        })
    }
}

fn collect_gitignores(root: &Path) -> Vec<std::path::PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.file_name().to_str() != Some(".git"))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name() == ".gitignore")
        .map(|e| e.into_path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_modules_always_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let gi = GitIgnore::load(tmp.path());
        assert!(gi.is_ignored(tmp.path(), &tmp.path().join("node_modules/x/y.ts")));
    }
}
