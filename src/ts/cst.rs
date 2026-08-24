//! Ports `TypeScript/CST.hs` + `TypeScript/Lexer.hs` + `TypeScript/Parser.hs`
//! + `TypeScript/Tokens.hs`.
//!
//! The original is a hand-written megaparsec lexer/parser producing a lossless
//! CST that can be re-rendered byte-for-byte after import rewrites. This is
//! the single largest porting effort in deslop-rs. TODO(port).

/// A parsed module whose trivia (comments, whitespace) round-trips exactly.
pub struct CsT {
    /// Original source bytes; rendering a CST without edits must return these.
    pub source: String,
}

pub struct ParseResult {
    pub program: Result<CsT, String>,
}

/// Placeholder: parses nothing yet. The port must reproduce:
/// - token stream incl. trivia attachment
/// - import/export declaration extraction
/// - lossless render (`render . cst`)
pub fn parse_ts(path: &str, content: String) -> ParseResult {
    let _ = path;
    ParseResult { program: Ok(CsT { source: content }) }
}
