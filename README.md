# deslop-rs

Rust rewrite of [deslop](../deslop) — a static import-graph analyzer for
TypeScript enforcing architecture rules written in YAML.

## Status: draft skeleton

The plumbing is ported and working: params/CLI, file discovery with
gitignore filtering, tsconfig alias parsing, baseline load/save (id format
byte-compatible with the Haskell original), problem ids, cycle detection
(Tarjan SCC), UI/reporting, parallel per-file pipeline.

Still to port (marked `TODO(port)` throughout):

- **TypeScript lexer/parser/CST** (`src/ts/cst.rs`) — lossless re-render after
  import rewrites; the largest piece.
- **Glob+ engine** (`src/glob_plus.rs`) — semantics pinned by docs/adr/0004,
  docs/adr/0005 and docs/GLOB+.md in the original repo.
- **Rulebook compiler + enforcer** (`src/rulebook/`) — forbids/allows/uses/
  exists clauses over the module graph.
- **Fix splicing** — applying import rewrites through the CST.

## Usage

```sh
cargo run -- check <project-dir>
cargo test
```

Commands mirror the original: `check`, `fix`, `baseline`.
