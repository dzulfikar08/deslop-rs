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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintRuleId(pub String);

/// A rulebook rule that was broken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleViolationKind;

// TODO(port): ViolationKind (DirectImport / TransitiveImport / MissingUse /
// MissingModule) once RuleEnforcer + ModuleId land.

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Problem {
    Lint {
        lint_rule: LintRuleId,
        file: String,
        code: String,
        description: String,
        fix: Option<String>,
        auto_fixable: bool,
    },
    // TODO(port): Rule { .. } variant with ViolationKind payload.
    #[allow(unused)]
    Rule {
        rulebook_id: String,
        rule_id: String,
        bad_module: String,
        kind: RuleViolationKind,
        prose: String,
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
            file: r#"src\a\b.ts"#.into(),
            code: "import x from '../b'".into(),
            description: "relative import".into(),
            fix: Some("import x from '@/b'".into()),
            auto_fixable: true,
        }
    }

    #[test]
    fn id_uses_portable_separators() {
        assert_eq!(lint().id(), ProblemId("no-relative-imports#src/a/b.ts".into()));
    }

    #[test]
    fn fixability_follows_lint_flag_only() {
        assert!(lint().is_auto_fixable());
    }
}
