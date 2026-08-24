//! Names, and the four ways a codebase spells them.
//!
//! Port of `Deslop/Casing.hs`. A *name* is a list of lower-case words; a
//! *casing* is a way of writing one down. Rather than asking what a spelling
//! decodes to, `spells` asks whether a spelling could denote a given name,
//! which is exact; `decode` guesses by reading each run of capitals as one
//! word, and is only needed when writing a captured name back out in another
//! casing.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Casing {
    Pascal,
    Camel,
    Constant,
    Kebab,
}

/// Declaration order matches the Haskell `[minBound .. maxBound]` iteration,
/// which is observable in ambiguity diagnostics.
pub const ALL_CASINGS: [Casing; 4] = [Casing::Pascal, Casing::Camel, Casing::Constant, Casing::Kebab];

impl Casing {
    pub fn name(self) -> &'static str {
        match self {
            Casing::Pascal => "PascalCase",
            Casing::Camel => "camelCase",
            Casing::Kebab => "kebab-case",
            Casing::Constant => "CONSTANT_CASE",
        }
    }

    /// Whether a token is a well-formed spelling in this casing.
    pub fn spelled_in(self, text: &str) -> bool {
        match self {
            Casing::Pascal => starts_with_upper_alnum_tail(text, |c| c.is_ascii_uppercase()),
            Casing::Camel => starts_with_upper_alnum_tail(text, |c| c.is_ascii_lowercase()),
            Casing::Kebab => separated_by('-', |c| c.is_ascii_lowercase() || c.is_ascii_digit(), text),
            Casing::Constant => separated_by('_', |c| c.is_ascii_uppercase() || c.is_ascii_digit(), text),
        }
    }

    /// The name a spelling most likely denotes: each run of capitals is one
    /// word. Exact for kebab-case and CONSTANT_CASE, a guess otherwise.
    pub fn decode(self, text: &str) -> Vec<String> {
        match self {
            Casing::Kebab => split_on_separator('-', text),
            Casing::Constant => split_on_separator('_', text).into_iter().map(|w| w.to_lowercase()).collect(),
            Casing::Pascal => coarsest_grouping(is_capitalised_block, &atoms(text)),
            Casing::Camel => coarsest_grouping(is_leading_block, &atoms(text)),
        }
    }

    /// Every name a spelling could denote. Separator casings propose exactly
    /// one; Pascal and camel propose one per way of grouping their atoms.
    pub fn decodings(self, text: &str) -> Vec<Vec<String>> {
        match self {
            Casing::Kebab | Casing::Constant => vec![self.decode(text)],
            Casing::Pascal => groupings_of(is_capitalised_block, &atoms(text)),
            Casing::Camel => groupings_of(is_leading_block, &atoms(text)),
        }
    }

    /// Whether `name`, written in this casing, could have produced `text`.
    pub fn spells(self, name: &[String], text: &str) -> bool {
        match self {
            Casing::Kebab => text == render(self, name),
            Casing::Constant => text == render(self, name),
            Casing::Pascal => spells_capitalised(name, text),
            Casing::Camel => match name.split_first() {
                None => text.is_empty(),
                Some((first, rest)) => text
                    .strip_prefix(first.as_str())
                    .is_some_and(|remainder| spells_capitalised(rest, remainder)),
            },
        }
    }
}

/// Writes a name out in a casing, capitalising rather than upper-casing.
pub fn render(casing: Casing, name: &[String]) -> String {
    match casing {
        Casing::Pascal => name.iter().map(|w| capitalise(w)).collect(),
        Casing::Camel => match name.split_first() {
            None => String::new(),
            Some((first, rest)) => {
                let mut out = first.clone();
                out.extend(rest.iter().map(|w| capitalise(w)));
                out
            }
        },
        Casing::Kebab => name.join("-"),
        Casing::Constant => name.iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("_"),
    }
}

/// Every spelling of a name in a casing, canonical first. Pascal and camel may
/// write any word wholly upper-case, so an acronym-bearing name has several.
pub fn renderings(casing: Casing, name: &[String]) -> Vec<String> {
    match casing {
        Casing::Kebab => vec![render(casing, name)],
        Casing::Constant => vec![render(casing, name)],
        Casing::Pascal => word_spellings(name).into_iter().map(|ws| ws.concat()).collect(),
        Casing::Camel => match name.split_first() {
            None => vec![String::new()],
            Some((first, rest)) => word_spellings(rest)
                .into_iter()
                .map(|tail| format!("{first}{}", tail.concat()))
                .collect(),
        },
    }
}

