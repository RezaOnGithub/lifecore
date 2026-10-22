# AGENTS.md

LifeCore is an experimental Rust database with an append-only, typed graph.
Remaining work is in [PLAN](docs/PLAN.md). Read the document that owns a
behavior before implementing it:

- Core: [SPEC](docs/SPEC.md) (graph semantics, validation, inspection),
  [ANALYZERS](docs/ANALYZERS.md) (execution, finding identity, dismissal), and
  [MESSAGES](docs/MESSAGES.md), which traces the current analyzer message flow.
  [PREFIX-VALIDITY](docs/PREFIX-VALIDITY.md) records why every prefix is
  validated, and [PERFORMANCE](docs/PERFORMANCE.md) what checking costs.
- Application: [INTERFACES](docs/INTERFACES.md) (architecture, CLI, RPC
  envelope, server access, analysis lifetime), [RPC](docs/RPC.md) (methods and
  errors), [FILES](docs/FILES.md) (proposal encoding and plan IDs),
  [STORAGE](docs/STORAGE.md) (database selection, snapshot, locking,
  publication), and [SUGGESTIONS](docs/SUGGESTIONS.md) (suggestion export and
  acknowledgment).
- Browser client: [CLIENT](docs/CLIENT.md) (contract, values, build, tests)
  and [UI](docs/UI.md) (presentation decisions).
- [docs/backlog](docs/backlog/) keeps the backlogged collection and receipt
  contracts.

Keep the graph/analyzer libraries and the application service synchronous.
Persistence belongs to the shared application layer; async HTTP belongs to the
Rust server adapter, which runs each request on a blocking task without a
queue. HTTP, the CLI, and JSON/TOML change files share one set of Serde
requests, results, errors, and proposals. The frontend is React/TypeScript.

## Compatibility

This is a sketch. Backwards compatibility is not a concern at all: APIs, formats,
persisted representations, and commands may change freely within the requested
scope. Do not add compatibility shims, legacy readers, or migration machinery.
This explicitly includes file formats and encodings: the state snapshot, JSON/TOML
change files, plan-ID tokens, and RPC payloads. Existing files may stop loading
after a change; do not add version detection, fallbacks, or migrations for them.
Preserve historical correctness under the chosen model and unrelated user work.

## Checks

Active workspace: `lifecore` (root), `lifecore-typecheck`, `lifecore-dag-check`,
`lifecore-app`, `lifecore-cli`, and `lifecore-server` (the Axum adapter behind
`lifecore serve`; its `build.rs` embeds the React client in
`lifecore-server/ui`). Shared checking and atomic in-memory commits
live in `lifecore::transaction`; persistence belongs to the synchronous
application service `lifecore_app::service::Service`, shared by HTTP and CLI.

- Tests: `cargo test --workspace` or `cargo test -p <package>`. Where Graphviz
  is installed, `LIFECORE_UI=skip cargo test -p lifecore-app -- --include-ignored`
  also runs the rendering tests that need `dot`.
- Formatting: `cargo +nightly fmt --check -p <package>`; omit `--check` to format.
- Lint: `cargo clippy -p <package> --all-targets --no-deps -- -D warnings`.
- Rust-only runs: building `lifecore-server`, and so `lifecore-cli` and the
  workspace, bundles the client and fails until `npm ci` has run in
  `lifecore-server/ui`. Prefix any Cargo command with `LIFECORE_UI=skip` to
  embed a placeholder page instead, e.g. `LIFECORE_UI=skip cargo test --workspace`.
- Frontend, in `lifecore-server/ui` after `npm ci`: `npm run typecheck`
  (`tsc --noEmit`, including the fixtures), `npm run lint` (Biome; `npx biome
  check --write .` formats), `npm test` (`node --test`), and `npm run e2e`
  (Playwright in Firefox; it builds `lifecore` with Cargo, and
  `npx playwright install firefox` fetches the browser once).
- Fixtures: `cargo test -p lifecore-server` rewrites stale files in
  `lifecore-server/ui/src/fixtures/` and then fails. Run the type-check, change
  `contract.ts` with the Rust shape if it fails, and commit both.

Rust targets stable; rustfmt requires nightly. The active workspace passes
Clippy with `-D warnings`. Keep checks and formatting scoped to the change.

## Invariants

The documents above own the contracts; this list names the hazards an edit is
most likely to trip, with the owning section.

