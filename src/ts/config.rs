//! Ports `TypeScript/Config.hs` — tsconfig.json reading and path-mapping
//! patterns.
//!
//! A mapping key/value is either an exact path or a wildcard with one `*`
//! split into prefix and suffix. Mappings are sorted so exact keys beat
//! wildcards, longer prefixes beat shorter, then longer suffixes — that order
//! decides resolution everywhere downstream.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::types::DeslopError;

/// Ports `TypeScript.Config.Pattern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    Exact(String),
    Wildcard { pre: String, suff: String },
}

impl Pattern {
    /// Ports `parsePattern`: empty text is invalid; more than one `*` is
    /// invalid; one `*` splits into prefix and suffix.
    pub fn parse(t: &str) -> Option<Pattern> {
        match t.matches('*').count() {
            _ if t.is_empty() => None,
            0 => Some(Pattern::Exact(t.to_string())),
            1 => {
                let (pre, suff) = t.split_once('*')?;
                Some(Pattern::Wildcard { pre: pre.to_string(), suff: suff.to_string() })
            }
            _ => None,
        }
    }

    fn sort_key(&self) -> (u8, usize, usize) {
        match self {
            // Priority 1: exact matches float to the top.
            Pattern::Exact(k) => (1, k.chars().count(), 0),
            // Priority 0: wildcards after exacts, sub-sorted by prefix then
            // suffix length.
            Pattern::Wildcard { pre, suff } => (0, pre.chars().count(), suff.chars().count()),
        }
    }
}

/// Ports `TypeScript.Config.PathMapping`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMapping {
    pub key: Pattern,
    pub values: Vec<Pattern>,
}

impl PathMapping {
    /// Ports `parsePathMapping`: an empty target array, an invalid key, or a
    /// key that is exact while every candidate target carrying a wildcard
    /// fails ruins the mapping; otherwise invalid targets are filtered out,
    /// `./` prefixes are cleaned away recursively, and at least one target
    /// must survive.
    pub fn parse(key_text: &str, values: &[String]) -> Option<PathMapping> {
        let key = Pattern::parse(key_text)?;
        let cleaned: Vec<Pattern> = values
            .iter()
            .filter_map(|v| Pattern::parse(v))
            .map(clean_value_pattern)
            .filter(|v| valid_key_value_pair(&key, v))
            .collect();
        if cleaned.is_empty() {
            None
        } else {
            Some(PathMapping { key, values: cleaned })
        }
    }
}

fn clean_value_pattern(p: Pattern) -> Pattern {
    match p {
        Pattern::Exact(t) => Pattern::Exact(clean_prefix(t)),
        Pattern::Wildcard { pre, suff } => Pattern::Wildcard { pre: clean_prefix(pre), suff },
    }
}

/// Strips meaningless current-directory prefixes: `.` becomes empty and `./`
/// prefixes are removed repeatedly; `../` traversals survive untouched.
fn clean_prefix(t: String) -> String {
    if t == "." {
        String::new()
    } else if let Some(rest) = t.strip_prefix("./") {
        clean_prefix(rest.to_string())
    } else {
        t
    }
}

/// An exact key cannot legally point at wildcard targets.
fn valid_key_value_pair(key: &Pattern, value: &Pattern) -> bool {
    !matches!((key, value), (Pattern::Exact(_), Pattern::Wildcard { .. }))
}

