# deslop-rs

Rust rewrite of [deslop](../deslop) — a static import-graph analyzer for
TypeScript enforcing architecture rules written in YAML.

## Status: port complete

Everything algorithmic from the Haskell original is ported, and verified
against it: the binary reproduces all 18 of the original suite's fixture
goldens byte-for-byte (check transcripts, baseline files, the rulebook
error report, and fix snapshots of rewritten files).

- **Glob+ engine** — variable capture with casing agreement, clause
  hydration, `..` resolution, exclude patterns; compiled error-for-error
  like the original's.
- **Rulebooks** — YAML under `deslop/rules/*.yaml`; `forbids`/`allows`/
  `uses`/`exists` clauses, direct or transitive, with variables bound by the
  rule's target. Broken rulebooks abort the run with every failure grouped
  by file, rule, and field.
- **Built-in lints** — `no-relative-imports` (rewrites through the tsconfig
  alias mapping and splices the fix back losslessly) and `no-import-cycles`
  (shortest loop per strongly-connected component).
- **Pipeline** — parallel per-file parse/lint, module graph with alias-mapped
  ids, transitive enforcement, duplicate compaction (shortest chain absorbs
  the rest), baselines whose ids interoperate with the Haskell original's.

## Usage

```sh
cargo run -- check <project-dir>    # report problems
cargo run -- fix <project-dir>      # rewrite relative imports to aliases
cargo run -- baseline <project-dir> # silence the current problem set
```

A rulebook looks like:

```yaml
id: clean-architecture
name: Clean Architecture
rules:
  - id: domain-is-pure
    description: The domain imports only itself.
    target: "@/domain/**"
    forbids:
      - import: "**"
    allows:
      - import: "@/domain/**"
    fix: Remove the import; domain stays pure.
```

## Development

```sh
cargo build
cargo test                 # 104 unit tests
cargo clippy --all-targets # zero warnings

# Differential QA: run the binary over the Haskell suite's fixture goldens
# (expects the original repo checked out beside this one).
DESLOP_HS=../deslop python3 scripts/qa_goldens.py
```

`DESLOP_TRANSCRIPT=1` swaps CLI styling for the original test double's
`[Style]` tags — what the golden harness compares against.

## Layout

Mirrors the original's layering, one module per pipeline stage:

```
src/pipeline.rs       orchestration — the only module that knows the whole run
src/Deslop-equivalents
  ast.rs              language-agnostic module + import edges
  code_graph.rs       reachability, shortest paths, SCCs
  glob_plus/          the Glob+ pattern language
  rulebook/           dto → compiler → book → loader
  rule_enforcer.rs    rules against the graph
  lint/               built-in lints
  problem*.rs         problems, shrinker, formatter
src/ts/               the TypeScript frontend: cst, config, iterator,
                      module_resolver (ids and alias mapping)
src/effects/          cli surface
```
