//! deslop-rs — Rust rewrite of deslop, a static import-graph analyzer for
//! TypeScript enforcing architecture rules written in YAML.
//!
//! Status: the pipeline runs end to end — Glob+ rulebook compilation and
//! enforcement, both built-in lints (relative imports with fix splicing,
//! import cycles), the compaction shrinker, and baseline round-tripping.
//! Remaining `TODO(port)` markers note parity gaps (report goldens, NO_COLOR).

// Stubs are intentionally unused until their engines are ported.
#![allow(dead_code)]

pub mod ast;
pub mod baseline;
pub mod casing;
pub mod code_graph;
pub mod effects;
pub mod git_ignore;
pub mod glob_plus;
pub mod lint;
pub mod params;
pub mod pipeline;
pub mod problem;
pub mod problem_formatter;
pub mod problem_shrinker;
pub mod rule_enforcer;
pub mod rulebook;
pub mod ts;
pub mod types;
pub mod ui;
pub mod utils;

pub use pipeline::run_deslop;
