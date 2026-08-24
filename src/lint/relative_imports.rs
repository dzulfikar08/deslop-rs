//! Ports `Deslop/Lint/RelativeImports.hs` — built-in lint #1.

use crate::ast::{AstModule, ImportEntry};
use crate::problem::{LintRuleId, Problem};

pub const RULE_ID: &str = "no-relative-imports";

/// Flags relative imports; auto-fix rewrites them through tsconfig aliases.
pub fn no_relative_imports(module: &AstModule) -> Vec<Problem> {
    module
        .imports
        .iter()
        .filter(|i| i.is_relative)
        .map(|ImportEntry { specifier, .. }| Problem::Lint {
            lint_rule: LintRuleId(RULE_ID.into()),
            file: String::new(), // filled by pipeline which knows the path
            code: format!("import '{specifier}'"),
            description: format!("relative import '{specifier}'"),
            fix: Some(specifier.clone()),
            auto_fixable: true,
        })
        .collect()
}

// TODO(port): alias-based rewrite + CST splice so `deslop fix` edits files
// losslessly.
