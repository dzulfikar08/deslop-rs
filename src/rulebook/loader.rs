//! Ports `Deslop/Rule/Book/Loader.hs` — reading rulebooks off disk: the only
//! part of the pipeline that touches IO.
//!
//! Every `deslop/rules/*` is read, decoded and compiled, and all of their
//! failures are reported together. The report is grouped by file, then by
//! rule, then by field, because more than one rulebook can be broken in one
//! run and an error that does not say which file it came from is a treasure
//! hunt.
//!
//! `RulebookLoadError` deliberately does not carry the file it came from.
//! Only the loader knows that, and keeping it out means a single rulebook can
//! be compiled and its failure inspected without an absolute path getting
//! into the answer.

use std::fs;
use std::path::{Path, PathBuf};

use super::book::Rulebook;
use super::compiler::{compile_rulebook, render_compile_error, CompileError};
use super::dto::parse_rulebook_yaml;
use crate::utils::pluralise;

/// Why one rulebook file could not become a `Rulebook`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulebookLoadError {
    /// Not well-formed YAML, or not shaped like a rulebook at all.
    UnreadableYaml(String),
    /// A rulebook, but some of its patterns do not compile.
    UncompilablePatterns(Vec<CompileError>),
}

pub fn load_rulebook(project_root: &Path) -> Result<Vec<Rulebook>, String> {
    load_rulebook_from(&project_root.join("deslop").join("rules"))
}

/// Loads every rulebook in a directory. A failure anywhere means none is
/// returned: enforcing half a rulebook would report problems its author never
/// asked for and miss the ones they did.
pub fn load_rulebook_from(dir: &Path) -> Result<Vec<Rulebook>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.sort();

    let mut rulebooks = Vec::new();
    let mut failures: Vec<(String, RulebookLoadError)> = Vec::new();
    for path in paths {
        match rulebook_from_file(&path) {
            Ok(rulebook) => rulebooks.push(rulebook),
            Err(err) => failures.push((heading(&path), err)),
        }
    }
    match failures.is_empty() {
        true => Ok(rulebooks),
        false => Err(render_rulebook_errors(&failures)),
    }
}

/// The heading a failure is reported under: the full path, as `decodeOsPath`
/// renders it in the original.
fn heading(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn rulebook_from_file(path: &Path) -> Result<Rulebook, RulebookLoadError> {
    let text = fs::read_to_string(path).map_err(|e| RulebookLoadError::UnreadableYaml(e.to_string()))?;
    let dto = parse_rulebook_yaml(&text).map_err(RulebookLoadError::UnreadableYaml)?;
    compile_rulebook(dto).map_err(RulebookLoadError::UncompilablePatterns)
}

/// Every failure of a run, grouped by file and then by rule, in source order
/// throughout — the author reads their file top to bottom and the report
/// should match. The count comes first, because "how bad is this" is the
/// first question anyone asks.
pub fn render_rulebook_errors(failures: &[(String, RulebookLoadError)]) -> String {
    let rendered: Vec<String> =
        failures.iter().map(|(name, err)| render_one(name, err)).collect();
    format!("Could not load {}.\n\n{}", pluralise(failures.len(), "rulebook"), rendered.join("\n\n"))
}

fn render_one(name: &str, err: &RulebookLoadError) -> String {
    match err {
        RulebookLoadError::UnreadableYaml(detail) => {
            let indented: Vec<String> = detail.lines().map(|l| format!("  {l}")).collect();
            format!("{name}\n{}", indented.join("\n"))
        }
        RulebookLoadError::UncompilablePatterns(errors) => {
            let rendered: Vec<String> = by_rule(errors)
                .into_iter()
                .map(|group| group.iter().map(|err| render_compile_error(err)).collect::<Vec<_>>().join("\n"))
                .collect();
            format!("{name}\n{}", rendered.join("\n"))
        }
    }
}

/// Groups a rule's errors together while keeping every rule in the order it
/// appeared. Errors arrive in source order already, so consecutive runs of
/// the same rule are the groups — no sorting, which would scramble that order.
fn by_rule(errors: &[CompileError]) -> Vec<Vec<&CompileError>> {
    let mut groups: Vec<Vec<&CompileError>> = Vec::new();
    for err in errors {
        match groups.last_mut() {
            Some(group) if group[0].rule == err.rule => group.push(err),
            _ => groups.push(vec![err]),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, name: &str, text: &str) {
        fs::write(dir.join(name), text).unwrap();
    }

    #[test]
    fn missing_rules_dir_loads_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load_rulebook(tmp.path()).unwrap(), Vec::new());
    }

    #[test]
    fn loads_and_compiles_every_file() {
        let tmp = tempfile::tempdir().unwrap();
        let rules = tmp.path().join("deslop").join("rules");
        fs::create_dir_all(&rules).unwrap();
        write(
            &rules,
            "a.yaml",
            "id: a\nname: A\nrules:\n  - id: r1\n    target: \"@/a/**\"\n    fix: x\n",
        );
        write(
            &rules,
            "b.yml",
            "id: b\nname: B\nrules:\n  - id: r2\n    target: \"@/b/**\"\n    fix: y\n",
        );
        let books = load_rulebook(tmp.path()).unwrap();
        let ids: Vec<&str> = books.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn one_broken_file_fails_the_whole_load_naming_every_file() {
        let tmp = tempfile::tempdir().unwrap();
        let rules = tmp.path().join("deslop").join("rules");
        fs::create_dir_all(&rules).unwrap();
        write(&rules, "good.yaml", "id: good\nname: G\nrules: []\n");
        write(&rules, "bad.yaml", "rules: [unclosed");
        let err = load_rulebook(tmp.path()).unwrap_err();
        assert!(err.starts_with("Could not load 1 rulebook.\n\n"), "{err}");
        assert!(err.contains("bad.yaml"), "{err}");
        assert!(!err.contains("good.yaml"), "{err}");
    }

    #[test]
    fn compile_failures_are_grouped_by_file_then_rule() {
        let tmp = tempfile::tempdir().unwrap();
        let rules = tmp.path().join("deslop").join("rules");
        fs::create_dir_all(&rules).unwrap();
        write(
            &rules,
            "broken.yaml",
            "id: broken\nname: B\nrules:\n  - id: r1\n    target: \"@/a/..\"\n    fix: x\n  - id: r2\n    target: \"@/ok/**\"\n    uses:\n      - import: \"{{ghost}}\"\n    fix: y\n",
        );
        let err = load_rulebook(tmp.path()).unwrap_err();
        assert!(err.contains("broken.yaml\n  rule 'r1'\n    target:"), "{err}");
        assert!(err.contains("  rule 'r2'\n    uses.import:"), "{err}");
    }
}
