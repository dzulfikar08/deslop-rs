//! Ports `Deslop/Rule/Book/Dto.hs` — the shape of a rulebook file, exactly as
//! a user may have written it.
//!
//! A DTO is raw: nothing here has been checked beyond being well-formed YAML,
//! so a pattern string may hold any text at all. Turning one into a
//! `book::Rulebook` is `compiler`'s job, and it is the only thing that may do
//! so.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RulebookDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub rules: Vec<RuleDto>,
}

#[derive(Debug, Deserialize)]
pub struct RuleDto {
    pub id: String,
    #[serde(default)]
    pub description: String,
    /// The Glob+ pattern whose match the rule governs; unchecked text.
    pub target: String,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub forbids: Vec<ForbidsDto>,
    #[serde(default)]
    pub allows: Vec<AllowsDto>,
    #[serde(default)]
    pub uses: Vec<UsesDto>,
    #[serde(default)]
    pub exists: Vec<ExistsDto>,
    #[serde(default)]
    pub example: Option<String>,
    #[serde(default)]
    pub fix: String,
}

#[derive(Debug, Deserialize)]
pub struct ForbidsDto {
    pub import: String,
    #[serde(default)]
    pub transitive: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AllowsDto {
    pub import: String,
}

#[derive(Debug, Deserialize)]
pub struct UsesDto {
    pub import: String,
    #[serde(default)]
    pub transitive: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ExistsDto {
    pub module: String,
}

/// Ports `parseRulebookYaml` — decode failure carries the parser's rendering,
/// as `show` does in the original.
pub fn parse_rulebook_yaml(text: &str) -> Result<RulebookDto, String> {
    serde_yaml::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_rulebook_shape() {
        let yaml = r#"
id: clean-architecture
name: Clean Architecture
description: Layers point inward.
rules:
  - id: domain-is-pure
    description: The domain imports only itself.
    target: "@/domain/**"
    forbids:
      - import: "**"
    allows:
      - import: "@/domain/**"
    uses:
      - import: "@/domain/**"
        transitive: true
    exists:
      - module: "@/domain/entities"
    example: |
      import { User } from '@/domain/user';
    fix: Remove the import.
"#;
        let dto = parse_rulebook_yaml(yaml).unwrap();
        assert_eq!(dto.id, "clean-architecture");
        assert_eq!(dto.rules.len(), 1);
        let rule = &dto.rules[0];
        assert_eq!(rule.target, "@/domain/**");
        assert_eq!(rule.forbids[0].import, "**");
        assert_eq!(rule.forbids[0].transitive, None);
        assert_eq!(rule.allows[0].import, "@/domain/**");
        assert_eq!(rule.uses[0].import, "@/domain/**");
        assert_eq!(rule.uses[0].transitive, Some(true));
        assert_eq!(rule.exists[0].module, "@/domain/entities");
        assert_eq!(rule.fix, "Remove the import.");
    }

    #[test]
    fn malformed_yaml_is_unreadable() {
        assert!(parse_rulebook_yaml("rules: [unclosed").is_err());
    }
}
