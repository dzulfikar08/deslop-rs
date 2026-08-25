//! Turning the text an author wrote into a pattern the matcher can run.
//!
//! Port of `Deslop/GlobPlus/Compiler.hs`. Compilation is where every rule
//! about what a Glob+ pattern *may say* lives, so that the matcher answers
//! only what a valid pattern *means*.

use crate::casing::{render, Casing, ALL_CASINGS};
use crate::glob_plus::{
    min_segments, ClauseVar, CompiledClausePattern, CompiledExcludePattern, CompiledTargetPattern,
    Polarity, Seg, SegPart, Step, TargetVar, VarName,
};

/// A segment whose variable tokens are still raw text.
type RawPart = SegPart<String>;
type RawSegment = Seg<Vec<RawPart>>;

// ---------------------------------------------------------------------------
// 1. Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobPlusError {
    /// The pattern is not valid Glob+ syntax at all.
    MalformedPattern { input: String, detail: String },
    /// {{Provider-Name}} — not a recognised casing.
    UnrecognisedCasing(String),
    /// {{provider}} — reads as more than one casing.
    AmbiguousCasing { raw: String, casings: Vec<Casing> },
    /// {{HTTPClient}} — word boundaries cannot be determined.
    ConsecutiveCapitals(String),
    /// {{target-dir}} — reserved; only {{TARGET_DIR}} is accepted.
    ReservedTargetDir(String),
    /// {{TARGET_DIR}} in a target pattern, where it cannot be captured.
    TargetDirInTargetPattern(String),
    /// Any variable in an exclude pattern, which binds nothing.
    VariableInExcludePattern(String),
    /// `..` in a target pattern.
    ParentDirInTargetPattern,
    /// `..` in an exclude pattern.
    ParentDirInExcludePattern,
    /// `**/..` or `*/..` — the cancelled segment names no one directory.
    ParentDirPastWildcard(String),
    /// A clause variable the rule's target pattern never captures.
    UnboundVariable { name: VarName, bound: Vec<VarName> },
    /// `**View` — a globstar glued to text inside a segment.
    GlobStarNotWholeSegment(String),
    /// `**/{{a}}/**` — nothing says which segment `a` is.
    UnanchoredVariable(VarName),
    /// `{{a}}{{b}}` — no literal separates the two variables.
    NoBoundaryBetween(String, String),
}

pub const PARENT_DIR: &str = "..";
pub const GLOB_STAR: &str = "**";
pub const TARGET_DIR_KEYWORD: &str = "TARGET_DIR";

fn braced(t: &str) -> String {
    format!("{{{{{t}}}}}")
}

fn quoted(t: &str) -> String {
    format!("\"{t}\"")
}

