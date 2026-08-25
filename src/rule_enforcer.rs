//! Ports `Deslop/Rule/Enforcer.hs` — walks every module's imports against
//! every compiled rulebook rule and emits `Problem::Rule` violations with
//! `ViolationKind` payloads.

use crate::ast::AstModule;
use crate::code_graph::{find_known_path, module_exists, reachable_from, CodeGraph};
use crate::glob_plus::{
    hydrate, interpolate, match_exclude, match_resolved, match_target, module_from_glob,
    render_clause_pattern, segments_of, MatchEnv, ResolvedClause, Segments,
};
use crate::problem::{Problem, ViolationKind};
use crate::rulebook::{ForbidsClause, Rule, Rulebook, UsesClause};
use crate::types::DeslopError;

/// The rule's own prose speaks about the match that violated it, so the
/// variables its target captured are substituted into it before it is
/// reported.
/// A rule whose target matched, with the environment the match captured —
/// everything a clause needs to know about why it is running.
struct MatchedRule<'a> {
    rulebook_id: &'a str,
    rule: &'a Rule,
    env: &'a MatchEnv,
}

fn rule_violation(matched: &MatchedRule<'_>, bad_module: &str, kind: ViolationKind) -> Problem {
    let rule = matched.rule;
    Problem::Rule {
        rulebook_id: matched.rulebook_id.to_string(),
        rule_id: rule.id.clone(),
        bad_module: bad_module.to_string(),
        prose: interpolate(matched.env, &rule.description),
        kind,
        fix: interpolate(matched.env, &rule.fix),
    }
}

/// Every path this module will be tested against, each split into segments
/// exactly once. A module id is matched against every rule and every clause,
/// so taking it apart per match is work done as many times as there are
/// clauses.
struct Candidates<'a> {
    self_segments: Segments,
    imports: Vec<(&'a crate::ast::ImportNode, Segments)>,
    reachable: Vec<(String, Segments)>,
}

pub fn enforce_rulebooks(
    m: &AstModule,
    graph: &CodeGraph,
    rulebooks: &[Rulebook],
) -> Result<Vec<Problem>, DeslopError> {
    let candidates = Candidates {
        self_segments: segments_of(&m.id),
        imports: m.nodes.iter().map(|node| (node, segments_of(&node.target))).collect(),
        reachable: reachable_from(graph, &m.id)
            .into_iter()
            .map(|id| (id.clone(), segments_of(&id)))
            .collect(),
    };
    let mut problems = Vec::new();
    for rulebook in rulebooks {
        for rule in &rulebook.rules {
            enforce_rule(m, graph, rulebook.id.as_str(), rule, &candidates, &mut problems)?;
        }
    }
    Ok(problems)
}

fn enforce_rule(
    m: &AstModule,
    graph: &CodeGraph,
    rulebook_id: &str,
    rule: &Rule,
    candidates: &Candidates<'_>,
    problems: &mut Vec<Problem>,
) -> Result<(), DeslopError> {
    if let Some(env) = is_target(&candidates.self_segments, rule) {
        let matched = MatchedRule { rulebook_id, rule, env: &env };
        for clause in &rule.forbids {
            enforce_forbids(m, graph, &matched, clause, candidates, problems);
        }
        for clause in &rule.exists {
            enforce_exists(m, graph, &matched, clause, problems)?;
        }
        for clause in &rule.uses {
            enforce_uses(m, &matched, clause, candidates, problems);
        }
    }
    Ok(())
}

fn is_target(module_segments: &Segments, rule: &Rule) -> Option<MatchEnv> {
    let env = match_target(&rule.target, module_segments)?;
    let excluded = rule.exclude.iter().any(|pattern| match_exclude(pattern, module_segments));
    (!excluded).then_some(env)
}

