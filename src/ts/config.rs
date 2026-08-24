//! Ports `TypeScript/Config.hs` — tsconfig.json path-alias resolution.
//!
//! TODO(port): `extends` chains and `baseUrl` fallbacks from the original;
//! this covers the flat `compilerOptions.paths` case.

use std::fs;

use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub struct TsConfig {
    /// Alias prefix → target prefixes, longest-prefix matched at lookup time.
    pub paths: Vec<(String, Vec<String>)>,
    pub base_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTsConfig {
    #[serde(default)]
    compiler_options: CompilerOptions,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CompilerOptions {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    paths: Option<std::collections::BTreeMap<String, Vec<String>>>,
}

impl TsConfig {
    pub fn load(path: &std::path::Path) -> Result<Self, crate::types::DeslopError> {
        let raw = fs::read_to_string(path).unwrap_or_else(|_| "{}".into());
        Self::parse(&raw)
    }

    pub fn parse(json: &str) -> Result<Self, crate::types::DeslopError> {
        // tsconfig is JSON-with-comments; strip // and /* */ comments first.
        let stripped = strip_comments(json);
        let raw: RawTsConfig = serde_json::from_str(&stripped)
            .map_err(|e| crate::types::DeslopError::TsConfigParse(e.to_string()))?;
        Ok(Self {
            paths: raw
                .compiler_options
                .paths
                .map(|m| m.into_iter().collect())
                .unwrap_or_default(),
            base_url: raw.compiler_options.base_url,
        })
    }

    /// Longest alias prefix wins; see [`Self::resolve_alias_to`].
    pub fn matches_alias(&self, specifier: &str) -> bool {
        self.paths.iter().any(|(alias, _)| {
            alias.strip_suffix('*').map_or(specifier == alias, |p| specifier.starts_with(p))
        })
    }

    /// Rewrites a module specifier through tsconfig path aliases, longest
    /// wildcard-prefix first; returns it unchanged when no alias matches.
    pub fn resolve_alias_to(&self, specifier: &str) -> String {
        let mut best: Option<(&str, &String)> = None;
        for (alias, targets) in &self.paths {
            let Some(target) = targets.first() else { continue };
            if let Some(prefix) = alias.strip_suffix('*') {
                if specifier.starts_with(prefix)
                    && best.as_ref().map_or(true, |(bp, _)| prefix.len() > bp.len())
                {
                    best = Some((prefix, target));
                }
            }
        }
        match best {
            None => specifier.to_string(),
            Some((prefix, target)) => {
                let captured = &specifier[prefix.len()..];
                match target.split_once('*') {
                    Some((head, tail)) => format!("{head}{captured}{tail}"),
                    None => target.clone(),
                }
            }
        }
    }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push(c);
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            }
            '"' => {
                out.push('"');
                for c in chars.by_ref() {
                    out.push(c);
                    if c == '"' || c == '\n' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths_with_comments() {
        let cfg = TsConfig::parse(
            r#"{
  // comment
  "compilerOptions": {
    "baseUrl": "src",
    "paths": { "@/*": ["app/*"], "@/ui/*": ["src/ui/*"] }
  }
}"#,
        )
        .unwrap();
        assert_eq!(cfg.resolve_alias_to("@/x/y"), "app/x/y");
        // longest alias prefix wins
        assert_eq!(cfg.resolve_alias_to("@/ui/button"), "src/ui/button");
        assert_eq!(cfg.resolve_alias_to("react"), "react");
    }
}