/// Per-word spellings `[Capitalised, UPPERCASE]`, deduplicated, cartesian
/// product in order, last stream varying fastest.
fn word_spellings(words: &[String]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = vec![Vec::new()];
    for word in words {
        let cap = capitalise(word);
        let shout = word.to_uppercase();
        let options: Vec<String> =
            if cap == shout { vec![cap.clone()] } else { vec![cap, shout] };
        let mut next = Vec::new();
        for prefix in &out {
            for option in &options {
                let mut combined = prefix.clone();
                combined.push(option.clone());
                next.push(combined);
            }
        }
        out = next;
    }
    out
}

#[derive(Debug, Clone)]
pub struct AgreedName {
    /// The coarsest of `candidates` (fewest words); what gets written out.
    pub canonical: Vec<String>,
    /// Every name all occurrences spell, shortest first.
    pub candidates: Vec<Vec<String>>,
}

/// The name every occurrence spells, if there is one.
///
/// A candidate only has to be proposed by *some* occurrence, because a
/// candidate no occurrence proposed cannot survive the check anyway.
/// Occurrences must be non-empty (guaranteed by callers).
pub fn agree(occurrences: &[(Casing, String)]) -> Option<AgreedName> {
    let mut proposed: Vec<Vec<String>> = Vec::new();
    for (casing, text) in occurrences {
        for decoding in casing.decodings(text) {
            if !proposed.contains(&decoding) {
                proposed.push(decoding);
            }
        }
    }
    let survivors: Vec<Vec<String>> = proposed
        .into_iter()
        .filter(|name| occurrences.iter().all(|(c, t)| c.spells(name, t)))
        .collect();
    if survivors.is_empty() {
        return None;
    }
    let mut sorted = survivors;
    sorted.sort_by_key(|name| name.len());
    Some(AgreedName { canonical: sorted[0].clone(), candidates: sorted })
}

// ---------------------------------------------------------------------------
// Word-block machinery
// ---------------------------------------------------------------------------

const CANDIDATE_LIMIT: usize = 64;

type BlockPredicate = fn(&str) -> bool;

/// Splits a spelling at every point a word boundary could fall: before a
/// capital, and before a run of digits (`Api2fa` -> `["Api", "2fa"]`).
fn atoms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut prev: Option<char> = None;
    for c in text.chars() {
        let boundary = match prev {
            None => true,
            Some(p) => c.is_ascii_uppercase() || (c.is_ascii_digit() && !p.is_ascii_digit()),
        };
        if boundary {
            out.push(c.to_string());
        } else {
            out.last_mut().expect("non-empty").push(c);
        }
        prev = Some(c);
    }
    out
}

fn groupings_of(is_first_block: BlockPredicate, as_atoms: &[String]) -> Vec<Vec<String>> {
    let mut found = search_groupings(is_first_block, as_atoms);
    if found.is_empty() {
        found.push(coarsest_grouping(is_first_block, as_atoms));
    } else {
        found.truncate(CANDIDATE_LIMIT);
    }
    found
}

