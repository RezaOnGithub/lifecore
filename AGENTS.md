# AGENTS.md

This repository is a Rust workspace for `lifecore`, an experimental graph
database with type declarations and lightweight version-control semantics. Nodes
and edges are appended over time, and a newer node can supersede an older node
through a parent relationship. Type declarations are themselves stored as graph
nodes, so the active schema can change across graph history.

The word `Event` in the code mostly means an append-log record for a node or an
edge. Do not infer that the project is primarily an event-sourcing framework;
the central model is a typed, versioned graph.

## Workspace Layout

- `lifecore`: core graph model, validation, contexts, node typing support,
  analyzer traits, and the `LifeCoreList` implementation.
- `lifecore-typecheck`: analyzer that checks graph contents against active
  `Type` declarations.
- `lifecore-test`: integration tests for runner-based analysis behavior.
- `lifecore-cli`: functional CLI and reusable support modules for versioned
  JSON state, durable entry locators, analysis-gated transactions, aliases,
  queries and explanations, type editing, messages and dismissals, recursive
  TOML sidecars, GraphViz output, and menu-driven interactive mode. It also
  contains the built-in `Interactive` analyzer.
- `lifecore-proc-macro`: placeholder procedural macro crate for future
  typed-body derive helpers.
- `lifecore-kman`, `lifecore-prod`: placeholder downstream analyzer/domain
  crates.
- `NOTES.md`: project conventions, known pitfalls, TODOs, and design notes.

## Commands

- Run the complete test suite with `cargo test --workspace`.
- Run focused CLI tests with `cargo test -p lifecore-cli`.
- Run the CLI with `cargo run -p lifecore-cli -- <arguments>`; use `--help` at
  any command level for the exact clap interface.
- Format with `cargo +nightly fmt`; `NOTES.md` says nightly rustfmt features are
  used even though the rest of the project targets stable Rust.
- In a dirty workspace, prefer a targeted formatting check such as
  `cargo +nightly fmt --check -p lifecore-cli` so unrelated user work is not
  rewritten.
- A useful strict check for CLI-owned code is
  `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`. The
  wider workspace currently emits pre-existing warnings, so do not assume a
  workspace-wide `-D warnings` invocation is clean.

## Conventions

- Read `NOTES.md` before making design-heavy changes.
- Comments tagged `SUS`, `TODO`, `NOTDO`, `COPY_PASTE`, `NOTE`, and `PERF` have
  project-specific meanings documented in `NOTES.md`.
- Be careful with event-context windows. Types and staleness are contextual: the
  right answer depends on both the inspected graph item and the historical
  context used to inspect it.
- Keep changes scoped. Several crates are intentionally skeletal, and broad
  refactors should wait until the model and analyzer interface are clearer.
- Preserve existing working-tree changes. Proposal documents and core APIs are
  often edited together while designs are being explored.
- Prefer deterministic iteration and presentation. Query-attested selection
  always uses a fingerprint-checked entry locator and rechecks query membership;
  table row numbers are never identities.

## Agent-Authored Development Conventions

- Add new tests only in a module matching `*::test::test2`: either the root
  `test::test2` module or a `test::test2` module nested under an internal
  module. Do not add implementation-adjacent tests unless the user explicitly
  requests another placement.
- Do not add Rust doc comments (`///`, `//!`) or doctests unless the user
  explicitly requests it.
- Let the reader of the code breathe: separate logical blocks with empty lines.
  Add a short comment explaining what a non-obvious block does and why.
- Every new code comment written by an agent must be a single-line comment
  prefixed exactly with `//A`. Keep comments concise and self-descriptive.
- Agent comments may use these annotation forms:
  - `//A HACK: ...` for a rare, narrowly scoped workaround and its reason.
  - `//A TODO: ...` for concrete deferred work, including what remains and why
    it was deferred. Do not introduce other annotation labels, such as `FIXME`
    or `NOTE`.
- If an agent adds a `//A TODO:`, its final handoff must explicitly state the
  impact and recommended next step.
- Do not rewrite or relabel pre-existing comments merely to conform to these
  conventions.

## Interface Terms

- `Context` describes a history window from the beginning of graph history to
  an exclusive end.
- `EntryContext` couples that window with a mandatory `thing` identifying the
  specific node or edge being inspected. Methods requiring a concrete
  append-log record accept this pointed type.
- `EntryIndex::before_and_including()` creates the smallest valid pointed
  context containing an entry. Use a current bounded window for inspection and
  `.absolute()` only when comparing entry identity across windows.
- A stale node is a node that has been superseded by a newer child within the
  chosen context. A stale edge is an edge whose endpoint has become stale.
- Fresh means non-stale in the chosen context. Freshness can change when the
  context window changes.
- `Node.e` and `Edge.e` are pointed contexts. `Node.parent` and edge endpoints
  are absolute `EntryIndex` identities; inspect them through the intended
  bounded `Context`.
