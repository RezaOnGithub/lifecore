# AGENTS.md

This repository is a Rust workspace for `lifecore`, an experimental typed,
versioned graph database. Nodes and edges are appended over time. A newer node
can supersede an older node through a parent relationship. Type declarations are
also graph nodes, so schema meaning can change across graph history.

## Workspace

- `lifecore`: graph model, validation, contexts, typing support, analyzer
  traits, and `LifeCoreList`.
- `lifecore-typecheck`: analyzer for checking graph contents against active
  `Type` declarations.
- `lifecore-test`: integration tests.
- `lifecore-cli`: CLI and its support modules. Work in progress.
- `lifecore-kman`, `lifecore-prod`: placeholder downstream crates. Work in
  progress.
- `lifecore-proc-macro`: placeholder for future typed-body derive helpers.
  Deferred.

Several crates and interfaces are deliberately incomplete. Keep changes scoped
unless the task explicitly calls for broader design work.

## Commands

- Full test suite: `cargo test --workspace`
- Package tests: `cargo test -p <package>`
- CLI: `cargo run -p lifecore-cli -- <arguments>`
- Formatting: `cargo +nightly fmt`
- Formatting check: `cargo +nightly fmt --check -p <package>`
- Strict CLI lint:
  `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`

Nightly is required for rustfmt features; the project otherwise targets stable
Rust. The wider workspace has pre-existing warnings, so do not assume a
workspace-wide `-D warnings` check is clean. In a dirty tree, prefer targeted
formatting and checks that do not rewrite unrelated files.

## Core Model

- `Context` is a graph-history window from the beginning of history to an
  exclusive end.
- `EntryContext` is a `Context` plus a mandatory `thing` identifying the node or
  edge being inspected.
- `EntryIndex::before_and_including()` creates the smallest pointed context
  containing an entry.
- Use the intended bounded context for inspection. Use `.absolute()` only when
  comparing entry identity across windows.
- `Node.e` and `Edge.e` are pointed contexts. `Node.parent` and edge endpoints
  are absolute `EntryIndex` identities.
- A node is stale when a newer child supersedes it within the chosen context. An
  edge is stale when an endpoint is stale. Freshness is therefore contextual.
- `Node.ty_name` is a stored name, not a permanently resolved type. Resolve its
  active `typing::Type` in the inspection window.
- Type meaning may shift over history. Do not cache a resolved type without its
  inspection context.

Many subtle bugs here are context-window or off-by-one errors. When changing
history traversal, test more than one window around the relevant entry.

## Validation And Analysis

- `basic_validation` protects storage-level graph integrity.
- Type checking is analyzer behavior, not core validation.
- Analyzer diagnostics and validation failures have different semantics; do not
  silently turn one into the other.
- Core mutation helpers must leave the receiver unchanged on failure.
- Provisional `Mock` entries form a suffix that `apply` turns into canonical
  session entries.
- `AnalysisRunner` constructs configured analyzers from registered factories and
  analyzes historical prefixes. Missing factories are errors.
- Validation and type checking currently favor correctness over performance and
  may repeatedly scan history.

Keep validation, analysis, and rollback behavior in shared staging paths.
Command handlers and other front ends should not duplicate mutation pipelines.

## Code And Comment Style

- Add agent-authored tests only in a module matching `*::test::test2`: either
  the root `test::test2` module or one nested under an internal module.
- Prefix every agent-authored comment with `//A`. This reserves ordinary `//`
  comments for reviewers and future editors. Longer comments may use multiple
  consecutive `//A` lines; do not use `/* ... */`.
- Separate logical blocks with blank lines when it improves readability.
- Comment non-obvious reasoning: invariants, context choice, ordering
  constraints, safety assumptions, workarounds, and intentionally rejected
  alternatives.
- Substantial new logic should normally contain comments at its important
  reasoning boundaries. Comments are not required when the code is already
  self-explanatory; usefulness matters more than comment count.
- Do not add Rust doc comments (`///`, `//!`) or doctests unless requested. This
  restriction does not discourage explanatory `//A` comments.
- Use `//A TODO: ...` for concrete deferred work, stating what remains and why
  it is deferred. Mention any material impact and recommended next step in the
  final handoff.
- Use `//A HACK: ...` for workarounds and explain why it is necessary.
- Do not rewrite existing comments solely to enforce these conventions.
