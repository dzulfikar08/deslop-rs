//! Ports `Git/Ignore.hs` — a gitignore(5) matcher for file discovery.
//!
//! Hand-rolled wildmatch like the original: patterns compile to segments /
//! tokens, matching walks both in lockstep with backtracking on `**`.
//! Negation, directory-only rules, anchoring, character classes (including
//! POSIX classes) and nested `.gitignore` scopes (deeper overrides shallower)
//! all behave as in `Git.Ignore`.

use std::path::{Path, PathBuf};

/// Names never traversed, for any reason (`alwaysIgnoredNames`). Pruned
/// greedily during both walks; no rule can re-include one.
pub const ALWAYS_IGNORED_NAMES: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    ".next",
    "next-env.d.ts",
    ".next-env.d.ts",
    "build",
    "out",
    ".output",
    "storybook-static",
    "coverage",
    ".direnv",
    ".devenv",
    ".turbo",
    ".cache",
    ".parcel-cache",
    ".yarn",
    ".svelte-kit",
    ".nuxt",
    ".astro",
    ".vercel",
    ".wrangler",
];

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// Matches exactly one character.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CharMatch {
    Lit(char),
    AnyChar,
    Class(bool, Vec<ClassItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClassItem {
    Char(char),
    Range(char, char),
    Posix(PosixClass),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PosixClass {
    Alnum,
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    XDigit,
}

const ALL_POSIX: [PosixClass; 12] = [
    PosixClass::Alnum,
    PosixClass::Alpha,
    PosixClass::Blank,
    PosixClass::Cntrl,
    PosixClass::Digit,
    PosixClass::Graph,
    PosixClass::Lower,
    PosixClass::Print,
    PosixClass::Punct,
    PosixClass::Space,
    PosixClass::Upper,
    PosixClass::XDigit,
];

impl PosixClass {
    fn name(self) -> &'static str {
        match self {
            PosixClass::Alnum => "alnum",
            PosixClass::Alpha => "alpha",
            PosixClass::Blank => "blank",
            PosixClass::Cntrl => "cntrl",
            PosixClass::Digit => "digit",
            PosixClass::Graph => "graph",
            PosixClass::Lower => "lower",
            PosixClass::Print => "print",
            PosixClass::Punct => "punct",
            PosixClass::Space => "space",
            PosixClass::Upper => "upper",
            PosixClass::XDigit => "xdigit",
        }
    }

    /// Unicode approximations of the Data.Char predicates.
    fn matches(self, c: char) -> bool {
        match self {
            PosixClass::Alnum => c.is_alphanumeric(),
            PosixClass::Alpha => c.is_alphabetic(),
            PosixClass::Blank => c == '\t' || c.is_whitespace() && !c.is_control(),
            PosixClass::Cntrl => c.is_control(),
            PosixClass::Digit => c.is_ascii_digit(),
            PosixClass::Graph => !c.is_control() && !c.is_whitespace(),
            PosixClass::Lower => c.is_lowercase(),
            PosixClass::Print => !c.is_control(),
            PosixClass::Punct => {
                !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control()
            }
            PosixClass::Space => c.is_whitespace(),
            PosixClass::Upper => c.is_uppercase(),
            PosixClass::XDigit => c.is_ascii_hexdigit(),
        }
    }

    fn parse(name: &str) -> Option<PosixClass> {
        ALL_POSIX.iter().copied().find(|p| p.name() == name)
    }
}

/// A piece of a single path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// `*`: zero or more characters, never crossing a separator
    Star,
    One(CharMatch),
}

/// One path segment of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    /// `**` standing alone as a whole segment
    GlobStar,
    /// Fast path: a segment holding no metacharacters
    Exact(String),
    Tokens(Vec<Tok>),
}

/// One meaningful line of a `.gitignore`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IgnoreRule {
    pattern: Vec<Seg>,
    negated: bool,
    dir_only: bool,
    anchored: bool,
}

/// The rules of one `.gitignore`, in file order, and the directory they govern.
#[derive(Debug)]
struct IgnoreScope {
    base: PathBuf,
    rules: Vec<IgnoreRule>,
}

