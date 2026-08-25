//! Ports `Deslop/Rule/Book/Compiler.hs` — turning a raw `RulebookDto` into a
//! valid `Rulebook`, or into every reason it cannot be one.
//!
//! Errors accumulate: a rulebook with five broken patterns reports five,
//! because fixing a rulebook one error per run is a poor way to spend an
//! afternoon. The one place accumulation stops is inside a rule, and for a
//! reason: a clause compiles against the variables its target binds, so a
//! rule whose target failed has no scope to check its clauses against. Such a
//! rule reports its target and its excludes — which bind nothing and so need
//! no scope — and stays quiet about its clauses rather than blaming each of
//! them for a variable the target never got to define.
//!
//! Polarity is fixed here, at the four clause sites, and is never a caller's
//! choice.

use crate::glob_plus::compiler::{
    compile_clause_pattern, compile_exclude_pattern, compile_target_pattern, render_error,
    GlobPlusError,
};
use crate::glob_plus::{CompiledClausePattern, Polarity};

use super::book::{AllowsClause, ExistsClause, ForbidsClause, Rule, Rulebook, UsesClause};
use super::dto::{RuleDto, RulebookDto};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Which field of a rule a pattern came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Target,
    Exclude,
    Forbids,
    Allows,
    Uses,
    Exists,
}

fn field_name(field: Field) -> &'static str {
    match field {
        Field::Target => "target",
        Field::Exclude => "exclude",
        Field::Forbids => "forbids.import",
        Field::Allows => "allows.import",
        Field::Uses => "uses.import",
        Field::Exists => "exists.module",
    }
}

/// One reason a raw rulebook cannot become a `Rulebook`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub rule: String,
    pub field: Field,
    pub glob: String,
    pub cause: GlobPlusError,
}

