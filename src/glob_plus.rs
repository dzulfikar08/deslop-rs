//! Port target: `Deslop/GlobPlus.hs` + `GlobPlus/Compiler.hs` (~1160 LOC).
//!
//! A bespoke glob dialect ("Glob+") whose exact semantics are specified across
//! docs/adr/0004 and docs/adr/0005 and pinned by property/oracle tests. It
//! powers both rulebook matching and gitignore handling.
//!
//! TODO(port): compile(pattern) -> Matcher with the ADR-specified semantics:
//! brace expansion, character classes, recursive `**`, negation, and the
//! Glob+-specific extensions documented in docs/GLOB+.md.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

pub struct GlobPlus {
    set: GlobSet,
}

impl GlobPlus {
    /// Approximation: plain globs only. Must be replaced by the real compiler
    /// before any rulebook semantics are trusted.
    pub fn compile(patterns: &[String]) -> Self {
        let mut b = GlobSetBuilder::new();
        for p in patterns {
            if let Ok(g) = GlobBuilder::new(p).literal_separator(true).build() {
                b.add(g);
            }
        }
        Self { set: b.build().unwrap_or_default() }
    }

    pub fn matches(&self, path: &str) -> bool {
        self.set.is_match(path)
    }
}
