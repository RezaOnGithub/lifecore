# AGENTS.md

LifeCore is an experimental Rust database with an append-only, typed graph.
The cleanup direction and open decisions are in [docs/PLAN.md](docs/PLAN.md).
`docs/SPEC.md` and `docs/FILES.md` describe proposed contracts, not necessarily
current behavior. Stages 0–4 and the review checkpoint before stage 5 are complete:
the old CLI/sidecars and context wrappers are removed. Values are scalars, lists,
and records. Bounded inspection, comparison, and Graphviz export use `View`.
Analyzer reports separate complete finding keys from observations; dismissal is
serializable display state and suggestions prepare explicit in-memory transactions.
The libraries provide graph operations, analysis, transactions, and Serde support,
with no filesystem persistence or authoring envelopes. Stage 5 creates
`lifecore-cli` for storage, TOML authoring, timestamp conversion, and durable
receipts. Document collections are backlogged.

## Compatibility

This is a sketch. Backwards compatibility is not a concern at all: APIs, formats,
persisted representations, and commands may change freely within the requested
scope. Do not add compatibility shims, legacy readers, or migration machinery.
Preserve historical correctness under the chosen model and unrelated user work.

## Checks

Active workspace: `lifecore` (root), `lifecore-typecheck`, and
`lifecore-dag-check`. Shared checking and atomic in-memory commits live in
`lifecore::transaction`; persistence belongs to the planned `lifecore-cli`.

- Tests: `cargo test --workspace` or `cargo test -p <package>`.
- Formatting: `cargo +nightly fmt --check -p <package>`; omit `--check` to format.
- Lint: `cargo clippy -p <package> --all-targets --no-deps -- -D warnings`.

Rust targets stable; rustfmt requires nightly. The active workspace passes
Clippy with `-D warnings`. Keep checks and formatting scoped to the change.

## Invariants

- `LifeCoreList::view()` borrows immutable entries; `View::prefix(end_exclusive)`
  only narrows `[0, end_exclusive)`. Lookups require an `EntryId`, not a position.
  Comparison exposes `base_view` and `target_view`; appended positions are
  `[base_view.end_exclusive, target_view.end_exclusive)`.
- Entry identity is separate from its interpretation in a bounded history view.
  Freshness and active type declarations depend on that view; never inspect a
  future suffix or cache resolved types without the source and boundary.
- Edges retain exact endpoint identities. Superseding an endpoint makes an edge
  stale in views containing the successor; it does not retarget the edge.
- Test history-sensitive changes at multiple boundaries, including before and
  after the relevant append.
- All mutations go through `transaction::prepare` and a `PreparedTransaction` commit.
  `Reference::Added(n)` addresses an earlier operation within that plan;
  `Reference::Existing(id)` retains exact identity across views.
- Core mutation failures leave the receiver unchanged. Preserve atomic composite
  writes and every-prefix checks through shared staging and validation. Commit
  compares database identity, the original entry count/content, and the exact
  original ordered analyzer suite; serialization and hashes do not define equality.
  Graph deserialization validates structure before permitting inspection.
- Keep structural validation failures distinct from analyzer diagnostics.
  TypeCheck remains a configured analyzer. Its metatype is fixed and built in;
  no bootstrap node is needed. Empty correctly shaped collections are present.
- Preserve full-prefix analyzer execution, typed inner findings, normalization,
  and third-party/dynamically dispatched implementations. Keep analyzers and
  their private findings/factories together; shared orchestration uses traits.
- Analyzer inspection is read-only and prefix-bounded. Initialization has no
  database access. Results must be deterministic for fixed graph, implementation,
  configuration, and evaluation inputs. `EvaluationInputs` contains only
  `evaluation_time: Option<jiff::Timestamp>` and defaults to `None`. Freeze it
  across initialization and replay; time-dependent analyzers error when absent.
  Neither runner nor analyzer implicitly reads the clock. Missing factories are errors.
  `evaluated_view: None` means initialization; `Some` includes prefix zero.
- Dismissal and display filtering cannot affect the inconsistency gate.
  Non-emission or supersession alone does not establish resolution.
- Finding equality compares database identity, registered analyzer name, exact
  `semver::Version`, original configuration string, category, and complete opaque
  `analyzer_key`. Factories receive configuration unchanged; behaviorally equivalent
  spellings still have distinct identities. Built-ins parse their package versions
  and deterministically JSON-encode typed variants, exact subjects, and details.
  Normalize distinguishing domain details inside the analyzer. Shared code does
  not parse finding keys or replace equality with digests. Ordinary boundaries,
  wording, evaluation inputs, and suggested actions belong to observations.
  `retain_in_current_report` does not assert current applicability or resolution.
  Dismissal reasons and optional notes never establish resolution or affect acceptance.

## Editing conventions

- Every file-backed module in the active crates uses `<module>/mod.rs`, including
  tests; `lib.rs` remains the crate root. Keep test scaffolding in `test/mod.rs`
  and agent-authored tests under `test/test2/mod.rs`.
- End every agent-authored commit message with
  `Co-authored-by: Codex <codex@openai.com>`.
- Add agent-authored tests only in a module matching `*::test::test2`.
- Prefix agent-authored Rust comments with `//A`; do not add block comments,
  Rust doc comments, or doctests unless requested. Leave existing comment styles
  alone unless their content needs changing.
- Explain non-obvious invariants, boundary choices, and ordering constraints;
  avoid narrating self-explanatory code.
- Use `//A TODO:` for concrete deferred work and `//A HACK:` for a necessary
  workaround. Explain the impact and next step in the handoff when material.