/// Ports `TypeScript.Config.TsConfig`: baseUrl resolved against the
/// tsconfig's own directory, mappings sorted for the hot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsConfig {
    pub base_url: PathBuf,
    pub paths: Vec<PathMapping>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TsConfigDto {
    #[serde(default)]
    compiler_options: CompilerOptionsDto,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CompilerOptionsDto {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    paths: Option<std::collections::BTreeMap<String, Vec<String>>>,
}

impl TsConfig {
    /// Ports `readTsConfig`.
    pub fn load(path: &Path) -> Result<TsConfig, DeslopError> {
        let json = fs::read_to_string(path)
            .map_err(|e| DeslopError::TsConfigParse(format!("{}: {e}", path.display())))?;
        Self::parse_from_json(&json, path.parent().unwrap_or(Path::new(".")))
    }

    pub fn parse_from_json(json: &str, cfg_dir: &Path) -> Result<TsConfig, DeslopError> {
        let stripped =
            strip_ts_comments(json).ok_or_else(|| DeslopError::TsConfigParse("invalid JSON".into()))?;
        let dto: TsConfigDto = serde_json::from_str(&stripped)
            .map_err(|e| DeslopError::TsConfigParse(e.to_string()))?;
        Ok(Self::from_dto(dto, cfg_dir))
    }

    fn from_dto(dto: TsConfigDto, cfg_dir: &Path) -> TsConfig {
        let base_url_rel = dto.compiler_options.base_url.unwrap_or_else(|| ".".into());
        // Mirrors `withAbsBaseSafe cfgDir baseUrl`: join always; an absolute
        // rel replaces the dir outright in both OsPath (</>) and Path::join.
        let base_url = cfg_dir.join(base_url_rel);
        let mut paths: Vec<PathMapping> = dto
            .compiler_options
            .paths
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(k, vs)| PathMapping::parse(&k, &vs))
            .collect();
        sort_path_mappings(&mut paths);
        TsConfig { base_url, paths }
    }
}

use std::cmp::Reverse;

fn sort_path_mappings(paths: &mut [PathMapping]) {
    paths.sort_by_key(|p| Reverse(p.key.sort_key()));
}

// ---------------------------------------------------------------------------
// Comment stripping (ports the jsoncStripper megaparsec parser)
// ---------------------------------------------------------------------------

