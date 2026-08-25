//! Glob+ patterns, and the matcher that runs them.
//!
//! Port of `Deslop/GlobPlus.hs`. A Glob+ pattern is a list of path segments.
//! Exactly one token, `**`, varies how many segments the pattern consumes;
//! everything else consumes one segment, or part of one.

pub mod compiler;
#[cfg(test)]
mod tests;

pub use compiler::{
    compile_clause_pattern, compile_exclude_pattern, compile_target_pattern, interpolate,
    GlobPlusError,
};

use crate::casing::{agree, render, AgreedName, Casing};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// 1. Names
// ---------------------------------------------------------------------------

/// The identity of a variable, canonicalised to kebab-case words.
pub type VarName = String;

/// One value, available in every casing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasedName {
    pub pascal: String,
    pub camel: String,
    pub kebab: String,
    pub constant: String,
}

/// A bound variable: the name its occurrences agreed on, written out in every
/// casing, plus every name they could equally have denoted (for Widen clauses).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundName {
    pub spelling: CasedName,
    /// Every surviving candidate name, shortest first.
    pub candidates: Vec<Vec<String>>,
}

fn cased_as(casing: Casing, n: &BoundName) -> String {
    match casing {
        Casing::Pascal => n.spelling.pascal.clone(),
        Casing::Camel => n.spelling.camel.clone(),
        Casing::Kebab => n.spelling.kebab.clone(),
        Casing::Constant => n.spelling.constant.clone(),
    }
}

// ---------------------------------------------------------------------------
// 2. Pattern structure
// ---------------------------------------------------------------------------

/// One piece of a pattern, at the level where a globstar lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg<A> {
    GlobStar,
    Segment(A),
}

/// One piece of a single segment. May be empty (the empty segment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegPart<V> {
    Lit(String),
    AnyChars,
    Var(V),
}

/// A variable occurrence in a target pattern. Strictly no TARGET_DIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetVar {
    pub name: VarName,
    pub casing: Casing,
}

/// A variable occurrence in a clause pattern; may be the TARGET_DIR keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClauseVar {
    Var { name: VarName, casing: Casing },
    TargetDir,
}

/// Which way it is safe to be wrong when a variable's spelling is guessed:
/// Widen accepts every spelling (target/forbids), Narrow only the canonical
/// one (uses/exists/allows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    Widen,
    Narrow,
}

// ---------------------------------------------------------------------------
// 3. Compiled patterns
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledTargetPattern {
    pub segments: Vec<Seg<Vec<SegPart<TargetVar>>>>,
    pub min_length: usize,
    pub bound_vars: Vec<VarName>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledClausePattern {
    pub steps: Vec<Step<PatternSegment<ClauseVar>>>,
    pub polarity: Polarity,
    pub source: String,
}

/// An exclude pattern is a plain glob over module ids: no variables, no `..`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledExcludePattern {
    pub segments: Vec<Seg<Vec<SegPart<std::convert::Infallible>>>>,
    pub min_length: usize,
    pub source: String,
}

pub type PatternSegment<V> = Seg<Vec<SegPart<V>>>;

/// A clause step. `..` consumes no path — it edits the list before matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<A> {
    ParentDir,
    Step(A),
}

/// Expands each step and cancels each `..` against what came before. A `..`
/// with nothing left to cancel does nothing, as `/..` is `/` on a Unix path.
pub fn resolve_steps<A, B, F>(expand: F, steps: &[Step<A>]) -> Vec<B>
where
    F: Fn(&A) -> Vec<B>,
{
    let mut done: Vec<B> = Vec::new();
    for step in steps {
        match step {
            Step::ParentDir => {
                // drop 1: a .. with nothing left to cancel does nothing.
                if !done.is_empty() {
                    done.remove(0);
                }
            }
            Step::Step(segment) => {
                let mut expanded = expand(segment);
                expanded.reverse();
                done.splice(0..0, expanded);
            }
        }
    }
    done.reverse();
    done
}

/// How many segments a pattern must consume at minimum: an O(1) reject.
pub fn min_segments<A>(segments: &[Seg<A>]) -> usize {
    segments.iter().filter(|s| matches!(s, Seg::Segment(_))).count()
}

// ---------------------------------------------------------------------------
// 4. Paths
// ---------------------------------------------------------------------------

/// A module path, split into segments once at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segments(pub Vec<String>);