/// Renders a compilation error with the same guidance the original prints.
pub fn render_error(error: &GlobPlusError) -> String {
    match error {
        GlobPlusError::MalformedPattern { input, detail } => {
            format!("invalid Glob+ pattern {}\n{detail}", quoted(input))
        }
        GlobPlusError::UnrecognisedCasing(raw) => {
            format!(
                "{} is not written in a recognised casing.\n  \
                 A variable must be spelled in exactly one of:\n    \
                 PascalCase     e.g. {{{{ProviderName}}}}\n    \
                 camelCase      e.g. {{{{providerName}}}}\n    \
                 kebab-case     e.g. {{{{provider-name}}}}\n    \
                 CONSTANT_CASE  e.g. {{{{PROVIDER_NAME}}}}",
                braced(raw)
            )
        }
        GlobPlusError::AmbiguousCasing { raw, casings } => {
            let names: Vec<&str> = casings.iter().map(|c| c.name()).collect();
            let suggestions: Vec<String> =
                ambiguity_suggestions(raw, casings).iter().map(|w| braced(w)).collect();
            format!(
                "{} is ambiguous: a single-word name reads as both {}.\n  \
                 Give the variable a name of at least two words, for example:\n{}",
                braced(raw),
                names.join(" and "),
                suggestions.iter().map(|s| format!("    {s}")).collect::<Vec<_>>().join("\n")
            )
        }
        GlobPlusError::ConsecutiveCapitals(raw) => {
            format!(
                "{} contains consecutive capitals, so its word boundaries are ambiguous.\n  \
                 Capitalise only the first letter of each word, e.g. {{{{HttpClient}}}},\n  \
                 or use kebab-case, e.g. {{{{http-client}}}}.",
                braced(raw)
            )
        }
        GlobPlusError::ReservedTargetDir(raw) => {
            format!(
                "{} is reserved.\n  \
                 The directory of the matched target is written {{{{TARGET_DIR}}}},\n  \
                 and no other spelling of that name is accepted.",
                braced(raw)
            )
        }
        GlobPlusError::TargetDirInTargetPattern(raw) => {
            format!(
                "{} cannot be used in a target pattern.\n  \
                 {{{{TARGET_DIR}}}} is derived from the path the target matches,\n  \
                 so it only has a value in a clause pattern.",
                braced(raw)
            )
        }
        GlobPlusError::VariableInExcludePattern(raw) => {
            format!(
                "{} cannot be used in an exclude pattern.\n  \
                 An exclude pattern filters the target and binds no variables.\n  \
                 Use a wildcard instead, e.g. * or **.",
                braced(raw)
            )
        }
        GlobPlusError::ParentDirInTargetPattern => {
            format!(
                "{} cannot be used in a target pattern.\n  \
                 A target is matched against whole module ids, so there is nothing\n  \
                 for {} to be relative to. Write the path you mean, e.g. \"@/shared/**\".\n{}",
                quoted(PARENT_DIR),
                quoted(PARENT_DIR),
                relative_to_target_dir()
            )
        }
        GlobPlusError::ParentDirInExcludePattern => {
            format!(
                "{} cannot be used in an exclude pattern.\n  \
                 An exclude pattern filters the target and is matched against whole\n  \
                 module ids, so there is nothing for {} to be relative to. Write\n  \
                 the path you mean, e.g. \"@/shared/**\".\n{}",
                quoted(PARENT_DIR),
                quoted(PARENT_DIR),
                relative_to_target_dir()
            )
        }
        GlobPlusError::ParentDirPastWildcard(segment) => {
            let why = if segment == GLOB_STAR {
                format!(
                    "{} stands for zero or many segments, so there is no one\n  \
                     directory to go back from.",
                    quoted(GLOB_STAR)
                )
            } else {
                "A segment containing \"*\" does not say which directory it is,\n  \
                 so there is no one directory to go back from."
                    .to_string()
            };
            format!(
                "{} cannot go back past {}.\n  {}\n  \
                 Write the directory you mean, or start from {}.",
                quoted(PARENT_DIR),
                quoted(segment),
                why,
                braced(TARGET_DIR_KEYWORD)
            )
        }
        GlobPlusError::UnboundVariable { name, bound } => {
            let render_bound = if bound.is_empty() {
                "(none)".to_string()
            } else {
                bound.join(", ")
            };
            let suggestion = did_you_mean(name, bound)
                .map(|s| format!("\n  Did you mean {}?", braced(&s)))
                .unwrap_or_default();
            format!(
                "unknown variable {}.\n  Variables bound by this rule's target: {}{}",
                braced(name),
                render_bound,
                suggestion
            )
        }
        GlobPlusError::GlobStarNotWholeSegment(segment) => {
            format!(
                "{} glues ** to text inside a single path segment.\n  \
                 ** stands for zero or many whole segments, so it cannot be part of one.\n  \
                 Match within a segment with *, e.g. *View, or give ** a segment of its\n  \
                 own, e.g. **/*View.",
                quoted(segment)
            )
        }
        GlobPlusError::UnanchoredVariable(name) => {
            format!(
                "{} has ** on both sides, so nothing in the pattern says which\n  \
                 path segment it names. A deeper tree would bind a different directory\n  \
                 than a shallow one, and neither would be the one you meant.\n  \
                 Anchor it: drop one of the **, or replace it with * to fix the depth.",
                braced(name)
            )
        }
        GlobPlusError::NoBoundaryBetween(left, right) => {
            format!(
                "{}{} has no literal between the two variables, so there\n  \
                 is no way to tell where the first one ends. A * between them is not a\n  \
                 boundary either, because it can match nothing.\n  \
                 Separate them with a literal, e.g. {}/{}.",
                braced(left),
                braced(right),
                braced(left),
                braced(right)
            )
        }
    }
}

fn relative_to_target_dir() -> String {
    format!(
        "  {} belongs in a clause, where it is relative to the directory\n  \
         of the file the target matched:\n    \
         allows: \"{}",
        quoted(PARENT_DIR),
        braced(TARGET_DIR_KEYWORD)
    ) + "/../shared/**\""
}