- Views are bounded. `View::prefix(end_exclusive)` only narrows `[0, end_exclusive)`;
  lookups require an `EntryId`, not a position; a prefix-bounded value reveals
  nothing past its boundary through any channel, including `Debug`, `Serialize`,
  `PartialEq`, `Hash`, ordering, and error text. The derived staleness index is
  never serialized, and every query compares it with the view boundary
  ([SPEC §9](docs/SPEC.md#9-inspection-and-queries)).
- Entry identity is separate from its interpretation in a view. Freshness and
  active declarations depend on the view; never inspect a future suffix or cache
  resolved types without the source and boundary. Edges keep exact endpoints and
  are never retargeted ([SPEC §4](docs/SPEC.md#4-edges-and-the-relationship-graph),
  [§5](docs/SPEC.md#5-current-and-historical-data)).
- `ancestors` starts at the parent, `descendants` excludes the start, `root_of`
  returns a root's own identity, and `fresh_successors` includes a current start.
  `ResolutionComparison` exposes `base_view` and `target_view`; appended positions
  are `[base_view.end_exclusive, target_view.end_exclusive)`. Construct
  `ResolutionDifference` only from different resolutions of the same entry
  ([SPEC §9](docs/SPEC.md#9-inspection-and-queries)).
- All mutations go through `transaction::prepare` and a `PreparedTransaction`
  commit. `Checked::Rejected` keeps the analysis and operation mapping but no
  graph. Core mutation failures leave the receiver unchanged; commit compares
  database identity, original entries, and the exact ordered suite, never
  serialization or hashes. `EntryChange` is the only operation shape a proposal
  or suggestion can carry; `Change` adds the suite change.
  `Reference::Added(n)` addresses an earlier operation of the plan
  ([SPEC §7.4](docs/SPEC.md#74-failure-and-persistence)).
- Only a `LockedSnapshot`, read under the writer lock for its own state path,
  can replace a published snapshot; unlocked reads are previews
  ([STORAGE §4](docs/STORAGE.md#4-writers)).
- Structural validation failures stay distinct from analyzer diagnostics.
  TypeCheck is a configured analyzer with a fixed built-in metatype and no
  bootstrap node. Empty, correctly shaped collections are present
  ([SPEC §6.1](docs/SPEC.md#61-built-in-metatype), [§7](docs/SPEC.md#7-mandatory-validation)).
- Analyzers are read-only, prefix-bounded, and deterministic for fixed graph,
  implementation, configuration, and `EvaluationInputs`; nothing reads the clock
  implicitly, and missing factories are errors. Keep full-prefix replay, typed
  findings asserted structurally in tests, and third-party `dyn Analyzer`
  implementations ([SPEC §8](docs/SPEC.md#8-analyzers),
  [ANALYZERS §1](docs/ANALYZERS.md#1-execution-and-normalization)).
- Finding identity is the complete `FindingKey`: database, registered name, exact
  version, verbatim configuration string, category, and the opaque certificate.
  Shared code never parses certificates or substitutes digests. Boundaries,
  wording, and actions belong to the observation, which records the contiguous
  prefix range over which its content was emitted
  ([ANALYZERS §2](docs/ANALYZERS.md#2-finding-identity-and-observations)).
- Dismissal, `sticky`, and display filtering never affect the inconsistency
  gate; non-emission or supersession never establishes resolution
  ([ANALYZERS §3](docs/ANALYZERS.md#3-presentation-and-dismissal)).
- Test history-sensitive changes at several boundaries, including before and
  after the relevant append.

## Editing conventions

- Use `<module>.rs` for single-file modules. When, and only when, a module spans
  multiple files, use `<module>/mod.rs` with its supporting files in that directory.
  This also applies to tests: keep multi-file scaffolding in `test/mod.rs` and
  single-file agent-authored tests in `test/test2.rs`. `lib.rs` remains the crate root.
- End every agent-authored commit message with one trailer per contributing agent:
  `Co-authored-by: Codex <codex@openai.com>` for Codex, and
  `Co-Authored-By: Claude <model> <noreply@anthropic.com>` for Claude, naming the
  model, e.g. `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Add agent-authored tests only in a module matching `*::test::test2`.
- Comment non-obvious invariants, boundary choices, and ordering constraints,
  not self-explanatory code. Mark concrete deferred work `// TODO:` and
  necessary workarounds `// HACK:`, and note material ones in the handoff.
- Treat every way a value can be observed as part of its contract, not only its
  intended accessors: trait implementations (`Debug`, `Display`, `Serialize`,
  `Hash`, `PartialEq`), error and panic messages, and ordering. A prefix-bounded
  value must reveal nothing past its boundary through any of them. Derived data
  must not take part in equality, hashing, or serialization. Hand-write these
  implementations on types that carry derived or boundary-sensitive data, and
  test the boundary through each channel that matters.
- Give each documented contract one owning document; elsewhere, link to it
  instead of restating it. Keep this file to guidance for every change and to
  invariants the code already upholds.