pub fn segments_of(path: &str) -> Segments {
    Segments(path.split('/').map(str::to_string).collect())
}

pub fn path_of(segments: &Segments) -> String {
    segments.0.join("/")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchEnv {
    /// Directory portion of the matched target path ("." if none).
    pub target_dir: String,
    pub variables: BTreeMap<VarName, BoundName>,
}

// ---------------------------------------------------------------------------
// 5. Target matching
// ---------------------------------------------------------------------------

pub fn match_target(target: &CompiledTargetPattern, path: &Segments) -> Option<MatchEnv> {
    if path.0.len() < target.min_length {
        return None;
    }
    let bindings = walk_segments(&bind_parts, &target.segments, &path.0, Bindings::default())?;
    Some(MatchEnv {
        target_dir: directory_of(&path.0),
        variables: resolve_bindings(bindings.bound),
    })
}

/// The directory a matched path sits in: everything but its final segment.
fn directory_of(path: &[String]) -> String {
    if path.is_empty() {
        ".".to_string()
    } else {
        path[..path.len() - 1].join("/")
    }
}

/// The outer walk: how many segments each globstar eats. Returns every way the
/// remaining pattern can consume the remaining path via `step`.
fn walk_segments<A, St, F>(
    step: &F,
    segs: &[Seg<Vec<SegPart<A>>>],
    path: &[String],
    st: St,
) -> Option<St>
where
    F: Fn(&[SegPart<A>], &str, St) -> Vec<St>,
    St: Clone,
{
    if segs.is_empty() {
        return if path.is_empty() { Some(st) } else { None };
    }
    match &segs[0] {
        Seg::GlobStar => {
            let rest = &segs[1..];
            let slack = path.len().saturating_sub(min_segments(rest));
            for width in 0..=slack {
                if let Some(result) = walk_segments(step, rest, &path[width..], st.clone()) {
                    return Some(result);
                }
            }
            None
        }
        Seg::Segment(parts) => {
            let (segment, deeper) = path.split_first()?;
            for st2 in step(parts, segment, st) {
                if let Some(result) = walk_segments(step, &segs[1..], deeper, st2) {
                    return Some(result);
                }
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 6. Binding, and agreement as a constraint
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Bindings {
    bound: BTreeMap<VarName, Binding>,
}

#[derive(Clone)]
struct Binding {
    agreed: AgreedName,
    occurrences: Vec<(Casing, String)>,
}

/// Every way the parts can divide one segment's text, greedy-left: each slot
/// takes the most it can. Contradicting divisions are pruned immediately.
fn bind_parts(parts: &[SegPart<TargetVar>], text: &str, bindings: Bindings) -> Vec<Bindings> {
    bind_go(parts, text, bindings)
}

fn bind_go(parts: &[SegPart<TargetVar>], text: &str, bindings: Bindings) -> Vec<Bindings> {
    match parts.split_first() {
        None => {
            if text.is_empty() {
                vec![bindings]
            } else {
                vec![]
            }
        }
        Some((SegPart::Lit(literal), rest)) => match text.strip_prefix(literal.as_str()) {
            Some(remaining) => bind_go(rest, remaining, bindings),
            None => vec![],
        },
        Some((SegPart::AnyChars, rest)) => widths_of(0, text)
            .into_iter()
            .flat_map(|taken| bind_go(rest, &text[taken..], bindings.clone()))
            .collect(),
        Some((SegPart::Var(var), rest)) => widths_of(1, text)
            .into_iter()
            .filter(|&taken| captured_by(var.casing, &text[..taken]))
            .filter_map(|taken| {
                let value = &text[..taken];
                let narrowed = bind_occurrence(&var.name, var.casing, value, bindings.clone())?;
                Some(bind_go(rest, &text[taken..], narrowed))
            })
            .flatten()
            .collect(),
    }
}

/// Candidate take-lengths as byte offsets at char boundaries, longest first.
/// A variable must take at least one char (`smallest` >= 1); a wildcard may
/// take none (smallest == 0).
fn widths_of(smallest: usize, text: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    boundaries.push(text.len());
    boundaries.into_iter().skip(smallest).rev().collect()
}

/// Records one occurrence and re-asks whether all occurrences so far can still
/// denote one name. None kills the branch.
fn bind_occurrence(name: &str, casing: Casing, value: &str, bindings: Bindings) -> Option<Bindings> {
    let mut bound = bindings;
    let entry = bound.bound.entry(name.to_string()).or_insert_with(|| Binding {
        agreed: AgreedName { canonical: vec![], candidates: vec![] },
        occurrences: Vec::new(),
    });
    entry.occurrences.push((casing, value.to_string()));
    let occurrences = entry.occurrences.clone();
    let agreed = agree(&occurrences)?;
    entry.agreed = agreed;
    Some(bound)
}

/// Whether a value is something this casing's capture accepts — deliberately
/// looser than `Casing::spelled_in`: patterns are strict, values are lenient.
/// Matches `capturedBy` in GlobPlus.hs character for character.
fn captured_by(casing: Casing, text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else { return false };
    let alnum = |c: char| c.is_ascii_alphanumeric();
    let kebab_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    let constant_char = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_';
    match casing {
        Casing::Pascal => first.is_ascii_uppercase() && chars.all(alnum),
        Casing::Camel => first.is_ascii_lowercase() && chars.all(alnum),
        Casing::Kebab => kebab_char(first) && text.chars().skip(1).all(kebab_char),
        Casing::Constant => constant_char(first) && text.chars().skip(1).all(constant_char),
    }
}

/// Turns the surviving constraints into values: the coarsest agreed name is
/// rendered into all four casings, then each occurrence's own literal text is
/// written back into its own casing slot (so same-casing use stays exact).
fn resolve_bindings(bound: BTreeMap<VarName, Binding>) -> BTreeMap<VarName, BoundName> {
    bound
        .into_iter()
        .map(|(name, binding)| {
            let base = CasedName {
                pascal: render(Casing::Pascal, &binding.agreed.canonical),
                camel: render(Casing::Camel, &binding.agreed.canonical),
                kebab: render(Casing::Kebab, &binding.agreed.canonical),
                constant: render(Casing::Constant, &binding.agreed.canonical),
            };
            let spelling = binding.occurrences.iter().fold(base, |mut acc, (casing, value)| {
                match casing {
                    Casing::Pascal => acc.pascal = value.clone(),
                    Casing::Camel => acc.camel = value.clone(),
                    Casing::Kebab => acc.kebab = value.clone(),
                    Casing::Constant => acc.constant = value.clone(),
                }
                acc
            });
            (name, BoundName { spelling, candidates: binding.agreed.candidates })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 7. Clause and exclude matching
// ---------------------------------------------------------------------------

/// A clause pattern with its variables already substituted; built once per
/// matched target and reused for every candidate import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClause {
    pub segments: Vec<Seg<Vec<ResolvedPart>>>,
    pub min_length: usize,
}

/// One piece of a hydrated segment: no variables left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPart {
    RLit(String),
    RAnyChars,
    /// One of several spellings, as a Widen clause admits.
    RAlt(Vec<String>),
}

pub fn match_clause(clause: &CompiledClausePattern, env: &MatchEnv, path: &Segments) -> bool {
    match_resolved(&hydrate(env, clause), path)
}

pub fn match_exclude(exclude: &CompiledExcludePattern, path: &Segments) -> bool {
    if path.0.len() < exclude.min_length {
        return false;
    }
    let resolved: Vec<Seg<Vec<ResolvedPart>>> = exclude
        .segments
        .iter()
        .map(|seg| match seg {
            Seg::GlobStar => Seg::GlobStar,
            Seg::Segment(parts) => Seg::Segment(
                parts
                    .iter()
                    .map(|p| match p {
                        SegPart::Lit(t) => ResolvedPart::RLit(t.clone()),
                        SegPart::AnyChars => ResolvedPart::RAnyChars,
                        SegPart::Var(v) => match *v {},
                    })
                    .collect::<Vec<ResolvedPart>>(),
            ),
        })
        .collect();
    match_segments(&resolved, &path.0)
}

pub fn match_resolved(clause: &ResolvedClause, path: &Segments) -> bool {
    if path.0.len() < clause.min_length {
        return false;
    }
    match_segments(&clause.segments, &path.0)
}

fn match_segments(segs: &[Seg<Vec<ResolvedPart>>], path: &[String]) -> bool {
    if segs.is_empty() {
        return path.is_empty();
    }
    match &segs[0] {
        Seg::GlobStar => {
            let rest = &segs[1..];
            let slack = path.len().saturating_sub(min_segments(rest));
            (0..=slack).any(|w| match_segments(rest, &path[w.min(path.len())..]))
        }
        Seg::Segment(parts) => {
            let Some((segment, deeper)) = path.split_first() else { return false };
            consumes(parts, segment) && match_segments(&segs[1..], deeper)
        }
    }
}

/// Whether the parts can divide this segment's text at all.
fn consumes(parts: &[ResolvedPart], text: &str) -> bool {
    match parts.split_first() {
        None => text.is_empty(),
        Some((ResolvedPart::RLit(literal), rest)) => {
            text.strip_prefix(literal.as_str()).is_some_and(|rest_text| consumes(rest, rest_text))
        }
        Some((ResolvedPart::RAnyChars, rest)) => {
            widths_of(0, text).into_iter().any(|taken| consumes(rest, &text[taken..]))
        }
        Some((ResolvedPart::RAlt(spellings), rest)) => spellings
            .iter()
            .any(|s| text.strip_prefix(s.as_str()).is_some_and(|rest_text| consumes(rest, rest_text))),
    }
}

/// Substitutes a clause's variables. Only {{TARGET_DIR}} can introduce a `/`,
/// so hydration splits on slashes after substitution and merges literals.
pub fn hydrate(env: &MatchEnv, clause: &CompiledClausePattern) -> ResolvedClause {
    let polarity = clause.polarity;
    let hydrated: Vec<Seg<Vec<ResolvedPart>>> = resolve_steps(
        |seg: &PatternSegment<ClauseVar>| match seg {
            Seg::GlobStar => vec![Seg::GlobStar],
            Seg::Segment(parts) => {
                let merged: Vec<ResolvedPart> =
                    merge_lits(parts.iter().flat_map(|p| hydrate_part(env, polarity, p)).collect());
                split_on_slash(merged).into_iter().map(Seg::Segment).collect()
            }
        },
        &clause.steps,
    );
    ResolvedClause { min_length: min_segments(&hydrated), segments: hydrated }
}

fn hydrate_part(env: &MatchEnv, polarity: Polarity, part: &SegPart<ClauseVar>) -> Vec<ResolvedPart> {
    match part {
        SegPart::Lit(t) => vec![ResolvedPart::RLit(t.clone())],
        SegPart::AnyChars => vec![ResolvedPart::RAnyChars],
        SegPart::Var(ClauseVar::TargetDir) => vec![ResolvedPart::RLit(env.target_dir.clone())],
        SegPart::Var(ClauseVar::Var { name, casing }) => match env.variables.get(name) {
            None => vec![ResolvedPart::RAlt(vec!["\0unbound".to_string()])],
            Some(bound) => vec![spellings_of(polarity, *casing, bound)],
        },
    }
}

/// What a variable stands for in a clause, in the direction its polarity says
/// it is safe to be wrong.
fn spellings_of(polarity: Polarity, casing: Casing, bound: &BoundName) -> ResolvedPart {
    match polarity {
        Polarity::Narrow => ResolvedPart::RLit(cased_as(casing, bound)),
        Polarity::Widen => {
            let mut spellings = vec![cased_as(casing, bound)];
            for candidate in &bound.candidates {
                for r in crate::casing::renderings(casing, candidate) {
                    if !spellings.contains(&r) {
                        spellings.push(r);
                    }
                }
            }
            if spellings.len() > ALTERNATION_LIMIT {
                spellings.truncate(ALTERNATION_LIMIT);
            }
            match spellings.first() {
                Some(first) if spellings.len() == 1 => ResolvedPart::RLit(first.clone()),
                _ => ResolvedPart::RAlt(spellings),
            }
        }
    }
}

const ALTERNATION_LIMIT: usize = 256;

/// Breaks a hydrated segment wherever a substitution introduced a `/`.
/// Mirrors `splitOnSlash` in GlobPlus.hs: text left of the first slash joins
/// the segment so far, each middle piece becomes its own segment, and the
/// final piece starts the next accumulated segment.
fn split_on_slash(parts: Vec<ResolvedPart>) -> Vec<Vec<ResolvedPart>> {
    fn go(current: Vec<ResolvedPart>, rest: &[ResolvedPart], out: &mut Vec<Vec<ResolvedPart>>) {
        match rest.split_first() {
            None => out.push(current),
            Some((ResolvedPart::RLit(text), tail)) if text.contains('/') => {
                let pieces: Vec<&str> = text.split('/').collect();
                let n = pieces.len();
                // No slash in this piece: stays part of the current segment.
                if n == 1 {
                    let mut next = current;
                    next.push(ResolvedPart::RLit(pieces[0].to_string()));
                    go(next, tail, out);
                    return;
                }
                let mut opening = current;
                opening.push(ResolvedPart::RLit(pieces[0].to_string()));
                out.push(opening);
                for middle in &pieces[1..n - 1] {
                    out.push(vec![ResolvedPart::RLit((*middle).to_string())]);
                }
                go(vec![ResolvedPart::RLit(pieces[n - 1].to_string())], tail, out);
            }
            Some((part, tail)) => {
                let mut next = current;
                next.push(part.clone());
                go(next, tail, out);
            }
        }
    }
    let mut out = Vec::new();
    go(Vec::new(), &parts, &mut out);
    out
}

fn merge_lits(parts: Vec<ResolvedPart>) -> Vec<ResolvedPart> {
    let mut out: Vec<ResolvedPart> = Vec::new();
    for part in parts {
        if let (Some(ResolvedPart::RLit(prev)), ResolvedPart::RLit(text)) = (out.last_mut(), &part) {
            prev.push_str(text);
        } else {
            out.push(part);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 8. Expansion
// ---------------------------------------------------------------------------

/// Expands a clause pattern into a concrete module path, or None if it holds
/// wildcards, which cannot be deterministically expanded.
///
/// Every step is expanded before any `..` is resolved, so a substitution that
/// introduces a `/` is several segments by the time one is cancelled.
pub fn module_from_glob(env: &MatchEnv, clause: &CompiledClausePattern) -> Option<String> {
    let mut dirs: Vec<String> = Vec::new();
    for step in &clause.steps {
        match step {
            Step::ParentDir => {
                if !dirs.is_empty() {
                    dirs.remove(0);
                }
            }
            Step::Step(seg) => {
                let mut pieces = expand_segment(env, seg)?;
                pieces.reverse();
                dirs.splice(0..0, pieces);
            }
        }
    }
    dirs.reverse();
    Some(dirs.join("/"))
}

fn expand_segment(env: &MatchEnv, seg: &Seg<Vec<SegPart<ClauseVar>>>) -> Option<Vec<String>> {
    match seg {
        Seg::GlobStar => None,
        Seg::Segment(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    SegPart::Lit(t) => text.push_str(t),
                    SegPart::AnyChars => return None,
                    SegPart::Var(v) => text.push_str(&value_of(env, v)?),
                }
            }
            Some(text.split('/').map(str::to_string).collect())
        }
    }
}

/// Renders a clause pattern for a human: variables substituted, `..` resolved,
/// wildcards kept literally.
pub fn render_clause_pattern(env: &MatchEnv, clause: &CompiledClausePattern) -> String {
    type PS = Seg<Vec<SegPart<ClauseVar>>>;
    let rendered: Vec<String> = resolve_steps(
        |seg: &PS| -> Vec<String> {
            match seg {
                Seg::GlobStar => vec!["**".to_string()],
                Seg::Segment(parts) => {
                    let joined: String = parts
                        .iter()
                        .map(|p| -> String {
                            match p {
                                SegPart::Lit(t) => t.clone(),
                                SegPart::AnyChars => "*".to_string(),
                                SegPart::Var(v) => value_of(env, v).unwrap_or_else(|| match v {
                                    ClauseVar::TargetDir => "{{TARGET_DIR}}".to_string(),
                                    ClauseVar::Var { name, casing } => {
                                        format!("{{{{{}}}}}", spell_var(name, *casing))
                                    }
                                }),
                            }
                        })
                        .collect();
                    joined.split('/').map(str::to_string).collect()
                }
            }
        },
        &clause.steps,
    );
    rendered.join("/")
}

/// What a clause variable stands for under a match, if anything does.
pub fn value_of(env: &MatchEnv, var: &ClauseVar) -> Option<String> {
    match var {
        ClauseVar::TargetDir => Some(env.target_dir.clone()),
        ClauseVar::Var { name, casing } => env.variables.get(name).map(|b| cased_as(*casing, b)),
    }
}

/// Writes a variable's canonical name back out in the given casing.
pub fn spell_var(name: &str, casing: Casing) -> String {
    render(casing, &crate::casing::Casing::Kebab.decode(name))
}