/// Every `.gitignore` under a project, merged. `scopes` runs shallowest-first,
/// which is what makes a deeper `.gitignore` override a shallower one.
#[derive(Debug, Default)]
pub struct GitIgnore {
    scopes: Vec<IgnoreScope>,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl GitIgnore {
    /// Walks the project once, collecting and parsing every `.gitignore`,
    /// then sorts scopes shallowest-first.
    pub fn load(project_root: &Path) -> Self {
        let mut scopes: Vec<IgnoreScope> = collect_gitignores(project_root)
            .into_iter()
            .filter_map(|f| read_scope(&f))
            .collect();
        scopes.sort_by_key(|s| s.base.components().count());
        Self { scopes }
    }

    /// Whether git would ignore this entry: any component along the ancestry
    /// chain is always-ignored, or carries an ignore verdict. Callers that
    /// prune as they descend find the chain short-circuits at the first part.
    pub fn is_ignored(&self, root: &Path, path: &Path, is_dir: bool) -> bool {
        self.ancestry(root, path, is_dir)
            .iter()
            .any(|&(ref p, d)| always_ignored_name(p) || self.verdict(p, d).unwrap_or(false))
    }

    /// The chain from the outermost component under the root down to the
    /// entry. Ancestors are directories by construction; only the entry
    /// carries the caller's own `is_dir`.
    fn ancestry(&self, root: &Path, path: &Path, is_dir: bool) -> Vec<(PathBuf, bool)> {
        let mut chain: Vec<(PathBuf, bool)> = match path.strip_prefix(root) {
            Ok(rel) => {
                let comps: Vec<&std::ffi::OsStr> =
                    rel.components().map(|c| c.as_os_str()).collect();
                let n = comps.len();
                (1..n)
                    .map(|i| (root.join(comps[..i].iter().collect::<PathBuf>()), true))
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        chain.push((path.to_path_buf(), is_dir));
        chain
    }

    /// The last matching rule wins, and deeper scopes are folded after
    /// shallower ones so that they override. `None` = no rule matched.
    fn verdict(&self, entry: &Path, is_dir: bool) -> Option<bool> {
        let entry_segs = segs_of(entry);
        let mut acc = None;
        for scope in &self.scopes {
            let base_segs = segs_of(&scope.base);
            let under = entry_segs.len() > base_segs.len()
                && entry_segs[..base_segs.len()] == base_segs[..];
            if !under {
                continue;
            }
            let rel = &entry_segs[base_segs.len()..];
            for rule in &scope.rules {
                if match_rule(rule, rel, is_dir) {
                    acc = Some(!rule.negated);
                }
            }
        }
        acc
    }
}

fn always_ignored_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| ALWAYS_IGNORED_NAMES.contains(&n))
}

fn collect_gitignores(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        // Depth 0 is the project root itself; never prune it by name.
        .filter_entry(|e| e.depth() == 0 || !always_ignored_name(e.path()))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name() == ".gitignore")
        .map(walkdir::DirEntry::into_path)
        .collect()
}

