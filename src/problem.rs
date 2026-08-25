//! Ports `Deslop/Problem.hs` — problem values and their stable ids.

use serde::Serialize;

/// The path part of an id is always spelled with `/` so ids match across OSes
/// (baselines are committed and shared).
pub fn portable_path(p: &str) -> String {
    p.replace('\\', "/")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct ProblemId(pub String);

/// Which built-in lint produced the problem (`no-relative-imports` today).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct LintRuleId(pub String);

/// How a Rule was broken. The Rule's own prose says why the Rule exists; this
/// says what the module actually did, and carries the facts a report is
/// written from rather than the sentence itself — `problem_formatter` owns
/// that. Ports `Deslop.Problem.ViolationKind`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum ViolationKind {
    /// The module names the forbidden module in an import of its own.
    DirectImport { imported: String, import_statement: String },
    /// The module arrives at a forbidden module by following imports. `chain`
    /// runs from the module to what it must not reach, and `first_import` is
    /// the import that opens it — absent when the chain has no first hop.
    TransitiveImport {
        chain: Vec<String>,
        first_import: Option<String>,
        /// The chains this violation stands in for, once duplicates have been
        /// compacted. Empty until the shrinker runs, and empty afterwards for
        /// a violation that had no duplicates.
        also_reached: Vec<Vec<String>>,
    },
    /// The module does not import something the Rule requires it to.
    MissingUse { required_import: String, transitive: bool },
    /// A module the Rule requires to exist does not.
    MissingModule { required_module: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Problem {
    Lint {
        lint_rule: LintRuleId,
        file: String,
        code: String,
        description: String,
        fix: Option<String>,
        auto_fixable: bool,
    },
    Rule {
        rulebook_id: String,
        rule_id: String,
        bad_module: String,
        prose: String,
        kind: ViolationKind,
        fix: String,
    },
}

impl Problem {
    /// Stable identity used by baselines and fix skip-lists. Must stay
    /// byte-identical to the Haskell `problemId` so baselines interoperate:
    /// lint = `<rule>#<portable path>`, violation = `<rulebook>#<rule>#<module>`.
    pub fn id(&self) -> ProblemId {
        match self {
            Problem::Lint { lint_rule, file, .. } => {
                ProblemId(format!("{}#{}", lint_rule.0, portable_path(file)))
            }
            Problem::Rule { rulebook_id, rule_id, bad_module, .. } => {
                ProblemId(format!("{rulebook_id}#{rule_id}#{bad_module}"))
            }
        }
    }

    /// Whether `deslop fix` can resolve it unattended. Rule violations never
    /// are: rulebooks describe architecture, not rewrites.
    pub fn is_auto_fixable(&self) -> bool {
        matches!(self, Problem::Lint { auto_fixable: true, .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint() -> Problem {
        Problem::Lint {
            lint_rule: LintRuleId("no-relative-imports".into()),
            file: r"src\a\b.ts".into(),
            code: "import x from '../b'".into(),
            description: "relative import".into(),
            fix: Some("import x from '@/b'".into()),
            auto_fixable: true,
        }
    }

    fn violation() -> Problem {
        Problem::Rule {
            rulebook_id: "clean-architecture".into(),
            rule_id: "domain-is-pure".into(),
            bad_module: "@/domain/user".into(),
            kind: ViolationKind::DirectImport {
                imported: "react".into(),
                import_statement: "import { useState } from 'react';".into(),
            },
            prose: "Domain must be pure".into(),
            fix: "Remove the import".into(),
        }
    }

    #[test]
    fn id_uses_portable_separators() {
        assert_eq!(lint().id(), ProblemId("no-relative-imports#src/a/b.ts".into()));
    }

    #[test]
    fn violation_id_is_rulebook_rule_module() {
        assert_eq!(
            violation().id(),
            ProblemId("clean-architecture#domain-is-pure#@/domain/user".into())
        );
    }

    #[test]
    fn fixability_follows_lint_flag_only() {
        assert!(lint().is_auto_fixable());
        assert!(!violation().is_auto_fixable());
    }
}
