//! Ports the AST half of `Deslop/AST.hs`.

use crate::ts::cst::CsT;

/// One import statement extracted from a module.
#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub specifier: String,
    /// True when the specifier starts with `./` or `../`.
    pub is_relative: bool,
}

/// A parsed module: canonical id, its imports, and its lossless CST.
pub struct AstModule {
    /// Module identity — relative path with `/`, extension kept.
    pub id: String,
    pub imports: Vec<ImportEntry>,
    pub cst: CsT,
}

impl AstModule {
    pub fn module_id(&self) -> &str {
        &self.id
    }
}

// TODO(port): parseAst — real import extraction from the token stream, plus
// the companion-file logic from Deslop/AST.hs.
pub fn parse_ast(id: String, source: String) -> AstModule {
    let imports = extract_imports(&source);
    AstModule { id, imports, cst: CsT { source } }
}

fn extract_imports(source: &str) -> Vec<ImportEntry> {
    // Placeholder scanner: matches `from '<spec>'` / `from "<spec>"`, skipping
    // line comments so commented-out imports don't count. The real port must
    // go through the lexer so block comments and strings don't fool it either.
    let mut out = Vec::new();
    for (line_no, line) in source.lines().enumerate() {
        let code = line.split_once("//").map_or(line, |(c, _)| c);
        for (i, _) in code.match_indices("from") {
            let after = code[i + 4..].trim_start();
            let quote = match after.chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => continue,
            };
            if let Some(end) = after[1..].find(quote) {
                let spec = &after[1..1 + end];
                let is_relative = spec.starts_with("./") || spec.starts_with("../");
                out.push((line_no, ImportEntry { specifier: spec.to_string(), is_relative }));
            }
        }
    }
    out.into_iter().map(|(_, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_relative_and_absolute_imports() {
        let m = parse_ast(
            "src/a.ts".into(),
            "import x from './b';\nimport y from '@/c';\n// from './not-real'\n".into(),
        );
        let specs: Vec<_> = m.imports.iter().map(|i| i.specifier.clone()).collect();
        assert_eq!(specs, ["./b", "@/c"]);
        assert!(m.imports[0].is_relative);
        assert!(!m.imports[1].is_relative);
    }
}