/// One error, indented under the rule and field it came from. The file
/// heading is added by whoever knows the file name — see `loader`.
pub fn render_compile_error(err: &CompileError) -> String {
    let cause = render_error(&err.cause);
    let indented: Vec<String> = cause.lines().map(|line| format!("      {line}")).collect();
    format!(
        "  rule '{}'\n    {}: \"{}\"\n{}",
        err.rule,
        field_name(err.field),
        err.glob,
        indented.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Compiling
// ---------------------------------------------------------------------------

pub fn compile_rulebook(dto: RulebookDto) -> Result<Rulebook, Vec<CompileError>> {
    let mut errors = Vec::new();
    let rules: Vec<Rule> =
        dto.rules.into_iter().filter_map(|rule| compile_rule(rule, &mut errors)).collect();
    if errors.is_empty() {
        Ok(Rulebook { id: dto.id, name: dto.name, description: dto.description, rules })
    } else {
        Err(errors)
    }
}

/// The target compiles first, because the variables it binds are the scope
/// its clauses compile in. That dependency is the one place a rule cannot
/// accumulate, so it is written as a match rather than folded into the
/// shared error list.
fn compile_rule(dto: RuleDto, errors: &mut Vec<CompileError>) -> Option<Rule> {
    let RuleDto {
        id,
        description,
        target: target_glob,
        exclude,
        forbids,
        allows,
        uses,
        exists,
        example,
        fix,
    } = dto;

    let target = match compile_target_pattern(&target_glob) {
        Ok(target) => target,
        Err(cause) => {
            errors.push(CompileError {
                rule: id.clone(),
                field: Field::Target,
                glob: target_glob,
                cause,
            });
            // Excludes bind nothing, so they need no scope the target failed
            // to provide; the clauses stay quiet, having none to check in.
            for glob in exclude {
                if let Err(cause) = compile_exclude_pattern(&glob) {
                    errors.push(CompileError {
                        rule: id.clone(),
                        field: Field::Exclude,
                        glob,
                        cause,
                    });
                }
            }
            return None;
        }
    };

    let before = errors.len();
    let exclude = compile_all(Field::Exclude, &id, exclude, compile_exclude_pattern, errors);
    let bound = target.bound_vars.clone();

    let forbids = compile_clauses(
        &id,
        Field::Forbids,
        Polarity::Widen,
        forbids.into_iter().map(|c| (c.import, c.transitive)),
        &bound,
        errors,
        |transitive| transitive.unwrap_or(false),
    );
    let forbids: Vec<ForbidsClause> =
        forbids.into_iter().map(|(target, transitive)| ForbidsClause { target, transitive }).collect();
    let allows = compile_clauses(
        &id,
        Field::Allows,
        Polarity::Narrow,
        allows.into_iter().map(|c| (c.import, None)),
        &bound,
        errors,
        |_| (),
    );
    let allows: Vec<AllowsClause> = allows.into_iter().map(|(target, ())| AllowsClause { target }).collect();
    let uses = compile_clauses(
        &id,
        Field::Uses,
        Polarity::Narrow,
        uses.into_iter().map(|c| (c.import, c.transitive)),
        &bound,
        errors,
        |transitive| transitive.unwrap_or(false),
    );
    let uses: Vec<UsesClause> =
        uses.into_iter().map(|(target, transitive)| UsesClause { target, transitive }).collect();
    let exists = compile_clauses(
        &id,
        Field::Exists,
        Polarity::Narrow,
        exists.into_iter().map(|c| (c.module, None)),
        &bound,
        errors,
        |_| (),
    );
    let exists: Vec<ExistsClause> = exists.into_iter().map(|(target, ())| ExistsClause { target }).collect();

    if errors.len() > before {
        return None;
    }
    Some(Rule {
        id,
        description,
        target,
        exclude,
        forbids,
        allows,
        uses,
        exists,
        example,
        fix,
    })
}

fn compile_all<T, F>(
    field: Field,
    rule_id: &str,
    globs: Vec<String>,
    compile: F,
    errors: &mut Vec<CompileError>,
) -> Vec<T>
where
    F: Fn(&str) -> Result<T, GlobPlusError>,
{
    globs
        .into_iter()
        .filter_map(|glob| match compile(&glob) {
            Ok(compiled) => Some(compiled),
            Err(cause) => {
                errors.push(CompileError { rule: rule_id.to_string(), field, glob, cause });
                None
            }
        })
        .collect()
}

/// Compiles one field's clause patterns, pairing each with the flag its DTO
/// carried. Patterns that fail drop out; the rule is discarded by the caller
/// in that case, so the shorter list is never observed.
fn compile_clauses<T>(
    rule_id: &str,
    field: Field,
    polarity: Polarity,
    globs: impl IntoIterator<Item = (String, Option<bool>)>,
    bound: &[String],
    errors: &mut Vec<CompileError>,
    flag_of: impl Fn(Option<bool>) -> T,
) -> Vec<(CompiledClausePattern, T)> {
    globs
        .into_iter()
        .filter_map(|(glob, transitive)| {
            match compile_clause_pattern(polarity, bound, &glob) {
                Ok(target) => Some((target, flag_of(transitive))),
                Err(cause) => {
                    errors.push(CompileError { rule: rule_id.to_string(), field, glob, cause });
                    None
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rulebook::dto::{AllowsDto, ExistsDto, ForbidsDto, RulebookDto, UsesDto};

    fn dto(id: &str, target: &str) -> RuleDto {
        RuleDto {
            id: id.to_string(),
            description: String::new(),
            target: target.to_string(),
            exclude: Vec::new(),
            forbids: Vec::new(),
            allows: Vec::new(),
            uses: Vec::new(),
            exists: Vec::new(),
            example: None,
            fix: String::new(),
        }
    }

    fn book(rules: Vec<RuleDto>) -> Result<Rulebook, Vec<CompileError>> {
        compile_rulebook(RulebookDto {
            id: "test".into(),
            name: "Test".into(),
            description: String::new(),
            rules,
        })
    }

    #[test]
    fn compiles_clauses_against_the_targets_bindings() {
        let mut rule = dto("layered", "@/features/{{feature-name}}/**");
        rule.forbids = vec![ForbidsDto { import: "@/other/{{feature-name}}".into(), transitive: Some(true) }];
        rule.allows = vec![AllowsDto { import: "@/shared/**".into() }];
        rule.uses = vec![UsesDto { import: "@/core".into(), transitive: None }];
        rule.exists = vec![ExistsDto { module: "@/core".into() }];
        rule.fix = "Move it.".into();

        let compiled = book(vec![rule]).unwrap();
        assert_eq!(compiled.rules.len(), 1);
        let rule = &compiled.rules[0];
        assert!(rule.forbids[0].transitive);
        assert!(!rule.uses[0].transitive);
        assert_eq!(rule.target.bound_vars, ["feature-name"]);
    }

    #[test]
    fn errors_accumulate_across_rules_and_fields() {
        let mut first = dto("first", "@/a/**");
        first.forbids = vec![ForbidsDto { import: "{{missing-name}}".into(), transitive: None }];
        first.allows = vec![AllowsDto { import: "{{gone-name}}".into() }];
        let second = dto("second", "@/b/..");

        let errors = book(vec![first, second]).unwrap_err();
        // first: unbound clause variable + .. in an allow; second: .. in target.
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert_eq!(errors[0].rule, "first");
        assert_eq!(errors[0].field, Field::Forbids);
        assert_eq!(errors[1].rule, "first");
        assert_eq!(errors[1].field, Field::Allows);
        assert_eq!(errors[2].rule, "second");
        assert_eq!(errors[2].field, Field::Target);
    }

    #[test]
    fn a_failed_target_silences_its_clauses_but_not_its_excludes() {
        let mut rule = dto("broken", "@/a/..");
        rule.exclude = vec!["**".into(), "{{ghost-name}}".into(), "..".into()];
        rule.forbids = vec![ForbidsDto { import: "{{also-unbound}}".into(), transitive: None }];

        let errors = book(vec![rule]).unwrap_err();
        // The target, plus both excludes; the forbids clause stays quiet.
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors.iter().all(|e| e.field != Field::Forbids));
        assert_eq!(errors[0].field, Field::Target);
        assert_eq!(errors[1].field, Field::Exclude);
        assert_eq!(errors[2].field, Field::Exclude);
    }

    #[test]
    fn render_places_each_error_under_its_rule_and_field() {
        let err = CompileError {
            rule: "layered".into(),
            field: Field::Uses,
            glob: "{{ghost}}".into(),
            cause: GlobPlusError::UnboundVariable { name: "ghost".into(), bound: vec![] },
        };
        let rendered = render_compile_error(&err);
        assert!(rendered.starts_with("  rule 'layered'\n    uses.import: \"{{ghost}}\"\n"), "{rendered}");
        assert!(rendered.contains("\n      unknown variable {{ghost}}."), "{rendered}");
    }
}