- `Node.ty_name` is the stored type name. Resolve a stored declaration as
  `typing::Type` in the inspection window instead of attaching an eternally
  correct resolved type to the node.
- The CLI exposes zero-based indices only inside verified entry locators. It
  does not accept raw serialized context pairs.

## Validation And TypeCheck

- `basic_validation` enforces storage-level graph integrity: event/circumstance
  alignment, duplicate node/edge rules, references only to prior records, edges
  only between nodes, node parents only to nodes, and stale-edge prevention.
- Type checking is implemented as an analyzer in `lifecore-typecheck`, not as
  part of core validation. It interprets active nodes whose stored type is
  `Type`, matching `Type::TYPE_NAME`, in context.
- `ValueType` includes exact text and signed-integer literals for simple
  enumeration-like fields. Type syntax writes text literals in double quotes
  without escaping and writes integer literals as decimal numbers; literal
  checks require exact payload equality.
- A `FieldDecl` is stored as a three-element `Payload::Sequence` containing its
  name, `ValueType` syntax, and status as text values. Its separate text parser
  uses `name = type : status` syntax. Field statuses accept canonical title case
  or lowercase spellings and describe those spellings with literal `OneOf`
  alternatives.
- TypeCheck reports semantic inconsistencies and warnings for missing types,
  duplicate active type definitions, field policy violations, edge endpoint
  permissions, and unresolved override targets.
- Analyzer output is diagnostic. Core mutation APIs currently reject only
  `basic_validation` failures before committing a graph change.
- `add_mock` and `connect_mock` append a validated provisional suffix under one
  `Mock` circumstance. `apply` replaces that suffix with a canonical session
  circumstance. A failed core mutation leaves the receiver unchanged.
- `NodeQuery` evaluates nodes in one bounded view. `all`, `any`, and `none`
  have explicit empty-predicate identities, and
  `LifeCore::explain_query` returns predicate-aligned structured results.
  Missing, wrong-kind, and out-of-range field paths are non-matches rather than
  crashes.
- `AnalysisRunner` reads analyzer name/config pairs from
  `LifeCore::current_analysis_suite`, constructs them from separately registered
  factories, runs analyzer initialization, and analyzes every valid historical
  prefix. A configured analyzer without a registered factory is an error.
- The CLI registers `Type_Check` and `Interactive`. CLI mutations are stricter
  than direct core mutations: every individual provisional addition must pass
  basic validation and the complete analyzer suite without an Inconsistency
  before the operation may continue or be saved. Warnings and Suggestions are
  non-fatal.
- `Interactive` examines a window ending in a child node and suggests recreating
  eligible incoming or outgoing connections from its parent. It suppresses a
  suggestion when the opposite endpoint is stale or a fresh successor already
  has the equivalent connection.

## CLI State And Transactions

- The state file is pretty JSON with exactly two top-level values: `lifecore`
  contains `LifeCoreList`, and `cli` contains version 1 metadata: an immutable
  UUIDv7, state/entry-fingerprint/message-ID versions, aliases, dismissals,
  optional sidecar path, and reserved metadata.
- There is no migration path from older CLI state formats. Missing or
  unsupported version/UUID fields are load errors and must not trigger an
  automatic rewrite.
- The default path is `./LifeCoreState.json`. If `LIFECORE_STATE_FILE_PATH` is
  defined, that path is authoritative; failures must never fall back to the
  current-directory file.
- `init` creates the state file, registers `Interactive` and `Type_Check`, runs
  validation and analysis, applies any provisional state, and refuses to replace
  an existing file. TypeCheck's bootstrap Suggestion on an empty graph is
  expected and is non-fatal.
- Persisted state must never contain `Mock`. Loading aborts if it does, and
  saving checks again before atomic same-directory replacement. `init` uses a
  no-clobber atomic creation path. There is intentionally no file locking.
- Database mutations follow loaded -> staged clone -> analyzed -> applied ->
  atomically saved. `add`, `connect`, `type create`, `type override`,
  `analyzer add`, and `message apply` are dry runs unless `--commit` is present.
  Analyzer configuration is not a graph entry, but is still staged and analyzed.
- `--expect-head` accepts a stable entry locator or `EMPTY`; it requires
  `--commit`. It compares against the latest canonical entry, including an
  edge. Dynamic node reference `HEAD` is deliberately rejected here.
- CLI-state-only operations such as alias/dismissal changes and sidecar
  attach/detach persist immediately. They must not accidentally persist a
  provisional database assembled from a sidecar.
- Output modes are `auto`, `table`, `lines`, and `json`; `auto` selects tables
  only for a TTY. JSON listings include a view and items; transaction output
  includes its state, entries, and messages.
- Analyzer diagnostics are user-facing messages. Warning and Suggestion
  dismissals persist full versioned SHA3-512 IDs; relevance is sorted for
  identity while SuggestedAction order remains significant. Inconsistencies
  cannot be dismissed or presentation-filtered. Applying a Suggestion commits
  through the normal transaction path and dismisses it unless `--keep-message`.
