//! Ports `Deslop/Problem/Formatter.hs` — one Problem as text.

use crate::problem::{Problem, ViolationKind};
use crate::utils::pluralise;

pub fn format_problem(p: &Problem) -> String {
    match p {
        Problem::Lint { .. } => lint_text(p),
        Problem::Rule { .. } => rule_text(p),
    }
}

fn lint_text(p: &Problem) -> String {
    let Problem::Lint { code, description, fix, auto_fixable, .. } = p else {
        unreachable!("dispatch checked");
    };
    let auto_fix = if *auto_fixable { "[AUTO-FIXABLE] " } else { "" };
    format!(
        "{auto_fix}# {}\n{}\n```ts\n{}\n```\nFIX: {}",
        p.id().0,
        description,
        code.trim(),
        fix.as_deref().unwrap_or("").trim()
    )
}

fn rule_text(p: &Problem) -> String {
    let Problem::Rule { bad_module, prose, kind, fix, .. } = p else {
        unreachable!("dispatch checked");
    };
    format!("# {}\n{}\n\n{}\nFIX: {}", p.id().0, prose, violation(bad_module, kind), fix.trim())
}

/// What the module did, in the Rule's own terms. Every sentence names the
/// module even though the header above it already does, so that a violation
/// quoted on its own still says who broke the Rule.
fn violation(bad_module: &str, kind: &ViolationKind) -> String {
    match kind {
        ViolationKind::DirectImport { imported, import_statement } => format!(
            "Module '{bad_module}' directly imports '{imported}'.{}",
            code_block(import_statement)
        ),
        ViolationKind::TransitiveImport { chain, first_import, also_reached } => {
            let last = chain.last().map(String::as_str).unwrap_or_default();
            let via = chain.join(" → ");
            let first = first_import.as_deref().map(code_block).unwrap_or_default();
            format!(
                "Module '{bad_module}' transitively imports '{last}' ({}) via: {via}.{first}{}",
                pluralise(chain.len().saturating_sub(1), "hop"),
                absorbed(first_hop(chain), also_reached)
            )
        }
        ViolationKind::MissingUse { required_import, transitive } => {
            let verb = if *transitive { "transitively import '" } else { "import '" };
            format!("Module '{bad_module}' must {verb}{required_import}'.")
        }
        ViolationKind::MissingModule { required_module } => {
            format!("Module '{bad_module}' requires '{required_module}' to exist.")
        }
    }
}

/// What the compacted duplicates would have said. Their forbidden modules are
/// left out on purpose — they are what made the un-compacted report
/// unreadable. What the reader still has to act on is the set of imports at
/// fault, so any hop other than the one already shown above is named.
fn absorbed(shown_hop: Option<&String>, chains: &[Vec<String>]) -> String {
    if chains.is_empty() {
        return String::new();
    }
    // ordNub: first occurrence order, no duplicates.
    let mut hops: Vec<&String> = Vec::new();
    for chain in chains {
        if let Some(hop) = first_hop(chain) {
            if Some(hop) != shown_hop && !hops.contains(&hop) {
                hops.push(hop);
            }
        }
    }
    let others = match hops.as_slice() {
        [] => " through this import".to_string(),
        [one] => format!(", through the import of '{one}'"),
        many => format!(
            ", through the imports of {}",
            many.iter().map(|hop| format!("'{hop}'")).collect::<Vec<_>>().join(", ")
        ),
    };
    format!(
        "\nAlso reaches {}{}.",
        pluralise(chains.len(), "more forbidden module"),
        others
    )
}

/// The import that opens a chain. Absent when the chain never leaves the
/// module.
fn first_hop(chain: &[String]) -> Option<&String> {
    chain.get(1)
}

fn code_block(statement: &str) -> String {
    format!("\n```ts\n{statement}\n```")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint() -> Problem {
        Problem::Lint {
            lint_rule: crate::problem::LintRuleId("no-relative-imports".into()),
            file: "src/ui/button.ts".into(),
            code: "  import { User } from '../domain/user';  \n".into(),
            description: "Relative imports are not allowed. Use aliased ones.".into(),
            fix: Some("Use ```import { User } from '@/domain/user';``` instead.".into()),
            auto_fixable: true,
        }
    }

    fn rule(kind: ViolationKind) -> Problem {
        Problem::Rule {
            rulebook_id: "clean".into(),
            rule_id: "domain-is-pure".into(),
            bad_module: "@/domain/user".into(),
            prose: "The domain imports only itself.".into(),
            kind,
            fix: "Remove the import.  \n".into(),
        }
    }

    #[test]
    fn lint_golden() {
        assert_eq!(
            format_problem(&lint()),
            "[AUTO-FIXABLE] # no-relative-imports#src/ui/button.ts\n\
             Relative imports are not allowed. Use aliased ones.\n\
             ```ts\nimport { User } from '../domain/user';\n```\n\
             FIX: Use ```import { User } from '@/domain/user';``` instead."
        );
    }

    #[test]
    fn direct_import_golden() {
        let p = rule(ViolationKind::DirectImport {
            imported: "react".into(),
            import_statement: "import React from 'react';".into(),
        });
        assert_eq!(
            format_problem(&p),
            "# clean#domain-is-pure#@/domain/user\n\
             The domain imports only itself.\n\
             \n\
             Module '@/domain/user' directly imports 'react'.\n\
             ```ts\nimport React from 'react';\n```\n\
             FIX: Remove the import."
        );
    }

    #[test]
    fn transitive_import_golden_with_absorbed_chains() {
        let p = rule(ViolationKind::TransitiveImport {
            chain: vec!["@/app".into(), "@/ui".into(), "@/domain".into()],
            first_import: Some("import { Button } from '@/ui/button';".into()),
            also_reached: vec![
                vec!["@/app".into(), "@/ui".into(), "@/icons".into(), "@/domain".into()],
                vec!["@/app".into(), "@/lib".into(), "@/domain".into()],
            ],
        });
        assert_eq!(
            format_problem(&p),
            "# clean#domain-is-pure#@/domain/user\n\
             The domain imports only itself.\n\
             \n\
             Module '@/domain/user' transitively imports '@/domain' (2 hops) via: @/app → @/ui → @/domain.\n\
             ```ts\nimport { Button } from '@/ui/button';\n```\n\
             Also reaches 2 more forbidden modules, through the import of '@/lib'.\n\
             FIX: Remove the import."
        );
    }

    #[test]
    fn missing_use_and_module_goldens() {
        let missing_use = rule(ViolationKind::MissingUse {
            required_import: "@/domain/**".into(),
            transitive: true,
        });
        assert!(format_problem(&missing_use)
            .contains("Module '@/domain/user' must transitively import '@/domain/**'."));

        let missing_module =
            rule(ViolationKind::MissingModule { required_module: "@/domain/entities".into() });
        assert!(format_problem(&missing_module)
            .contains("Module '@/domain/user' requires '@/domain/entities' to exist."));
    }
}
