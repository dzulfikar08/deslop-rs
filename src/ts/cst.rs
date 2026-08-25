//! Ports `TypeScript/Tokens.hs`, `TypeScript/Lexer.hs`, `TypeScript/CST.hs`
//! and `TypeScript/Parser.hs`.
//!
//! A hand-written lexer splits source into import statements, comments,
//! whitespace and raw runs; re-concatenating every token's raw text reproduces
//! the input byte-for-byte. Each import token is then re-parsed into
//! `prefix | target | suffix` so an import's specifier can be rewritten while
//! everything around it survives untouched.
//!
//! Two deliberate asymmetries carried over from the original:
//! - the lexer's brace/paren counter clamps at zero (`max 0 (d-1)`), the
//!   parser's brace counter does not;
//! - the lexer counts `{`, `}`, `(` and `)`; the parser counts braces only,
//!   which is why `await import ('../x')` finds its target quote at depth 0.

/// Ports `TypeScript.Tokens.TsTokenKind`. Comments carry no content beyond
/// their raw text: they are lexed only so an `import` inside one is never
/// mistaken for a real import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsTokenKind {
    Import,
    Comment,
    Whitespace,
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsToken {
    pub raw: String,
    pub kind: TsTokenKind,
}

/// Ports `TypeScript.CST.TsProgram`/`TsNode`. Rendering every node in order
/// reproduces the file exactly (`Types.Renderable`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TsNode {
    Import { prefix: String, target: String, suffix: String },
    Source { raw: String },
}

impl TsNode {
    pub fn render(&self) -> String {
        match self {
            TsNode::Source { raw } => raw.clone(),
            TsNode::Import { prefix, target, suffix } => format!("{prefix}{target}{suffix}"),
        }
    }