- Logging uses `log` and `env_logger`, defaults to `trace`, and writes to
  stderr. Set `RUST_LOG` when quieter or more selective output is needed.

## CLI References, Queries, And Types

- An entry locator is `INDEX:FINGERPRINT`. The CLI displays 16 lowercase hex
  digits and accepts prefixes of 5–128 digits. Resolution verifies index,
  fingerprint, entry kind, and selected-view membership in that order.
- Aliases are case-sensitive C-style identifiers storing a full durable node
  locator. They never track a fresh successor automatically. An input
  containing `:` never falls back to alias lookup.
- `HEAD` is a reserved node reference resolving to the highest-index node in
  the effective graph, not the latest entry. It is accepted by node-reference
  positions such as `add --parent` and connection endpoints, but not stable
  identity positions such as `--at`, `--select`, or `--expect-head`.
- `query nodes all|any|none` constructs a core `NodeQuery`. Results are sorted
  by ascending entry index. `--select LOCATOR` rechecks membership in the same
  invocation; `--explain LOCATOR` may explain a non-member.
- `type create` and patch-based `type override` append ordinary `Type` nodes.
  Direct override resolves the unique active declaration by name and view;
  query-attested override is available for additional guards.
- Ordinary `add --field` and sidecar bodies share recursive TOML payload input:
  strings, signed integers, `{ sequence = [...] }`, and
  `{ ordered_set = [...] }`. Duplicate ordered-set members and unsupported TOML
  kinds are errors.

## Sidecars

- A sidecar is a TOML `[[entry]]` list. Entries are processed in file order;
  each creates one node followed by `connect_to` edges and then
  `connect_back_from` edges. Bodies use the same recursive text, signed-integer,
  sequence, and ordered-set converter as ordinary `add --field`.
- Sidecar aliases are temporary until commit. They may reference persisted
  aliases or aliases from earlier entries, and become persisted only after a
  successful `sidecar commit`.
- At the beginning of each entry, sidecar `HEAD` is captured from the graph
  before its node append. The capture remains fixed for that entry's parent and
  connection references. Other connection references resolve after the
  optional temporary alias is installed, allowing intentional self-links.
- `sidecar attach` checks only that the path exists and stores an absolute path;
  it deliberately does not parse the file. Later ordinary invocations rebuild
  and analyze the provisional suffix in memory.
- `sidecar detach` must not read or parse the sidecar. This is the escape hatch
  for deleted, unreadable, or invalid sidecars, and attach followed by detach
  with no other saved change should restore identical state-file bytes.
- While attached, singular mutations, analyzer additions, suggestion
  application, alias mutation, and dismissal mutation are forbidden. Read-only
  commands operate on the analyzed provisional graph. Only `sidecar commit` may
  apply and save that suffix; the TOML file itself is never deleted.
- A basically valid but analysis-inconsistent sidecar remains available to
  `check` and detach, but not browsing. A sidecar that cannot be parsed,
  basically validated, or supplied with analyzer factories permits only detach.

## CLI Code Map

- `lifecore-cli/src/lib.rs`: clap interface, dispatch, transaction
  orchestration, queries, aliases, types, messages, sidecar preparation,
  GraphViz generation, output, and interactive workflows.
- `state.rs`: serialized state envelope, path resolution, Mock rejection, and
  atomic persistence.
- `locator.rs`: entry-fingerprint encoding, strict locator/reference parsing,
  and view-aware resolution.
- `messages.rs`: canonical message IDs, universe deduplication, visibility,
  resolution, and dismissal records.
- `query.rs`: textual predicate grammar for core `NodeQuery`.
- `payload_input.rs`: shared recursive TOML-to-`Payload` conversion.
- `sidecar.rs`: sidecar TOML schema; staging uses the shared converter and
  transaction services in `lib.rs`.
- `interactive_analyzer.rs`: reusable `Interactive` analyzer and factory.

Keep validation and analysis in the shared staging helpers. Duplicating mutation
logic in a command handler is likely to omit rollback, analyzer execution, Mock
handling, or diagnostic presentation.

## Current Risks

- Some `LifeCore` methods still panic when passed invalid, out-of-range, or
  wrong-kind pointed contexts. CLI resolution must validate before constructing
  contexts. Field-query traversal itself is now panic-free for missing and
  out-of-range paths.
- Type meaning can shift across history. Do not cache a resolved `Type` without
  also retaining the inspection view.
- Validation and TypeCheck are currently correctness-first and repeatedly scan
  graph history.
- The analyzer API is still thin. Downstream analyzer crates may need richer
  graph-query helpers so they can avoid rebuilding common traversals.
- User-authored time payloads remain deferred. The shared TOML grammar rejects
  booleans, floats, bare arrays, arbitrary tables, and TOML date-time values.
- The built-in TypeCheck bootstrap Suggestion remains the supported way to
  create the initial `Type` declaration from a fresh CLI state.
- SVG generation requires the external `dot` executable. `graphviz --dot` writes
  DOT directly and has no GraphViz runtime dependency.
