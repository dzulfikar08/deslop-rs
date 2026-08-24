//! Ports `Deslop/Lint/RelativeImports.hs` — built-in lint #1.
//!
//! A relative import is rewritten through the tsconfig alias mapping and
//! reported against its old form; baselined problems keep the original text.

use crate::problem::{LintRuleId, Problem};
use crate::ts::cst::{TsNode, TsProgram};

pub const RULE_ID: &str = "no-relative-imports";

/// Ports `relativeImport`'s problem shape. `project_dir` is the baseUrl in
/// the original, which is what `Location.file` is made relative to.
fn relative_import_problem(
    project_dir: &std::path::Path,
    module_path: &str,
    old: &TsNode,
    new_target: &str,
) -> Problem {
    let _ = project_dir;
    let mut new = old.clone();
    if let TsNode::Import { target: t, .. } = &mut new {
        *t = new_target.to_string();
    }
    Problem::Lint {
        lint_rule: LintRuleId(RULE_ID.into()),
        file: module_path.to_string(),
        code: old.render(),
        description: "Relative imports are not allowed. Use aliased ones.".into(),
        fix: Some(format!("Use ```{}``` instead.", new.render())),
        auto_fixable: true,
    }
}

/// Ports `noRelativeImports`: rewrite every relative import whose alias
/// resolution differs from what it says, report each change, and leave
/// baselined problems' source untouched. Returns the (possibly edited)
/// program for lossless re-render, plus the reported problems.
pub fn no_relative_imports(
    program: &TsProgram,
    project_dir: &std::path::Path,
) -> (TsProgram, Vec<Problem>) {
    let _ = (program, project_dir);
    // TODO(port): reverseResolveImport through TypeScript.ModuleResolver to
    // compute each target's alias form; splice rewrites into a fresh CST and
    // consult the baseline per problem. The rule's shape is pinned by the
    // original: report "Relative imports are not allowed. Use aliased ones."
    // with fix "Use ```<rewritten>``` instead.", autoFixable = True.
    (program.clone(), Vec::new())
}
