# AGENTS.md

This repository is a Rust workspace for `lifecore`, an experimental graph
database with type declarations and lightweight version-control semantics.
Nodes and edges are appended over time, and a newer node can supersede an older
node through a parent relationship. Type declarations are themselves stored as
graph nodes, so the active schema can change across graph history.

The word `Event` in the code mostly means an append-log record for a node or an
edge. Do not infer that the project is primarily an event-sourcing framework;
the central model is a typed, versioned graph.

## Workspace Layout

- `lifecore`: core graph model, validation, contexts, node typing support,
  analyzer traits, and the `LifeCoreList` implementation.
- `lifecore-typecheck`: analyzer that checks graph contents against active
  `Node_Type` declarations.
- `lifecore-test`: integration tests for runner-based analysis behavior.
- `lifecore-cli`: placeholder CLI crate. Most GraphViz code is currently
  commented out.
- `lifecore-derive`: placeholder procedural macro crate for future typed-body
  derive helpers.
- `lifecore-kman`, `lifecore-prod`: placeholder downstream analyzer/domain
  crates.
- `NOTES.md`: project conventions, known pitfalls, TODOs, and design notes.

## Commands

- Run tests with `cargo test`.
- Format with `cargo +nightly fmt`; `NOTES.md` says nightly rustfmt features are
  used even though the rest of the project targets stable Rust.

## Conventions

- Read `NOTES.md` before making design-heavy changes.
- Comments tagged `SUS`, `TODO`, `NOTDO`, `COPY_PASTE`, `NOTE`, and `PERF`
  have project-specific meanings documented in `NOTES.md`.
- Be careful with `EventContext` windows. Types and staleness are contextual:
  the right answer depends on both the inspected graph item and the historical
  context used to inspect it.
- Keep changes scoped. Several crates are intentionally skeletal, and broad
  refactors should wait until the model and analyzer interface are clearer.

## Current Risks

- Many `LifeCore` methods panic when passed invalid, unpointed, out-of-range, or
  wrong-kind contexts. Tests document some of these crash paths.
- `NodeExt` eagerly includes a resolved `NodeType`, but that type can shift over
  time. This is convenient for callers yet easy to misuse.
- Validation and TypeCheck are currently correctness-first and repeatedly scan
  graph history.
- The analyzer API is still thin. Downstream analyzer crates may need richer
  graph-query helpers before they can avoid rebuilding common traversals.