fn ambiguity_suggestions(raw: &str, casings: &[Casing]) -> Vec<String> {
    // The token is ambiguous, so any reading will do to name it after.
    let mut two_words = crate::casing::Casing::decode(casings[0], raw);
    two_words.push("name".to_string());
    casings.iter().map(|c| render(*c, &two_words)).collect()
}

fn did_you_mean(name: &str, bound: &[VarName]) -> Option<VarName> {
    bound
        .iter()
        .map(|candidate| (edit_distance(name, candidate), candidate))
        .filter(|(distance, _)| *distance <= 3)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate.clone())
}

/// Levenshtein distance, used only to suggest a near-miss variable name.
pub fn edit_distance(source: &str, target: &str) -> usize {
    let source: Vec<char> = source.chars().collect();
    let target: Vec<char> = target.chars().collect();
    let mut prev: Vec<usize> = (0..=source.len()).collect();
    for (j, tc) in target.iter().enumerate() {
        let mut current = vec![j + 1];
        for (i, sc) in source.iter().enumerate() {
            let cost = if sc == tc { 0 } else { 1 };
            current.push((prev[i] + cost).min(prev[i + 1] + 1).min(current[i] + 1));
        }
        prev = current;
    }
    prev[source.len()]
}

// ---------------------------------------------------------------------------
// 2. Compiling
// ---------------------------------------------------------------------------

/// Compiles the `target:` of a rule. Its captured variables become the only
/// ones its clauses may reference.
pub fn compile_target_pattern(input: &str) -> Result<CompiledTargetPattern, GlobPlusError> {
    let steps = parse_segments(input)?;
    let segments = no_parent_dirs(GlobPlusError::ParentDirInTargetPattern, &steps)?;
    let segments: Vec<Seg<Vec<SegPart<TargetVar>>>> =
        resolve_vars(resolve_target_var, &segments)?;
    for segment in &segments {
        if let Seg::Segment(parts) = segment {
            check_boundaries(parts)?;
        }
    }
    check_anchoring(&segments)?;
    let mut bound_vars: Vec<VarName> = Vec::new();
    for segment in &segments {
        if let Seg::Segment(parts) = segment {
            for part in parts {
                if let SegPart::Var(TargetVar { name, .. }) = part {
                    if !bound_vars.contains(name) {
                        bound_vars.push(name.clone());
                    }
                }
            }
        }
    }
    Ok(CompiledTargetPattern {
        min_length: min_segments(&segments),
        segments,
        bound_vars,
        source: input.to_string(),
    })
}

