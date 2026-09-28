# AGENTS.md

LifeCore is an experimental Rust database with an append-only, typed graph.
Remaining work is in [docs/PLAN.md](docs/PLAN.md). [docs/SPEC.md](docs/SPEC.md)
defines graph semantics, and [docs/MESSAGES.md](docs/MESSAGES.md) traces the
current analyzer message flow. The proposed application is specified in
[docs/INTERFACES.md](docs/INTERFACES.md) (commands, RPC, database selection,
React), [docs/FILES.md](docs/FILES.md) (proposal encoding, plan IDs, snapshot
publication), and [docs/SUGGESTIONS.md](docs/SUGGESTIONS.md) (suggestion export
and acknowledgment). Read the relevant document before implementing that behavior.

Keep the graph/analyzer libraries and the application service synchronous.
Persistence belongs to the shared application layer; async HTTP and queues belong
to the Rust server adapter. HTTP, the CLI, and JSON/TOML change files share one
set of Serde requests, results, errors, and proposals. The frontend is
React/TypeScript.

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
`lifecore-app`, and `lifecore-cli`. Shared checking and atomic in-memory commits
live in `lifecore::transaction`; persistence belongs to the synchronous
application service `lifecore_app::service::Service`, shared by HTTP and CLI.
Add package/frontend checks when those components are introduced.

- Tests: `cargo test --workspace` or `cargo test -p <package>`.
- Formatting: `cargo +nightly fmt --check -p <package>`; omit `--check` to format.
- Lint: `cargo clippy -p <package> --all-targets --no-deps -- -D warnings`.

Rust targets stable; rustfmt requires nightly. The active workspace passes
Clippy with `-D warnings`. Keep checks and formatting scoped to the change.

## Invariants

- `LifeCoreList::view()` borrows immutable entries; `View::prefix(end_exclusive)`
  only narrows `[0, end_exclusive)`. Lookups require an `EntryId`, not a position.
  `ResolutionComparison` exposes `base_view` and `target_view`; appended positions
  are `[base_view.end_exclusive, target_view.end_exclusive)`.
- Entry identity is separate from its interpretation in a bounded history view.
  Freshness and active type declarations depend on that view; never inspect a
  future suffix or cache resolved types without the source and boundary.
  The derived staleness index is never serialized, and every query compares it
  with the view boundary ([SPEC §9](docs/SPEC.md#9-inspection-and-queries)).
  `View::resolve(id)` returns a `Resolution` holding the entry, freshness, and
  explicit `TypeResolution` outcome.
  Construct `ResolutionDifference` only from different resolutions of the same
  immutable entry; its read-only accessors preserve that invariant.
- `ancestors` starts at the parent and walks toward the root; `descendants` also
  excludes the starting revision. `root_of` returns a root's own identity, and
  `fresh_successors` includes the starting revision when current. All traversals
  use exact identities visible in the selected view.
- Edges retain exact endpoint identities. Superseding an endpoint makes an edge
  stale in views containing the successor; it does not retarget the edge.
- Test history-sensitive changes at multiple boundaries, including before and
  after the relevant append.
- All mutations go through `transaction::prepare` and a `PreparedTransaction` commit.
  `prepare` returns `Checked`: `Accepted(PreparedTransaction)`, the only
  committable form, or `Rejected(Rejection)`, which keeps the analysis and
  operation mapping but no candidate graph.
  `Reference::Added(n)` addresses an earlier operation within that plan;
  `Reference::Existing(id)` retains exact identity across views.
- Only a `LockedSnapshot`, read under the writer lock for its own state path,
  can replace a published snapshot; unlocked reads are for previews only.
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
  Typed analyzers return `Vec<Self::MSG>` and own `convert_message`; the adapter
  produces `Vec<Message>`. Assert typed variants and details in analyzer tests;
  presentation text must not replace structured domain findings. The runner
  returns `AnalysisOutput` with observations and the complete evaluation context.
- Analyzer inspection is read-only and prefix-bounded. Initialization has no
  database access. Results must be deterministic for fixed graph, implementation,
  configuration, and evaluation inputs. `EvaluationInputs` contains only
  `evaluation_time: Option<jiff::Timestamp>` and defaults to `None`. Freeze it
  across initialization and replay; time-dependent analyzers error when absent.
  Neither runner nor analyzer implicitly reads the clock. Missing factories are errors.
  `EvaluatedView::Initialization` identifies initialization;
  `EvaluatedView::Prefix(view)` identifies the exact prefix, including zero.
- Dismissal and display filtering cannot affect the inconsistency gate.
  Non-emission or supersession alone does not establish resolution.
- Finding equality compares database identity, registered analyzer name, exact
  `semver::Version`, original configuration string, category, and complete opaque
  `analyzer_certificate: AnalyzerCertificate`, a String newtype. Factories receive
  configuration unchanged; behaviorally equivalent spellings still have distinct
  identities. Built-ins parse their package versions
  and deterministically JSON-encode typed variants, exact subjects, and details.
  Normalize distinguishing domain details inside the analyzer. Shared code does
  not parse finding keys or replace equality with digests. Ordinary boundaries,
  wording, evaluation inputs, and suggested actions belong to observations.
  `sticky` keeps historical advisories visible at later prefixes until dismissed
  or filtered, with no automatic expiry. It does not assert current applicability
  or resolution; nonsticky observations remain available in history mode.
  Dismissal reasons and optional notes never establish resolution or affect acceptance.

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
- Prefix agent-authored Rust comments with `//A`; do not add block comments,
  Rust doc comments, or doctests unless requested. Leave existing comment styles
  alone unless their content needs changing.
- Explain non-obvious invariants, boundary choices, and ordering constraints;
  avoid narrating self-explanatory code.
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
- Use `//A TODO:` for concrete deferred work and `//A HACK:` for a necessary
  workaround. Explain the impact and next step in the handoff when material.