    pub fn is_import(&self) -> bool {
        matches!(self, TsNode::Import { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsProgram {
    pub path: String,
    pub cst: Vec<TsNode>,
}

impl TsProgram {
    /// Renders every node in order: the file, exactly as it now stands.
    pub fn render(&self) -> String {
        self.cst.iter().map(|node| node.render()).collect()
    }
}

/// Ports `TypeScript.Parser.parseTs`: lex the whole file, turn every import
/// token into a structured node, keep everything else as verbatim source.
/// The lexer cannot fail; a malformed import token degrades to a `Source`.
pub fn parse_ts(path: &str, content: &str) -> TsProgram {
    TsProgram {
        path: path.to_string(),
        cst: build_cst(content),
    }
}

pub fn build_cst(source: &str) -> Vec<TsNode> {
    lex(source)
        .into_iter()
        .map(|token| match token.kind {
            TsTokenKind::Import => parse_import(&token.raw).unwrap_or(TsNode::Source { raw: token.raw }),
            TsTokenKind::Comment | TsTokenKind::Whitespace | TsTokenKind::Raw => {
                TsNode::Source { raw: token.raw }
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// Splits source into tokens whose raw texts concatenate back to `source`.
pub fn lex(source: &str) -> Vec<TsToken> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        let start = i;
        if source[i..].starts_with("import") {
            i = scan_import_body(bytes, i + "import".len());
            tokens.push(TsToken { raw: source[start..i].to_string(), kind: TsTokenKind::Import });
        } else if bytes[i..].starts_with(b"//") {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            if i < len {
                i += 1; // the line comment swallows its trailing newline
            }
            tokens.push(TsToken { raw: source[start..i].to_string(), kind: TsTokenKind::Comment });
        } else if bytes[i..].starts_with(b"/*") {
            match find_block_comment_end(bytes, i + 2) {
                Some(end) => i = end,
                // An unterminated block comment is not a comment: fall back to
                // a raw run, which consumes one character unconditionally.
                None => i = scan_raw(bytes, i),
            }
            tokens.push(TsToken { raw: source[start..i].to_string(), kind: TsTokenKind::Comment });
        } else if is_space(bytes[i]) {
            while i < len && is_space(bytes[i]) {
                i += 1;
            }
            tokens.push(TsToken { raw: source[start..i].to_string(), kind: TsTokenKind::Whitespace });
        } else {
            i = scan_raw(bytes, i);
            tokens.push(TsToken { raw: source[start..i].to_string(), kind: TsTokenKind::Raw });
        }
    }
    tokens
}

/// megaparsec's `space1`: space, tab, newline, carriage return, vertical tab,
/// form feed.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
}

/// Walks an import statement starting just past the `import` keyword and
/// returns the exclusive end offset. Depth counts braces and parens, clamping
/// at zero; the statement ends at a `;`, newline or `)` reached at depth zero
/// (a directly following `;` is swallowed too), or at end of input. Strings
/// and comments inside the statement are skipped whole, so their braces,
/// semicolons and newlines never terminate it.
fn scan_import_body(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    let mut depth: i64 = 0;
    loop {
        if i >= len {
            return len;
        }
        let c = bytes[i];
        let next = match c {
            b'{' | b'(' => depth + 1,
            b'}' | b')' => (depth - 1).max(0),
            _ => depth,
        };
        if next == 0 && (c == b';' || c == b'\n' || c == b')') {
            let mut end = i + 1;
            if end < len && bytes[end] == b';' {
                end += 1;
            }
            return end;
        }
        match c {
            b'"' | b'\'' | b'`' => match skip_string(bytes, i, c) {
                Some(j) => i = j,
                // Unterminated string: the try backtracks and the opening
                // quote alone is consumed as an ordinary character.
                None => {
                    i += 1;
                }
            },
            b'/' if bytes[i..].starts_with(b"//") => {
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
            }
            b'/' if bytes[i..].starts_with(b"/*") => match find_block_comment_end(bytes, i + 2) {
                Some(end) => i = end,
                None => i += 1,
            },
            _ => i += 1,
        }
        depth = next;
    }
}

/// Skips a quoted string opened at `open`; `None` when the input ends first.
fn skip_string(bytes: &[u8], open: usize, quote: u8) -> Option<usize> {
    let mut j = open + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b if b == quote => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// Returns the offset just past a `*/` closing an open block comment.
fn find_block_comment_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j + 1 < bytes.len() {
        if bytes[j] == b'*' && bytes[j + 1] == b'/' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

/// Consumes one character unconditionally, then runs until just before a
/// comment start, the keyword `import`, or end of input.
fn scan_raw(bytes: &[u8], start: usize) -> usize {
    let len = bytes.len();
    let mut i = start + 1;
    while i < len {
        let at_token_start = bytes[i..].starts_with(b"//")
            || bytes[i..].starts_with(b"/*")
            || bytes[i..].starts_with(b"import");
        if at_token_start {
            break;
        }
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------
// Import statement parsing
// ---------------------------------------------------------------------------

/// Splits an import token's raw text at its module specifier:
/// `prefix` runs from the start through the opening quote, `suffix` from the
/// closing quote to the end. Depth counts braces only and is *not* clamped,
/// so a stray `}` pushes the counter negative and hides following quotes.
/// Any failure (no quote found, empty target) leaves the token as raw source.
fn parse_import(raw: &str) -> Option<TsNode> {
    let bytes = raw.as_bytes();
    let mut depth: i64 = 0;
    let mut i = 0;
    let quote = loop {
        let c = *bytes.get(i)?;
        let next = match c {
            b'{' => depth + 1,
            b'}' => depth - 1,
            _ => depth,
        };
        if next == 0 && (c == b'\'' || c == b'"') {
            break c;
        }
        depth = next;
        i += 1;
    };

    // takeWhile1: the target must hold at least one character.
    let target_start = i + 1;
    let mut target_end = target_start;
    while target_end < bytes.len() && bytes[target_end] != quote {
        target_end += 1;
    }
    if target_end == target_start {
        return None;
    }

    Some(TsNode::Import {
        prefix: raw[..=i].to_string(),
        target: raw[target_start..target_end].to_string(),
        suffix: raw[target_end..].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rendering a parsed program reproduces the input byte-for-byte.
    #[test]
    fn program_render_round_trips() {
        let src = "// c\nimport x from './y';\nconst z = 1;\n";
        let prog = parse_ts("a.ts", src);
        assert_eq!(prog.render(), src);
    }

    /// LexerSpec round-trip property, spot-checked over tricky inputs.
    #[test]
    fn round_trips_exactly() {
        let cases = [
            "",
            "const x = 1;",
            "import { foo } from 'bar'; const x = 1;",
            "import * as React from \"react\";\nconsole.log();",
            "// leading comment\nimport x from './y';\n/* block */ tail",
            "unterminated /* comment...",
            "import 'unterminated",
            "await import (`../../strings/${local}.json`)).default,",
            "import { \"}\" as brace } from 'lib'; const y = 2;",
            "ximport y from 'z';",
            "emoji ✨ and unicode ümlaut in raw text",
        ];
        for case in cases {
            let rebuilt: String = lex(case).into_iter().map(|t| t.raw).collect();
            assert_eq!(rebuilt, case);
        }
    }

    /// LexerSpec's import-token table: the first Import token, stripped.
    #[test]
    fn import_tokens_match_spec() {
        let cases: &[(&str, &str, &str)] = &[
            ("Basic single quotes", "import { foo } from 'bar'; const x = 1;", "import { foo } from 'bar';"),
            ("Basic double quotes", "import * as React from \"react\";\nconsole.log();", "import * as React from \"react\";"),
            (
                "Multiline",
                "import {\n  urls,\n  labels,\n} from '../../lib/constants';\n\n\n",
                "import {\n  urls,\n  labels,\n} from '../../lib/constants';\n",
            ),
            (
                "Multiline with trailing comma",
                "import {\n  foo,\n  bar,\n} from 'baz'; export const x = 1;\n",
                "import {\n  foo,\n  bar,\n} from 'baz';\n",
            ),
            ("Strings containing braces", "import { \"}\" as brace } from 'lib'; const y = 2;", "import { \"}\" as brace } from 'lib';"),
            ("Strings containing semicolons", "import { \";\" as semi } from 'lib'; ", "import { \";\" as semi } from 'lib';"),
            ("Block comments inside", "import { /* } */ a } from 'b'; ", "import { /* } */ a } from 'b';"),
            ("Line comments inside", "import {\n  a, // comment with }\n} from 'b'; ", "import {\n  a, // comment with }\n} from 'b';"),
            ("Terminated by newline", "import { x } from 'y'\nconst z = 1;", "import { x } from 'y'\n"),
            (
                "Await import terminated by )",
                "return {\n  locale,\n  strings: await import (`../../strings/${local}.json`)).default,\n};\n",
                "import (`../../strings/${local}.json`)",
            ),
            ("Await import terminated by ;", "const module = await import ('./heavy-module');\nlet x = 42", "import ('./heavy-module');"),
            ("Default + Named", "import React, { useState } from \"react\";", "import React, { useState } from \"react\";"),
            ("Side-effect import", "import './styles.css';", "import './styles.css';"),
            ("Type-only imports", "import type { User, Role } from './models'; const x = 1;", "import type { User, Role } from './models';"),
            ("Inline type imports", "import { createStore, type Store } from 'redux'; ", "import { createStore, type Store } from 'redux';"),
            ("Import attributes", "import data from './data.json' with { type: \"json\" }; ", "import data from './data.json' with { type: \"json\" };"),
            ("Aliased named import", "import { originalName as aliasName } from 'lib'; ", "import { originalName as aliasName } from 'lib';"),
            ("Namespace import", "import * as Utils from './utils'; ", "import * as Utils from './utils';"),
            ("Keywords as identifiers", "import { class as classSelector, delete as remove } from 'dom'; ", "import { class as classSelector, delete as remove } from 'dom';"),
            ("Empty named import", "import {} from './init-module'; ", "import {} from './init-module';"),
            ("String literal export names", "import { \"stupid-name\" as normal } from 'weird-lib'; ", "import { \"stupid-name\" as normal } from 'weird-lib';"),
        ];
        for (desc, input, expected) in cases {
            let token = lex(input)
                .into_iter()
                .find(|t| t.kind == TsTokenKind::Import)
                .unwrap_or_else(|| panic!("{desc}: no import token"));
            assert_eq!(token.raw.trim(), expected.trim(), "{desc}");
        }
    }

    /// ParserSpec: import tokens split into prefix | target | suffix.
    #[test]
    fn import_nodes_split_at_target() {
        let cases: &[(&str, &str, &str, &str)] = &[
            ("import * from '@/lib/utils'", "import * from '", "@/lib/utils", "'"),
            (
                "import { \"hello\" as hell } from \"./Context\"\n",
                "import { \"hello\" as hell } from \"",
                "./Context",
                "\"\n",
            ),
            ("import '../../tests/viewmodel-test';", "import '", "../../tests/viewmodel-test", "';"),
            ("await import ('../heavy-module');", "import ('", "../heavy-module", "');"),
            ("await import ('../../lib/extra').extras;", "import ('", "../../lib/extra", "')"),
        ];
        for (input, prefix, target, suffix) in cases {
            let program = parse_ts("test.ts", input);
            let imports: Vec<_> = program.cst.into_iter().filter(|n| n.is_import()).collect();
            assert_eq!(
                imports,
                vec![TsNode::Import {
                    prefix: (*prefix).to_string(),
                    target: (*target).to_string(),
                    suffix: (*suffix).to_string(),
                }],
                "{input}"
            );
        }
    }

    #[test]
    fn render_is_lossless_and_malformed_degrades_to_source() {
        let src = "// hi\nimport { a } from './b'; // trailing\nconst done = true;";
        let program = parse_ts("p.ts", src);
        let rendered: String = program.cst.iter().map(|n| n.render()).collect();
        assert_eq!(rendered, src);

        // No quote in the token: stays a Source node.
        let program = parse_ts("p.ts", "importantly, no import here");
        assert!(program.cst.iter().all(|n| matches!(n, TsNode::Source { .. })));
        // Empty target: degraded too.
        let program = parse_ts("p.ts", "import '';");
        assert!(program.cst.iter().all(|n| matches!(n, TsNode::Source { .. })));
    }
}
