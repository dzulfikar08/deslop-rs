//! Ports `TypeScript/Lint/RelativeImports.hs` — built-in lint #1.
//!
//! A relative import is rewritten through the tsconfig alias mapping and
//! reported against its old form; baselined problems keep the original text.

use std::path::Path;

use crate::baseline::Baseline;
use crate::problem::{LintRuleId, Problem};
use crate::ts::config::TsConfig;
use crate::ts::cst::{TsNode, TsProgram};
use crate::ts::module_resolver::{module_id_unsafe, reverse_resolve_import};

pub const RULE_ID: &str = "no-relative-imports";

/// Ports `relativeImport`'s problem shape. The file is the module's own path
/// made relative to the tsconfig baseUrl, extension and all.
fn relative_import_problem(file: &str, old: &TsNode, new: &TsNode) -> Problem {
    Problem::Lint {
        lint_rule: LintRuleId(RULE_ID.into()),
        file: file.to_string(),
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
    cfg: &TsConfig,
    baseline: &Baseline,
) -> (TsProgram, Vec<Problem>) {
    let module_path = Path::new(&program.path);
    let file = relative_to(&cfg.base_url, module_path);
    let mut problems = Vec::new();
    let cst = program
        .cst
        .iter()
        .map(|node| fix_import(node, cfg, baseline, module_path, &file, &mut problems))
        .collect();
    (TsProgram { path: program.path.clone(), cst }, problems)
}

fn fix_import(
    old: &TsNode,
    cfg: &TsConfig,
    baseline: &Baseline,
    module_path: &Path,
    file: &str,
    problems: &mut Vec<Problem>,
) -> TsNode {
    let TsNode::Import { prefix, target, suffix } = old else {
        return old.clone();
    };
    let new_target =
        reverse_resolve_import(cfg, module_path, &module_id_unsafe(target.clone())).text().to_string();
    if *target == new_target {
        return old.clone();
    }
    let new = TsNode::Import {
        prefix: prefix.clone(),
        target: new_target,
        suffix: suffix.clone(),
    };
    let problem = relative_import_problem(file, old, &new);
    problems.push(problem.clone());
    // Don't change baselined imports.
    if baseline.contains(&problem.id()) {
        old.clone()
    } else {
        new
    }
}

/// Ports `relativePathTo`: the target with the base prefix removed — or the
/// target unchanged when it does not sit under the base.
fn relative_to(base: &Path, target: &Path) -> String {
    target.strip_prefix(base).unwrap_or(target).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ts::config::PathMapping;
    use std::path::PathBuf;

    fn cfg(base: &str, mappings: &[(&str, &[&str])]) -> TsConfig {
        TsConfig {
            base_url: PathBuf::from(base),
            paths: mappings
                .iter()
                .map(|(k, vs)| {
                    PathMapping::parse(
                        k,
                        &vs.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    )
                    .unwrap()
                })
                .collect(),
        }
    }

    #[test]
    fn rewrites_reports_and_splices_the_new_specifier() {
        let c = cfg("/repo", &[("@/*", &["src/*"])]);
        let src = "import { User } from '../domain/user';\nconst x = 1;\n";
        let prog = crate::ts::cst::parse_ts("/repo/src/ui/button.ts", src);
        let (fixed, problems) = no_relative_imports(&prog, &c, &Baseline::empty());

        assert_eq!(problems.len(), 1, "{problems:?}");
        let Problem::Lint { lint_rule, file, code, fix, auto_fixable, .. } = &problems[0] else {
            panic!("expected lint problem");
        };
        assert_eq!(lint_rule.0, RULE_ID);
        assert_eq!(file, "src/ui/button.ts");
        assert_eq!(code, "import { User } from '../domain/user';");
        assert_eq!(fix.as_deref(), Some("Use ```import { User } from '@/domain/user';``` instead."));
        assert!(auto_fixable);

        // The rewrite is spliced in; everything else survives untouched.
        assert_eq!(
            fixed.render(),
            "import { User } from '@/domain/user';\nconst x = 1;\n"
        );
    }

    #[test]
    fn baselined_imports_keep_their_original_text() {
        let c = cfg("/repo", &[("@/*", &["src/*"])]);
        let src = "import { User } from '../domain/user';\n";
        let prog = crate::ts::cst::parse_ts("/repo/src/ui/button.ts", src);

        // Baseline exactly the problem this import produces.
        let (_, problems) = no_relative_imports(&prog, &c, &Baseline::empty());
        let ids: Vec<String> = problems.iter().map(|p| p.id().0).collect();
        let baseline = crate::baseline::Baseline::from_ids(ids);

        let (fixed, still_reported) = no_relative_imports(&prog, &c, &baseline);
        assert_eq!(still_reported.len(), 1);
        assert_eq!(fixed.render(), src);
    }

    #[test]
    fn already_aliased_and_unmappable_imports_are_untouched() {
        let c = cfg("/repo", &[("@/*", &["src/*"])]);
        let src = "import { User } from '@/domain/user';\nimport fs from 'fs';\n";
        let prog = crate::ts::cst::parse_ts("/repo/src/ui/button.ts", src);
        let (fixed, problems) = no_relative_imports(&prog, &c, &Baseline::empty());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(fixed.render(), src);

        // With no mappings at all, nothing reverse-resolves, nothing changes.
        let bare = cfg("/repo", &[]);
        let src = "import { User } from '../domain/user';\n";
        let prog = crate::ts::cst::parse_ts("/repo/src/ui/button.ts", src);
        let (fixed, problems) = no_relative_imports(&prog, &bare, &Baseline::empty());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(fixed.render(), src);
    }
}