fn search_groupings(is_first_block: BlockPredicate, remaining: &[String]) -> Vec<Vec<String>> {
    if remaining.is_empty() {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for n in (1..=remaining.len()).rev() {
        let block: String = remaining[..n].concat();
        if is_first_block(&block) {
            for rest in search_groupings(is_capitalised_block, &remaining[n..]) {
                let mut grouping = Vec::with_capacity(rest.len() + 1);
                grouping.push(block.to_lowercase());
                grouping.extend(rest);
                out.push(grouping);
            }
        }
    }
    out
}

/// The coarsest grouping: the longest readable word, repeatedly.
fn coarsest_grouping(is_first_block: BlockPredicate, as_atoms: &[String]) -> Vec<String> {
    if as_atoms.is_empty() {
        return Vec::new();
    }
    for n in (1..=as_atoms.len()).rev() {
        let block: String = as_atoms[..n].concat();
        if is_first_block(&block) {
            let mut out = vec![block.to_lowercase()];
            out.extend(coarsest_grouping(is_capitalised_block, &as_atoms[n..]));
            return out;
        }
    }
    as_atoms.iter().map(|a| a.to_lowercase()).collect()
}

/// A word capitalised, or a word shouted. May open with a digit.
fn is_capitalised_block(block: &str) -> bool {
    let mut chars = block.chars();
    match chars.next() {
        None => false,
        Some(first) => {
            (first.is_ascii_uppercase() || first.is_ascii_digit())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                || is_shouted_block(block)
        }
    }
}

/// Only a camel spelling's first word looks like this.
fn is_leading_block(block: &str) -> bool {
    let mut chars = block.chars();
    match chars.next() {
        None => false,
        Some(first) => {
            (first.is_ascii_lowercase() && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
                || is_capitalised_block(block)
        }
    }
}

fn is_shouted_block(block: &str) -> bool {
    !block.is_empty() && block.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Whether each word appears in order, capitalised or wholly upper-case.
fn spells_capitalised(words: &[String], text: &str) -> bool {
    match words.split_first() {
        None => text.is_empty(),
        Some((word, rest)) => {
            let cap = capitalise(word);
            let shout = word.to_uppercase();
            let spellings: Vec<&str> =
                if cap == shout { vec![cap.as_str()] } else { vec![cap.as_str(), shout.as_str()] };
            spellings.into_iter().any(|spelling| {
                text.strip_prefix(spelling).is_some_and(|remainder| spells_capitalised(rest, remainder))
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Character-class helpers
// ---------------------------------------------------------------------------

fn starts_with_upper_alnum_tail(text: &str, first: fn(char) -> bool) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        None => false,
        Some(c) => first(c) && chars.all(|c| c.is_ascii_alphanumeric()),
    }
}

fn separated_by(separator: char, is_body: fn(char) -> bool, text: &str) -> bool {
    !text.is_empty()
        && text.split(separator).all(|segment| !segment.is_empty() && segment.chars().all(is_body))
}

fn split_on_separator(separator: char, text: &str) -> Vec<String> {
    text.split(separator).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn decodes_coarsely() {
        // Expectations mirror test/Deslop/CasingSpec.hs in the original repo.
        assert_eq!(Casing::Pascal.decode("HTTPClient"), words(&["http", "client"]));
        assert_eq!(Casing::Pascal.decode("AWSS3Client"), words(&["awss3", "client"]));
        assert_eq!(Casing::Camel.decode("dbConnection"), words(&["db", "connection"]));
        assert_eq!(Casing::Kebab.decode("stripe-connect"), words(&["stripe", "connect"]));
        assert_eq!(Casing::Constant.decode("AWSS3_CLIENT"), words(&["awss3", "client"]));
        assert_eq!(Casing::Pascal.decode("Api2fa"), words(&["api2fa"]));
    }

    #[test]
    fn atoms_split_at_every_capital_and_digit_run() {
        assert_eq!(atoms("Api2fa"), words(&["Api", "2fa"]));
        assert_eq!(atoms("DBConnection"), words(&["D", "B", "Connection"]));
    }

    #[test]
    fn spells_exact_for_separators_and_guesses_for_capitals() {
        assert!(Casing::Kebab.spells(&words(&["http", "client"]), "http-client"));
        assert!(!Casing::Kebab.spells(&words(&["http", "client"]), "httpClient"));
        assert!(Casing::Pascal.spells(&words(&["http", "client"]), "HttpClient"));
        assert!(Casing::Pascal.spells(&words(&["http", "client"]), "HTTPClient"));
        assert!(!Casing::Pascal.spells(&words(&["http", "client"]), "httpClient"));
        assert!(Casing::Camel.spells(&words(&["http", "client"]), "httpClient"));
        assert!(Casing::Pascal.spells(&words(&["api", "2fa"]), "Api2fa"));
        assert!(Casing::Camel.spells(&words(&["api", "2fa"]), "api2fa"));
    }

    #[test]
    fn agreement_finds_the_name_all_occurrences_spell() {
        let occ = vec![
            (Casing::Kebab, "http-client".to_string()),
            (Casing::Pascal, "HTTPClient".to_string()),
        ];
        let agreed = agree(&occ).expect("agrees");
        assert_eq!(agreed.canonical, words(&["http", "client"]));
        // Pascal occurrence proposes several readings; all survivors kept.
        assert!(agreed.candidates.contains(&words(&["http", "client"])));
    }

    #[test]
    fn agreement_fails_when_nothing_spells_both() {
        let occ = vec![(Casing::Kebab, "paypal".to_string()), (Casing::Pascal, "StripeConnect".to_string())];
        assert!(agree(&occ).is_none());
    }

    #[test]
    fn renderings_enumerate_acronym_forms_canonical_first() {
        let name = words(&["db", "connection"]);
        // First stream varies slowest, matching `traverse` in the Haskell.
        assert_eq!(
            renderings(Casing::Pascal, &name),
            vec![
                "DbConnection".to_string(),
                "DbCONNECTION".to_string(),
                "DBConnection".to_string(),
                "DBCONNECTION".to_string()
            ]
        );
        assert_eq!(renderings(Casing::Kebab, &name), vec!["db-connection".to_string()]);
    }
}