/// Clauses are hydrated once per matched target and then run against every
/// candidate path, rather than resolved afresh for each one.
fn enforce_forbids(
    m: &AstModule,
    graph: &CodeGraph,
    matched: &MatchedRule<'_>,
    clause: &ForbidsClause,
    candidates: &Candidates<'_>,
    problems: &mut Vec<Problem>,
) {
    let allowed: Vec<ResolvedClause> = matched
        .rule
        .allows
        .iter()
        .map(|allows| hydrate(matched.env, &allows.target))
        .collect();
    let forbidden = hydrate(matched.env, &clause.target);
    let is_allowed = |segments: &Segments| allowed.iter().any(|a| match_resolved(a, segments));

    if clause.transitive {
        for (id, segments) in &candidates.reachable {
            if match_resolved(&forbidden, segments) && !is_allowed(segments) {
                let Some(chain) = find_known_path(graph, &m.id, id) else { continue };
                let first_import = chain.get(1).and_then(|hop| {
                    m.nodes
                        .iter()
                        .find(|node| node.target == *hop)
                        .map(|node| node.raw_statement.trim().to_string())
                });
                problems.push(rule_violation(
                    matched,
                    &m.id,
                    ViolationKind::TransitiveImport { chain, first_import, also_reached: Vec::new() },
                ));
            }
        }
    } else {
        for (node, segments) in &candidates.imports {
            if match_resolved(&forbidden, segments) && !is_allowed(segments) {
                problems.push(rule_violation(
                    matched,
                    &m.id,
                    ViolationKind::DirectImport {
                        imported: node.target.clone(),
                        import_statement: node.raw_statement.trim().to_string(),
                    },
                ));
            }
        }
    }
}

fn enforce_uses(
    m: &AstModule,
    matched: &MatchedRule<'_>,
    clause: &UsesClause,
    candidates: &Candidates<'_>,
    problems: &mut Vec<Problem>,
) {
    let required = hydrate(matched.env, &clause.target);
    let satisfied = if clause.transitive {
        candidates.reachable.iter().any(|(_, segments)| match_resolved(&required, segments))
    } else {
        candidates.imports.iter().any(|(_, segments)| match_resolved(&required, segments))
    };
    if !satisfied {
        problems.push(rule_violation(
            matched,
            &m.id,
            ViolationKind::MissingUse {
                required_import: render_clause_pattern(matched.env, &clause.target),
                transitive: clause.transitive,
            },
        ));
    }
}

