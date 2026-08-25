//! Ports `Deslop/Rulebook.hs` and friends: rulebooks are YAML files under
//! `deslop/rules/*` with forbids / allows / uses / exists clauses.
//!
//! - `dto` is the raw YAML shape exactly as written;
//! - `compiler` validates and lowers it into `book`, fixing each clause's
//!   polarity at the one site that knows it;
//! - `loader` is the only part that touches the filesystem;
//! - the enforcer (`crate::rule_enforcer`) checks compiled rules against the
//!   module graph.

pub mod book;
pub mod compiler;
pub mod dto;
pub mod loader;

pub use book::{AllowsClause, ExistsClause, ForbidsClause, Rule, Rulebook, UsesClause};
pub use loader::load_rulebook;