/// Safely strips `//` and `/* */` comments from JSON text, leaving string
/// literals (and URLs inside them) alone. Returns `None` when the text cannot
/// be parsed — e.g. an unterminated string — matching `parseMaybe ... <|>
/// fromMaybe input`'s bail-out.
pub fn strip_ts_comments(input: &str) -> Option<String> {
    let b = input.as_bytes();
    let len = b.len();
    let mut i = 0;
    let mut out = String::with_capacity(input.len());
    while i < len {
        let c = b[i];
        if c == b'"' {
            // Consume the literal whole, honoring backslash escapes.
            let start = i;
            i += 1;
            let mut closed = false;
            while i < len {
                match b[i] {
                    b'\\' => i += 2,
                    b'"' => {
                        i += 1;
                        closed = true;
                        break;
                    }
                    _ => i += 1,
                }
            }
            if !closed {
                return None;
            }
            out.push_str(&input[start..i]);
        } else if c == b'/' && i + 1 < len && b[i + 1] == b'/' {
            while i < len && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && i + 1 < len && b[i + 1] == b'*' {
            i += 2;
            let mut closed = false;
            while i + 1 < len {
                if b[i] == b'*' && b[i + 1] == b'/' {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return None;
            }
        } else {
            // Bulk-consume everything that cannot start a construct above.
            let start = i;
            while i < len && b[i] != b'"' && b[i] != b'/' {
                i += 1;
            }
            if start == i {
                // An isolated slash that starts nothing: keep it literally.
                out.push('/');
                i += 1;
            } else {
                out.push_str(&input[start..i]);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ConfigSpec parsePattern table.
    #[test]
    fn parse_pattern_cases() {
        assert_eq!(Pattern::parse(""), None);
        assert_eq!(Pattern::parse("hello"), Some(Pattern::Exact("hello".into())));
        assert_eq!(Pattern::parse("*"), Some(Pattern::Wildcard { pre: "".into(), suff: "".into() }));
        assert_eq!(
            Pattern::parse("@/*"),
            Some(Pattern::Wildcard { pre: "@/".into(), suff: "".into() })
        );
        assert_eq!(
            Pattern::parse("*.spec.ts"),
            Some(Pattern::Wildcard { pre: "".into(), suff: ".spec.ts".into() })
        );
        assert_eq!(
            Pattern::parse("@/data/*-dto"),
            Some(Pattern::Wildcard { pre: "@/data/".into(), suff: "-dto".into() })
        );
        assert_eq!(Pattern::parse("src/*/*"), None);
        assert_eq!(Pattern::parse("**"), None);
        assert_eq!(Pattern::parse("a*b*c"), None);
    }

    /// ConfigSpec parsePathMapping table.
    #[test]
    fn parse_path_mapping_cases() {
        let vals = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
        // Empty target array.
        assert_eq!(PathMapping::parse("@app/*", &[]), None);
        // Invalid key ruins the mapping even with valid targets.
        assert_eq!(PathMapping::parse("@app/*/*", &vals(&["./src/*"])), None);
        // Filters invalid targets, cleans './', keeps at least one.
        assert_eq!(
            PathMapping::parse("@app/*", &vals(&["./src/*", "./invalid/*/*", "./lib/*"])),
            Some(PathMapping {
                key: Pattern::Wildcard { pre: "@app/".into(), suff: "".into() },
                values: vec![
                    Pattern::Wildcard { pre: "src/".into(), suff: "".into() },
                    Pattern::Wildcard { pre: "lib/".into(), suff: "".into() },
                ],
            })
        );
        // Exact key mapped to exact value, './' cleaned.
        assert_eq!(
            PathMapping::parse("jquery", &vals(&["./vendor/jquery.js"])),
            Some(PathMapping {
                key: Pattern::Exact("jquery".into()),
                values: vec![Pattern::Exact("vendor/jquery.js".into())],
            })
        );
        // Pure '.' flattens to empty; nested './././' cleaned recursively.
        assert_eq!(
            PathMapping::parse("@root", &vals(&["."])),
            Some(PathMapping {
                key: Pattern::Exact("@root".into()),
                values: vec![Pattern::Exact("".into())],
            })
        );
        assert_eq!(
            PathMapping::parse("@app/*", &vals(&["./././src/*", "././lib/*"])),
            Some(PathMapping {
                key: Pattern::Wildcard { pre: "@app/".into(), suff: "".into() },
                values: vec![
                    Pattern::Wildcard { pre: "src/".into(), suff: "".into() },
                    Pattern::Wildcard { pre: "lib/".into(), suff: "".into() },
                ],
            })
        );
        // '../' traversals preserved.
        assert_eq!(
            PathMapping::parse("@shared/*", &vals(&["../shared/*", "./../external/*"])),
            Some(PathMapping {
                key: Pattern::Wildcard { pre: "@shared/".into(), suff: "".into() },
                values: vec![
                    Pattern::Wildcard { pre: "../shared/".into(), suff: "".into() },
                    Pattern::Wildcard { pre: "../external/".into(), suff: "".into() },
                ],
            })
        );
        // Exact key with only wildcard targets is invalid.
        assert_eq!(PathMapping::parse("react", &vals(&["*.css"])), None);
        // Next.js root alias './*' cleans into a bare catch-all.
        assert_eq!(
            PathMapping::parse("@/*", &vals(&["./*"])),
            Some(PathMapping {
                key: Pattern::Wildcard { pre: "@/".into(), suff: "".into() },
                values: vec![Pattern::Wildcard { pre: "".into(), suff: "".into() }],
            })
        );
    }

    /// Sorting: exact keys before wildcards, longer prefixes first.
    #[test]
    fn sorts_exact_first_then_longest_prefix() {
        let mk = |k: &str| PathMapping::parse(k, &[format!("{k}x")]).unwrap();
        let mut paths = vec![mk("@/*"), mk("jquery"), mk("@/ui/*"), mk("*")];
        sort_path_mappings(&mut paths);
        let keys: Vec<&str> = paths
            .iter()
            .map(|p| match &p.key {
                Pattern::Exact(s) => s.as_str(),
                Pattern::Wildcard { pre, suff } => Box::leak(format!("{pre}*{suff}").into_boxed_str()),
            })
            .collect();
        assert_eq!(keys, ["jquery", "@/ui/*", "@/*", "*"]);
    }

    #[test]
    fn strips_comments_protecting_strings() {
        let src = r#"{
  // comment
  /* block */
  "url": "http://x/y", // trailing
  "a": "str /* not comment */ ing",
  "esc": "quote \" stays"
}"#;
        let out = strip_ts_comments(src).unwrap();
        assert!(!out.contains("// comment"));
        assert!(out.contains("http://x/y"));
        assert!(out.contains("str /* not comment */ ing"));
        assert!(out.contains("\\\""));
    }
}