fn enforce_exists(
    m: &AstModule,
    graph: &CodeGraph,
    matched: &MatchedRule<'_>,
    clause: &crate::rulebook::ExistsClause,
    problems: &mut Vec<Problem>,
) -> Result<(), DeslopError> {
    let Some(required) = module_from_glob(matched.env, &clause.target) else {
        return Err(DeslopError::InvalidRuleConfig(format!(
            "Rule '{}' in rulebook '{}': 'exists' patterns must not contain wildcards (* or **).",
            matched.rule.id, matched.rulebook_id
        )));
    };
    if !module_exists(graph, &required) {
        problems.push(rule_violation(
            matched,
            &m.id,
            ViolationKind::MissingModule { required_module: required },
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AstModule, ImportNode};
    use crate::rulebook::compiler::compile_rulebook;
    use crate::rulebook::dto::{AllowsDto, ExistsDto, ForbidsDto, RuleDto, RulebookDto, UsesDto};

    fn module(id: &str, imports: &[(&str, &str)]) -> AstModule {
        AstModule {
            id: id.to_string(),
            path: format!("/{id}.ts"),
            nodes: imports
                .iter()
                .map(|(target, raw)| ImportNode {
                    target: (*target).to_string(),
                    raw_statement: (*raw).to_string(),
                })
                .collect(),
        }
    }

    fn rule_dto(id: &str, target: &str) -> RuleDto {
        RuleDto {
            id: id.into(),
            description: String::new(),
            target: target.into(),
            exclude: Vec::new(),
            forbids: Vec::new(),
            allows: Vec::new(),
            uses: Vec::new(),
            exists: Vec::new(),
            example: None,
            fix: String::new(),
        }
    }

    fn compile(rules: Vec<RuleDto>) -> Vec<Rulebook> {
        vec![compile_rulebook(RulebookDto {
            id: "clean-architecture".into(),
            name: "Clean Architecture".into(),
            description: String::new(),
            rules,
        })
        .unwrap()]
    }

    fn enforce_all(asts: &[AstModule], rulebooks: &[Rulebook]) -> Result<Vec<Problem>, DeslopError> {
        let graph = crate::code_graph::build_module_graph(asts);
        let mut problems = Vec::new();
        for m in asts {
            problems.extend(enforce_rulebooks(m, &graph, rulebooks)?);
        }
        Ok(problems)
    }

    fn sample_project() -> Vec<AstModule> {
        vec![
            module(
                "@/app/login",
                &[
                    ("react", "import React from 'react';"),
                    ("@/ui/button", "import { Button } from '@/ui/button';"),
                ],
            ),
            module(
                "@/ui/button",
                &[
                    ("react", "import React from 'react';"),
                    ("@/domain/user", "import { User } from '@/domain/user';"),
                ],
            ),
            module("@/domain/user", &[("react", "  import React from 'react';  ")]),
        ]
    }

    #[test]
    fn direct_forbids_reports_the_imported_module_and_its_statement() {
        let mut rule = rule_dto("domain-is-pure", "@/domain/**");
        rule.description = "The domain imports only itself.".into();
        rule.forbids = vec![ForbidsDto { import: "**".into(), transitive: None }];
        rule.allows = vec![AllowsDto { import: "@/domain/**".into() }];
        rule.fix = "Remove the import.".into();
        let rulebooks = compile(vec![rule]);

        let problems = enforce_all(&sample_project(), &rulebooks).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        let Problem::Rule { rule_id, bad_module, prose, kind, .. } = &problems[0] else {
            panic!("expected rule violation");
        };
        assert_eq!((rule_id.as_str(), bad_module.as_str()), ("domain-is-pure", "@/domain/user"));
        assert_eq!(prose, "The domain imports only itself.");
        match kind {
            ViolationKind::DirectImport { imported, import_statement } => {
                assert_eq!(imported, "react");
                // The raw statement is trimmed before it is reported.
                assert_eq!(import_statement, "import React from 'react';");
            }
            other => panic!("expected DirectImport, got {other:?}"),
        }
    }

    #[test]
    fn allows_exempts_what_it_hydrates() {
        let mut rule = rule_dto("ui-is-clean", "@/ui/**");
        rule.forbids = vec![ForbidsDto { import: "**".into(), transitive: None }];
        rule.allows = vec![
            AllowsDto { import: "react".into() },
            AllowsDto { import: "@/domain/**".into() },
        ];
        let rulebooks = compile(vec![rule]);

        // @/ui/button imports react and @/domain/user, both allowed.
        assert!(enforce_all(&sample_project(), &rulebooks).unwrap().is_empty());
    }

    #[test]
    fn transitive_forbids_reports_the_chain_and_its_first_import() {
        let mut rule = rule_dto("app-never-reaches-domain", "@/app/**");
        rule.forbids = vec![ForbidsDto { import: "@/domain/**".into(), transitive: Some(true) }];
        let rulebooks = compile(vec![rule]);

        let problems = enforce_all(&sample_project(), &rulebooks).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        match &problems[0] {
            Problem::Rule { rule_id, kind, .. } => {
                assert_eq!(rule_id, "app-never-reaches-domain");
                match kind {
                    ViolationKind::TransitiveImport { chain, first_import, also_reached } => {
                        assert_eq!(
                            chain,
                            &vec![
                                "@/app/login".to_string(),
                                "@/ui/button".to_string(),
                                "@/domain/user".to_string()
                            ]
                        );
                        assert_eq!(first_import.as_deref(), Some("import { Button } from '@/ui/button';"));
                        assert!(also_reached.is_empty());
                    }
                    other => panic!("expected TransitiveImport, got {other:?}"),
                }
            }
            other => panic!("expected rule violation, got {other:?}"),
        }
    }

    #[test]
    fn missing_use_direct_and_transitive() {
        // @/ui/button imports @/domain/user directly: satisfied.
        let mut rule = rule_dto("ui-uses-domain", "@/ui/**");
        rule.uses = vec![UsesDto { import: "@/domain/**".into(), transitive: None }];
        let rulebooks = compile(vec![rule]);
        assert!(enforce_all(&sample_project(), &rulebooks).unwrap().is_empty());

        // Transitive use: @/app/login reaches the domain through the UI.
        let mut rule = rule_dto("app-touches-domain", "@/app/**");
        rule.uses = vec![UsesDto { import: "@/domain/**".into(), transitive: Some(true) }];
        let rulebooks = compile(vec![rule]);
        assert!(enforce_all(&sample_project(), &rulebooks).unwrap().is_empty());

        // Nothing anywhere imports @/infrastructure/db.
        let mut rule = rule_dto("app-touches-db", "@/app/**");
        rule.uses = vec![UsesDto { import: "@/infrastructure/**".into(), transitive: Some(true) }];
        let rulebooks = compile(vec![rule]);
        let problems = enforce_all(&sample_project(), &rulebooks).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        match &problems[0] {
            Problem::Rule { kind: ViolationKind::MissingUse { required_import, transitive }, .. } => {
                assert_eq!(required_import, "@/infrastructure/**");
                assert!(transitive);
            }
            other => panic!("expected MissingUse, got {other:?}"),
        }
    }

    #[test]
    fn missing_module_when_exists_target_is_absent() {
        let mut rule = rule_dto("domain-has-entities", "@/domain/**");
        rule.exists = vec![ExistsDto { module: "@/domain/entities".into() }];
        let rulebooks = compile(vec![rule]);

        let problems = enforce_all(&sample_project(), &rulebooks).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        match &problems[0] {
            Problem::Rule { kind: ViolationKind::MissingModule { required_module }, .. } => {
                assert_eq!(required_module, "@/domain/entities");
            }
            other => panic!("expected MissingModule, got {other:?}"),
        }

        // The module exists as a graph vertex once something imports it.
        let mut rule = rule_dto("app-needs-button", "@/app/**");
        rule.exists = vec![ExistsDto { module: "@/ui/button".into() }];
        let rulebooks = compile(vec![rule]);
        assert!(enforce_all(&sample_project(), &rulebooks).unwrap().is_empty());
    }

    #[test]
    fn wildcard_exists_is_invalid_rule_config() {
        let mut rule = rule_dto("bad-exists", "@/domain/**");
        rule.exists = vec![ExistsDto { module: "@/domain/**".into() }];
        let rulebooks = compile(vec![rule]);

        let err = enforce_all(&sample_project(), &rulebooks).unwrap_err();
        assert!(
            err.to_string().contains(
                "Rule 'bad-exists' in rulebook 'clean-architecture': 'exists' patterns must not contain wildcards"
            ),
            "{err}"
        );
    }

    #[test]
    fn excluded_targets_are_not_governed() {
        let mut rule = rule_dto("domain-is-pure", "@/domain/**");
        rule.exclude = vec!["**/user".into()];
        rule.forbids = vec![ForbidsDto { import: "**".into(), transitive: None }];
        let rulebooks = compile(vec![rule]);

        assert!(enforce_all(&sample_project(), &rulebooks).unwrap().is_empty());
    }

    #[test]
    fn target_variables_are_substituted_into_prose_and_fix() {
        let mut rule = rule_dto("feature-isolation", "@/features/{{feature-name}}/**");
        rule.description = "The {{feature-name}} feature must not reach the shared lib.".into();
        rule.fix = "Move the import out of {{FEATURE_NAME}}.".into();
        rule.forbids = vec![ForbidsDto { import: "@/lib/{{feature-name}}".into(), transitive: None }];
        let rulebooks = compile(vec![rule]);

        let asts = vec![module(
            "@/features/auth/login-page",
            &[("@/lib/auth", "import { x } from '@/lib/auth';")],
        )];
        let problems = enforce_all(&asts, &rulebooks).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        let Problem::Rule { prose, fix, .. } = &problems[0] else {
            panic!("expected rule violation");
        };
        assert_eq!(prose, "The auth feature must not reach the shared lib.");
        assert_eq!(fix, "Move the import out of AUTH.");
    }
}