/// A `.gitignore` that cannot be decoded contributes no rules.
fn read_scope(file: &Path) -> Option<IgnoreScope> {
    let base = file.parent()?.to_path_buf();
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let rules: Vec<IgnoreRule> = text.lines().filter_map(parse_ignore_rule).collect();
    Some(IgnoreScope { base, rules })
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parses one line, or `None` for a blank line or a comment. Order follows
/// git: strip the line ending, drop unescaped trailing spaces, reject
/// comments and blanks, peel off `!`, then the directory-only trailing `/`,
/// and only then decide anchoring from whether a `/` survives.
fn parse_ignore_rule(raw: &str) -> Option<IgnoreRule> {
    let line = strip_trailing_spaces(raw.trim_end_matches('\r'));
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (negated, unsigned) = peel_prefix('!', &line);
    let (dir_only, body) = peel_suffix('/', unsigned);
    if body.is_empty() {
        return None;
    }
    let anchored = body.contains('/');
    let body = body.strip_prefix('/').unwrap_or(body);
    Some(IgnoreRule {
        pattern: compile_pattern(body),
        negated,
        dir_only,
        anchored,
    })
}

fn peel_prefix(p: char, t: &str) -> (bool, &str) {
    t.strip_prefix(p).map_or((false, t), |rest| (true, rest))
}

fn peel_suffix(p: char, t: &str) -> (bool, &str) {
    t.strip_suffix(p).map_or((false, t), |rest| (true, rest))
}

/// Drops trailing spaces, keeping one escaped by an odd run of backslashes.
fn strip_trailing_spaces(t: &str) -> String {
    let mut s = t.to_string();
    while let Some(rest) = s.strip_suffix(' ') {
        let slashes = rest.chars().rev().take_while(|&c| c == '\\').count();
        if slashes % 2 == 1 {
            break;
        }
        s = rest.to_string();
    }
    s
}

fn compile_pattern(body: &str) -> Vec<Seg> {
    body.split('/').map(compile_seg).collect()
}

fn compile_seg(t: &str) -> Seg {
    if t == "**" {
        return Seg::GlobStar;
    }
    match parse_tokens(t) {
        Some(toks) => collapse(toks),
        None => Seg::Exact(t.to_string()),
    }
}

/// A segment whose tokens are all literals collapses to `Exact`.
fn collapse(toks: Vec<Tok>) -> Seg {
    let mut lit = String::new();
    for tok in &toks {
        match tok {
            Tok::One(CharMatch::Lit(c)) => lit.push(*c),
            _ => return Seg::Tokens(toks),
        }
    }
    Seg::Exact(lit)
}

/// Mirrors `many pTok <* eof`: any unparsable remainder rejects the whole
/// segment (which then stays a literal).
fn parse_tokens(t: &str) -> Option<Vec<Tok>> {
    let cs: Vec<char> = t.chars().collect();
    let mut i = 0;
    let mut toks = Vec::new();
    while i < cs.len() {
        let (tok, next) = p_tok(&cs, i)?;
        toks.push(tok);
        i = next;
    }
    Some(toks)
}

fn p_tok(cs: &[char], i: usize) -> Option<(Tok, usize)> {
    if cs[i] == '*' {
        return Some((Tok::Star, i + 1));
    }
    let (m, next) = p_char_match(cs, i)?;
    Some((Tok::One(m), next))
}

/// A `[` that does not open a well-formed class falls back to a literal `[`.
fn p_char_match(cs: &[char], i: usize) -> Option<(CharMatch, usize)> {
    match cs[i] {
        '?' => Some((CharMatch::AnyChar, i + 1)),
        '[' => p_class(cs, i).or_else(|| p_lit_char(cs, i)),
        _ => p_lit_char(cs, i),
    }
}

/// `\` escapes any single char; `*` and `?` are not literals here.
fn p_lit_char(cs: &[char], i: usize) -> Option<(CharMatch, usize)> {
    match cs[i] {
        '*' | '?' => None,
        '\\' => cs.get(i + 1).map(|&c| (CharMatch::Lit(c), i + 2)),
        c => Some((CharMatch::Lit(c), i + 1)),
    }
}

fn p_class(cs: &[char], i: usize) -> Option<(CharMatch, usize)> {
    let mut j = i + 1;
    let mut negated = false;
    if cs.get(j) == Some(&'!') || cs.get(j) == Some(&'^') {
        negated = true;
        j += 1;
    }
    let mut items = Vec::new();
    // A `]` immediately after the opening bracket is a literal, not a close.
    if cs.get(j) == Some(&']') {
        items.push(ClassItem::Char(']'));
        j += 1;
    }
    loop {
        match cs.get(j) {
            Some(']') => return Some((CharMatch::Class(negated, items), j + 1)),
            Some(_) => {
                let (item, next) = p_class_item(cs, j)?;
                items.push(item);
                j = next;
            }
            None => return None,
        }
    }
}

fn p_class_item(cs: &[char], i: usize) -> Option<(ClassItem, usize)> {
    if let Some(hit) = p_posix(cs, i) {
        return Some(hit);
    }
    if let Some(hit) = p_range(cs, i) {
        return Some(hit);
    }
    let (c, next) = p_class_char(cs, i)?;
    Some((ClassItem::Char(c), next))
}

fn p_posix(cs: &[char], i: usize) -> Option<(ClassItem, usize)> {
    if cs.get(i) != Some(&'[') || cs.get(i + 1) != Some(&':') {
        return None;
    }
    let start = i + 2;
    for p in ALL_POSIX {
        let name = p.name();
        let end = start + name.len();
        if end + 2 <= cs.len()
            && cs[start..end].iter().collect::<String>() == name
            && cs[end] == ':'
            && cs[end + 1] == ']'
        {
            return Some((ClassItem::Posix(p), end + 2));
        }
    }
    None
}

fn p_range(cs: &[char], i: usize) -> Option<(ClassItem, usize)> {
    let (lo, mid) = p_class_char(cs, i)?;
    if cs.get(mid) != Some(&'-') {
        return None;
    }
    let (hi, end) = p_class_char(cs, mid + 1)?;
    Some((ClassItem::Range(lo, hi), end))
}

fn p_class_char(cs: &[char], i: usize) -> Option<(char, usize)> {
    match cs.get(i)? {
        '\\' => Some((*cs.get(i + 1)?, i + 2)),
        ']' => None,
        &c => Some((c, i + 1)),
    }
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// Matches one rule against a path already made relative to its scope. An
/// anchored rule matches the relative path in full; an unanchored one matches
/// only the basename, which is what lets `*.log` hit at any depth.
fn match_rule(rule: &IgnoreRule, rel_segs: &[String], is_directory: bool) -> bool {
    if rule.dir_only && !is_directory {
        return false;
    }
    if rule.anchored {
        match_segs(&rule.pattern, rel_segs)
    } else {
        rel_segs.last().is_some_and(|base| {
            let one = [base.clone()];
            match_segs(&rule.pattern, &one)
        })
    }
}

/// `**` spans zero or more segments, except as the final segment of a
/// pattern, where `foo/**` means everything *inside* `foo` and so needs at
/// least one.
fn match_segs(segs: &[Seg], ps: &[String]) -> bool {
    match (segs.split_first(), ps.split_first()) {
        (None, _) => ps.is_empty(),
        // Trailing `**` means everything inside, so it needs one segment.
        (Some((Seg::GlobStar, [])), _) => !ps.is_empty(),
        (Some((Seg::GlobStar, _)), _) => {
            match_segs(&segs[1..], ps) || (!ps.is_empty() && match_segs(segs, &ps[1..]))
        }
        (Some(_), None) => false,
        (Some((seg, rest)), Some((p, prest))) => match seg {
            Seg::Exact(t) => t == p && match_segs(rest, prest),
            Seg::Tokens(toks) => {
                let pcs: Vec<char> = p.chars().collect();
                match_toks(toks, &pcs) && match_segs(rest, prest)
            }
            Seg::GlobStar => unreachable!(),
        },
    }
}

fn match_toks(toks: &[Tok], t: &[char]) -> bool {
    match toks.split_first() {
        None => t.is_empty(),
        Some((Tok::Star, rest)) => {
            match_toks(rest, t) || (!t.is_empty() && match_toks(toks, &t[1..]))
        }
        Some((Tok::One(m), rest)) => {
            !t.is_empty() && m.matches(t[0]) && match_toks(rest, &t[1..])
        }
    }
}

impl CharMatch {
    fn matches(&self, c: char) -> bool {
        match self {
            CharMatch::Lit(l) => *l == c,
            CharMatch::AnyChar => true,
            CharMatch::Class(negated, items) => *negated != items.iter().any(|it| in_class(c, it)),
        }
    }
}

fn in_class(c: char, item: &ClassItem) -> bool {
    match item {
        ClassItem::Char(x) => *x == c,
        ClassItem::Range(lo, hi) => lo <= &c && &c <= hi,
        ClassItem::Posix(p) => p.matches(c),
    }
}

fn segs_of(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| c.as_os_str().to_str())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an in-memory scope rooted at `base` — no files touched.
    fn gi_at(base: &str, lines: &str) -> GitIgnore {
        GitIgnore {
            scopes: vec![IgnoreScope {
                base: PathBuf::from(base),
                rules: lines.lines().filter_map(parse_ignore_rule).collect(),
            }],
        }
    }

    fn gi(lines: &str) -> GitIgnore {
        gi_at("/r", lines)
    }

    #[test]
    fn node_modules_always_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let g = GitIgnore::load(tmp.path());
        assert!(g.is_ignored(
            tmp.path(),
            &tmp.path().join("node_modules/x/y.ts"),
            true
        ));
    }

    #[test]
    fn always_ignored_names_cannot_be_reincluded() {
        let g = gi("!node_modules\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/node_modules"), true));
    }

    #[test]
    fn unanchored_basename_hits_any_depth() {
        let g = gi("*.log\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/deep/nest/x.log"), false));
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/x.log"), false));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/x.logx"), false));
        // Outside the scope: no rule applies at all.
        assert!(!g.is_ignored(Path::new("/other"), Path::new("/other/y.log"), false));
    }

    #[test]
    fn anchored_hits_only_at_scope_root() {
        let g = gi("/b/\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/b"), true));
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/b/f.ts"), true));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/a/b"), true));
    }

    #[test]
    fn negation_unignores() {
        let g = gi("*.log\n!important.log\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/a.log"), false));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/important.log"), false));
    }

    #[test]
    fn dir_only_skips_files() {
        let g = gi("gen/\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/gen"), true));
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/gen/deep.ts"), true));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/gen"), false));
    }

    #[test]
    fn glob_star_spans_segments() {
        let g = gi("src/**/tmp/\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/src/a/tmp"), true));
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/src/a/b/tmp"), true));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/src/tmp2"), true));
    }

    #[test]
    fn trailing_glob_star_needs_a_child() {
        let g = gi("src/**\n");
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/src"), true));
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/src/a.ts"), false));
    }

    #[test]
    fn deeper_scope_overrides_shallower() {
        // NB: `dist` itself is an always-ignored name and can never be
        // re-included, so use an ordinary directory name here.
        let mut g = gi("logs\n");
        g.scopes.push(IgnoreScope {
            base: PathBuf::from("/r/sub"),
            rules: vec![parse_ignore_rule("!logs").unwrap()],
        });
        // Shallowest-first invariant.
        assert!(g.scopes[0].base.components().count() <= g.scopes[1].base.components().count());
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/logs"), true));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/sub/logs"), true));
    }

    #[test]
    fn comments_blanks_and_trailing_spaces_are_stripped() {
        let g = gi("# comment\n\n  \nfoo \n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/foo"), true));
    }

    #[test]
    fn question_mark_matches_exactly_one_char() {
        let g = gi("file?.ts\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/fileA.ts"), false));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/fileAB.ts"), false));
    }

    #[test]
    fn character_classes() {
        let g = gi("[abc][0-9]\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/a5"), false));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/d5"), false));
        let neg = gi("[!a]\n");
        assert!(!neg.is_ignored(Path::new("/r"), Path::new("/r/a"), false));
        assert!(neg.is_ignored(Path::new("/r"), Path::new("/r/b"), false));
    }

    #[test]
    fn escaped_space_is_kept() {
        let g = gi("foo\\ \n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/foo "), false));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/foo"), false));
    }

    #[test]
    fn malformed_class_is_literal() {
        let g = gi("[abc\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/[abc"), false));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/a"), false));
    }

    #[test]
    fn crlf_lines_parse() {
        let g = gi("*.log\r\n!important.log\r\n");
        assert!(g.is_ignored(Path::new("/r"), Path::new("/r/a.log"), false));
        assert!(!g.is_ignored(Path::new("/r"), Path::new("/r/important.log"), false));
    }
}
