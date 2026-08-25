//! Ports `Deslop/Problem/Shrinker.hs` — collapses the duplicates a run
//! reports into the one Problem worth reading.
//!
//! A module that imports one thing it should not typically drags in a whole
//! subtree of forbidden modules behind it, and every module in that subtree
//! is a transitive violation of the same Rule. They are all repaired by the
//! same single edit, so reporting them one by one buries the edit under its
//! own consequences.
//!
//! Only same-kind duplicates of the same ProblemId are collapsed. A Rule that
//! a module breaks in two different ways — reaching what it must not and
//! failing to reach what it must — still has both reported.

use std::collections::BTreeMap;

use crate::problem::{Problem, ProblemId, ViolationKind};

/// A transitive-import violation with the chains that decide its fate lifted
/// out, so grouping never has to look inside a Problem again. `stands` is
/// every chain the violation speaks for — its own, and any it has already
/// absorbed.
struct Transitive {
    problem: Problem,
    chain: Vec<String>,
    stands: Vec<Vec<String>>,
}

/// One Problem per `(ProblemId, kind)` for transitive imports, every other
/// Problem untouched. Idempotent, and it never drops a ProblemId — so it
/// cannot change what a baseline suppresses.
pub fn compact_problems(problems: Vec<Problem>) -> Vec<Problem> {
    let mut rest = Vec::new();
    let mut grouped: BTreeMap<ProblemId, Vec<Transitive>> = BTreeMap::new();
    for problem in problems {
        match &problem {
            Problem::Rule {
                kind: ViolationKind::TransitiveImport { chain, also_reached, .. },
                ..
            } => {
                let chain = chain.clone();
                let mut stands = vec![chain.clone()];
                stands.extend(also_reached.iter().cloned());
                grouped.entry(problem.id()).or_default().push(Transitive { problem, chain, stands });
            }
            _ => rest.push(problem),
        }
    }
    let mut out = rest;
    for (_, group) in grouped {
        out.push(collapse(group));
    }
    out.sort();
    out
}

/// The shortest chain survives and absorbs what the rest stood for. Ties are
/// broken by the chain itself, which is a total order, so the survivor does
/// not depend on the order the Rules happened to be enforced in.
fn collapse(mut group: Vec<Transitive>) -> Problem {
    group.sort_by(|a, b| (a.chain.len(), &a.chain).cmp(&(b.chain.len(), &b.chain)));
    let mut iter = group.into_iter();
    let winner = iter.next().expect("group is never empty");
    let absorbed: Vec<Vec<String>> = iter.flat_map(|loser| loser.stands).collect();
    absorb(absorbed, winner.problem)
}

/// Records further chains a violation now stands in for, keeping the ones it
/// already did so that compacting an already-compacted report changes
/// nothing. A no-op on anything that is not a transitive import, which
/// grouping never produces.
fn absorb(chains: Vec<Vec<String>>, mut problem: Problem) -> Problem {
    if let Problem::Rule {
        kind: ViolationKind::TransitiveImport { also_reached, .. },
        ..
    } = &mut problem
    {
        also_reached.extend(chains);
    }
    problem
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transitive(chain: &[&str]) -> Problem {
        Problem::Rule {
            rulebook_id: "rb".into(),
            rule_id: "r".into(),
            bad_module: "m".into(),
            prose: String::new(),
            kind: ViolationKind::TransitiveImport {
                chain: chain.iter().map(|s| (*s).to_string()).collect(),
                first_import: None,
                also_reached: Vec::new(),
            },
            fix: String::new(),
        }
    }

    #[test]
    fn duplicates_collapse_into_the_shortest_chain_which_absorbs_the_rest() {
        let problems = vec![
            transitive(&["m", "a", "b", "forbidden"]),
            transitive(&["m", "a", "forbidden"]),
            transitive(&["m", "a", "c", "d", "forbidden"]),
        ];
        let compacted = compact_problems(problems);
        assert_eq!(compacted.len(), 1);
        match &compacted[0] {
            Problem::Rule { kind: ViolationKind::TransitiveImport { chain, also_reached, .. }, .. } => {
                assert_eq!(chain, &["m", "a", "forbidden"].map(String::from));
                let expected: Vec<Vec<String>> = vec![
                    ["m", "a", "b", "forbidden"].iter().map(|s| s.to_string()).collect(),
                    ["m", "a", "c", "d", "forbidden"].iter().map(|s| s.to_string()).collect(),
                ];
                assert_eq!(also_reached, &expected);
            }
            other => panic!("expected transitive violation, got {other:?}"),
        }
    }

    #[test]
    fn same_length_ties_break_on_the_chain_itself() {
        let problems = vec![
            transitive(&["m", "z", "forbidden"]),
            transitive(&["m", "a", "forbidden"]),
        ];
        let compacted = compact_problems(problems);
        match &compacted[0] {
            Problem::Rule { kind: ViolationKind::TransitiveImport { chain, .. }, .. } => {
                assert_eq!(chain, &["m", "a", "forbidden"].map(String::from));
            }
            other => panic!("expected transitive violation, got {other:?}"),
        }
    }

    #[test]
    fn other_problems_pass_through_and_survive_idempotence() {
        let lint = Problem::Lint {
            lint_rule: crate::problem::LintRuleId("no-relative-imports".into()),
            file: "src/a.ts".into(),
            code: "i".into(),
            description: "d".into(),
            fix: None,
            auto_fixable: false,
        };
        let once = compact_problems(vec![lint.clone(), transitive(&["m", "x"])]);
        assert_eq!(once.len(), 2);
        let twice = compact_problems(once.clone());
        assert_eq!(once, twice);
    }
}