/// Compiles a `uses`/`forbids`/`allows`/`exists` pattern against the variables
/// its rule's target binds. The only pattern that may carry `..`.
pub fn compile_clause_pattern(
    polarity: Polarity,
    bound: &[VarName],
    input: &str,
) -> Result<CompiledClausePattern, GlobPlusError> {
    let steps = parse_segments(input)?;
    let typed: Vec<Step<Seg<Vec<SegPart<ClauseVar>>>>> = steps
        .into_iter()
        .map(|step| match step {
            Step::ParentDir => Ok(Step::ParentDir),
            Step::Step(seg) => {
                let resolved: Seg<Vec<SegPart<ClauseVar>>> = match seg {
                    Seg::GlobStar => Seg::GlobStar,
                    Seg::Segment(parts) => Seg::Segment(
                        parts
                            .into_iter()
                            .map(|p| map_raw_part_owned(p, resolve_clause_var))
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                };
                Ok(Step::Step(resolved))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    for step in &typed {
        if let Step::Step(Seg::Segment(parts)) = step {
            for part in parts {
                if let SegPart::Var(ClauseVar::Var { name, .. }) = part {
                    if !bound.contains(name) {
                        return Err(GlobPlusError::UnboundVariable {
                            name: name.clone(),
                            bound: sorted_unique(bound),
                        });
                    }
                }
            }
        }
    }
    check_parent_dirs(&typed)?;
    Ok(CompiledClausePattern { steps: typed, polarity, source: input.to_string() })
}

/// Compiles an `exclude:` pattern: a plain glob over module ids.
pub fn compile_exclude_pattern(input: &str) -> Result<CompiledExcludePattern, GlobPlusError> {
    let steps = parse_segments(input)?;
    let segments = no_parent_dirs(GlobPlusError::ParentDirInExcludePattern, &steps)?;
    let segments: Vec<Seg<Vec<SegPart<std::convert::Infallible>>>> = resolve_vars(
        |raw| Err(GlobPlusError::VariableInExcludePattern(raw.to_string())),
        &segments,
    )?;
    Ok(CompiledExcludePattern {
        min_length: min_segments(&segments),
        segments,
        source: input.to_string(),
    })
}

fn sorted_unique(names: &[VarName]) -> Vec<VarName> {
    let mut out = names.to_vec();
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------------------
// 3. Parsing
// ---------------------------------------------------------------------------

/// Drops the step wrapper from a pattern that may not carry `..`.
fn no_parent_dirs(
    cause: GlobPlusError,
    steps: &[Step<RawSegment>],
) -> Result<Vec<RawSegment>, GlobPlusError> {
    steps
        .iter()
        .map(|step| match step {
            Step::ParentDir => Err(cause.clone()),
            Step::Step(seg) => Ok(seg.clone()),
        })
        .collect()
}

fn resolve_vars<V>(
    resolve: impl Fn(&str) -> Result<V, GlobPlusError>,
    segments: &[RawSegment],
) -> Result<Vec<Seg<Vec<SegPart<V>>>>, GlobPlusError> {
    segments
        .iter()
        .map(|seg| match seg {
            Seg::GlobStar => Ok(Seg::GlobStar),
            Seg::Segment(parts) => Ok(Seg::Segment(
                parts
                    .iter()
                    .map(|part| map_raw_part(part, |r| resolve(r)))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
        })
        .collect()
}

/// Splits on `/` and parses each piece on its own. A variable token cannot
/// contain a `/`, so splitting first never cuts one in half.
fn parse_segments(input: &str) -> Result<Vec<Step<RawSegment>>, GlobPlusError> {
    input.split('/').map(|piece| parse_step(input, piece)).collect()
}

/// `..` is a whole segment or it is nothing; only the exact token is structural.
fn parse_step(input: &str, piece: &str) -> Result<Step<RawSegment>, GlobPlusError> {
    if piece == PARENT_DIR {
        Ok(Step::ParentDir)
    } else {
        Ok(Step::Step(parse_segment(input, piece)?))
    }
}

fn parse_segment(input: &str, piece: &str) -> Result<RawSegment, GlobPlusError> {
    if piece == GLOB_STAR {
        return Ok(Seg::GlobStar);
    }
    if piece.contains(GLOB_STAR) {
        return Err(GlobPlusError::GlobStarNotWholeSegment(piece.to_string()));
    }
    Ok(Seg::Segment(merge_raw_lits(parse_parts(input, piece)?)))
}

/// Scans one segment: `*` wildcards, `{{var}}` tokens, everything else literal.
/// A `{` not followed by another `{` is an ordinary character.
fn parse_parts(input: &str, piece: &str) -> Result<Vec<RawPart>, GlobPlusError> {
    let chars: Vec<char> = piece.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if !lit.is_empty() {
                    parts.push(RawPart::Lit(std::mem::take(&mut lit)));
                }
                parts.push(RawPart::AnyChars);
                i += 1;
            }
            '{' if chars.get(i + 1) == Some(&'{') => {
                if !lit.is_empty() {
                    parts.push(RawPart::Lit(std::mem::take(&mut lit)));
                }
                let mut j = i + 2;
                let mut raw = String::new();
                let mut closed = false;
                while j < chars.len() {
                    match chars[j] {
                        '}' if chars.get(j + 1) == Some(&'}') => {
                            closed = true;
                            break;
                        }
                        '{' | '}' | '*' | '/' => break,
                        c => {
                            raw.push(c);
                            j += 1;
                        }
                    }
                }
                if !closed || raw.is_empty() {
                    return Err(GlobPlusError::MalformedPattern {
                        input: input.to_string(),
                        detail: format!("expected a variable token followed by }}}} in \"{piece}\""),
                    });
                }
                parts.push(RawPart::Var(raw));
                i = j + 2;
            }
            c => {
                lit.push(c);
                i += 1;
            }
        }
    }
    if !lit.is_empty() {
        parts.push(RawPart::Lit(lit));
    }
    let _ = input;
    Ok(parts)
}

fn merge_raw_lits(parts: Vec<RawPart>) -> Vec<RawPart> {
    let mut out: Vec<RawPart> = Vec::new();
    for part in parts {
        if let (Some(RawPart::Lit(prev)), RawPart::Lit(text)) = (out.last_mut(), &part) {
            prev.push_str(text);
        } else {
            out.push(part);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 4. Validation
// ---------------------------------------------------------------------------

const TARGET_DIR_NAME: &str = "target-dir";

fn resolve_target_var(raw: &str) -> Result<TargetVar, GlobPlusError> {
    if names_target_dir(raw) {
        return Err(GlobPlusError::TargetDirInTargetPattern(raw.to_string()));
    }
    let (name, casing) = resolve_var(raw)?;
    Ok(TargetVar { name, casing })
}

/// Resolves one raw variable token inside any segment kind.
fn map_raw_part<V>(
    part: &RawPart,
    resolve: impl Fn(&str) -> Result<V, GlobPlusError>,
) -> Result<SegPart<V>, GlobPlusError> {
    match part {
        RawPart::Lit(t) => Ok(SegPart::Lit(t.clone())),
        RawPart::AnyChars => Ok(SegPart::AnyChars),
        RawPart::Var(raw) => resolve(raw).map(SegPart::Var),
    }
}

/// Same as `map_raw_part` but consumes the part (for owned iterators).
fn map_raw_part_owned<V>(
    part: RawPart,
    resolve: impl Fn(&str) -> Result<V, GlobPlusError>,
) -> Result<SegPart<V>, GlobPlusError> {
    match part {
        RawPart::Lit(t) => Ok(SegPart::Lit(t)),
        RawPart::AnyChars => Ok(SegPart::AnyChars),
        RawPart::Var(raw) => resolve(&raw).map(SegPart::Var),
    }
}

fn resolve_clause_var(raw: &str) -> Result<ClauseVar, GlobPlusError> {
    if raw == TARGET_DIR_KEYWORD {
        return Ok(ClauseVar::TargetDir);
    }
    if names_target_dir(raw) {
        return Err(GlobPlusError::ReservedTargetDir(raw.to_string()));
    }
    let (name, casing) = resolve_var(raw)?;
    Ok(ClauseVar::Var { name, casing })
}

fn resolve_var(raw: &str) -> Result<(VarName, Casing), GlobPlusError> {
    let casing = detect_casing(raw)?;
    check_consecutive_capitals(casing, raw)?;
    let name = render(Casing::Kebab, &casing.decode(raw));
    Ok((name, casing))
}

/// Whether a token is any spelling of the reserved name.
fn names_target_dir(raw: &str) -> bool {
    detect_casing(raw).is_ok_and(|casing| render(Casing::Kebab, &casing.decode(raw)) == TARGET_DIR_NAME)
}

/// A token is written in the one casing it is a valid spelling of. A single
/// word such as `provider` reads as both camelCase and kebab-case: rejected.
fn detect_casing(raw: &str) -> Result<Casing, GlobPlusError> {
    let matching: Vec<Casing> = ALL_CASINGS.iter().copied().filter(|c| c.spelled_in(raw)).collect();
    match matching.as_slice() {
        [] => Err(GlobPlusError::UnrecognisedCasing(raw.to_string())),
        [only] => Ok(*only),
        _ => Err(GlobPlusError::AmbiguousCasing { raw: raw.to_string(), casings: matching }),
    }
}

/// `HTTPClient` has no determinable word boundaries. Constant case is all
/// capitals by definition, so the check applies only where capitals carry meaning.
fn check_consecutive_capitals(casing: Casing, raw: &str) -> Result<(), GlobPlusError> {
    let consecutive = raw
        .chars()
        .zip(raw.chars().skip(1))
        .any(|(a, b)| a.is_ascii_uppercase() && b.is_ascii_uppercase());
    let reject = matches!(casing, Casing::Pascal | Casing::Camel) && consecutive;
    if reject {
        Err(GlobPlusError::ConsecutiveCapitals(raw.to_string()))
    } else {
        Ok(())
    }
}

/// Two variables in one segment need a literal between them; a `*` does not
/// count because it can match nothing. Adjacent-var detection skips AnyChars.
fn check_boundaries(parts: &[SegPart<TargetVar>]) -> Result<(), GlobPlusError> {
    let filtered: Vec<&SegPart<TargetVar>> =
        parts.iter().filter(|p| !matches!(p, SegPart::AnyChars)).collect();
    for window in filtered.windows(2) {
        if let (
            SegPart::Var(TargetVar { name: left, casing: lc }),
            SegPart::Var(TargetVar { name: right, casing: rc }),
        ) = (window[0], window[1])
        {
            return Err(GlobPlusError::NoBoundaryBetween(
                spell_var_name(left, *lc),
                spell_var_name(right, *rc),
            ));
        }
    }
    Ok(())
}

/// A target's variable must have its segment fixed by the pattern: no `**`
/// both before and after its segment, or the path would decide the binding.
fn check_anchoring(segments: &[Seg<Vec<SegPart<TargetVar>>>]) -> Result<(), GlobPlusError> {
    let has_glob_star_before = |index: usize| {
        segments[..index].iter().any(|s| matches!(s, Seg::GlobStar))
    };
    let has_glob_star_after = |index: usize| {
        segments[index + 1..].iter().any(|s| matches!(s, Seg::GlobStar))
    };
    for (index, segment) in segments.iter().enumerate() {
        if let Seg::Segment(parts) = segment {
            if has_glob_star_before(index) && has_glob_star_after(index) {
                for part in parts {
                    if let SegPart::Var(TargetVar { name, .. }) = part {
                        return Err(GlobPlusError::UnanchoredVariable(name.clone()));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Simulates the cancellation to find what each `..` would go back past.
/// Taking back nothing at all is the clamp and is allowed; taking back a
/// segment that names no one directory is not.
fn check_parent_dirs(steps: &[Step<Seg<Vec<SegPart<ClauseVar>>>>]) -> Result<(), GlobPlusError> {
    #[derive(Clone)]
    enum Token {
        Segment(Seg<Vec<SegPart<ClauseVar>>>),
    }
    let mut behind: Vec<Token> = Vec::new();
    for step in steps {
        match step {
            Step::ParentDir => match behind.pop() {
                None => {}
                Some(Token::Segment(seg)) => names_one_directory(&seg)?,
            },
            Step::Step(seg) => behind.push(Token::Segment(seg.clone())),
        }
    }
    Ok(())
}

fn names_one_directory(segment: &Seg<Vec<SegPart<ClauseVar>>>) -> Result<(), GlobPlusError> {
    match segment {
        Seg::GlobStar => Err(GlobPlusError::ParentDirPastWildcard(GLOB_STAR.to_string())),
        Seg::Segment(parts) => {
            if parts.iter().any(|p| matches!(p, SegPart::AnyChars)) {
                Err(GlobPlusError::ParentDirPastWildcard(spell_segment(parts)))
            } else {
                Ok(())
            }
        }
    }
}

/// Writes a compiled segment back out the way its author wrote it.
fn spell_segment(parts: &[SegPart<ClauseVar>]) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            SegPart::Lit(t) => out.push_str(t),
            SegPart::AnyChars => out.push('*'),
            SegPart::Var(ClauseVar::TargetDir) => out.push_str(&braced(TARGET_DIR_KEYWORD)),
            SegPart::Var(ClauseVar::Var { name, casing }) => {
                out.push_str(&braced(&spell_var_name(name, *casing)))
            }
        }
    }
    out
}

fn spell_var_name(name: &str, casing: Casing) -> String {
    // A VarName is canonical kebab-case words; write it back out in `casing`.
    render(casing, &decode_kebab(name))
}

fn decode_kebab(name: &str) -> Vec<String> {
    crate::casing::Casing::Kebab.decode(name)
}

// ---------------------------------------------------------------------------
// 5. Prose
// ---------------------------------------------------------------------------

/// Substitutes variables in prose with captured values, each written in the
/// casing of its occurrence. Prose is not a pattern: `*` and `/` are ordinary
/// characters, and a token naming nothing in scope stays exactly as written.
pub fn interpolate(env: &crate::glob_plus::MatchEnv, text: &str) -> String {
    let open = "{{";
    let close = "}}";
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        match after_open.find(close) {
            Some(end) => {
                let raw = &after_open[..end];
                match try_resolve(env, raw) {
                    Some(value) => {
                        out.push_str(&value);
                        rest = &after_open[end + close.len()..];
                    }
                    None => {
                        out.push_str(open);
                        rest = after_open;
                    }
                }
            }
            None => {
                out.push_str(open);
                rest = after_open;
            }
        }
    }
    out.push_str(rest);
    out
}

fn try_resolve(env: &crate::glob_plus::MatchEnv, raw: &str) -> Option<String> {
    // A candidate must look like a variable token; anything else stays put.
    if raw.is_empty()
        || raw.contains('{')
        || raw.contains('}')
        || raw.contains('*')
        || raw.contains('/')
    {
        return None;
    }
    match resolve_clause_var(raw) {
        Ok(var) => crate::glob_plus::value_of(env, &var),
        Err(_) => None,
    }
}
