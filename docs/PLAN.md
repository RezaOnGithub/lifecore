# Rust Sketch 02 Cleanup Plan

Status: Stages 0–4 and the review checkpoint before stage 5 are complete.
Stages 5–6 remain planned.

## 1. Outcome

Reuse the authoritative Rust implementation in this checkout, reduce the amount
of machinery needed to understand it, and make plaintext authoring and bounded
inspection the main user workflows.

The round succeeds when a user can author records in TOML, inspect the exact
ordered changes and their intermediate views, check them, apply them, and
investigate the resulting history. The old CLI and attached-sidecar interaction
model should be gone. Markdown document collections are backlogged; they are
not a prerequisite for completing this round.

Read [AGENTS.md](../AGENTS.md), [SPEC.md](SPEC.md), and [FILES.md](FILES.md) together
with this plan. SPEC and FILES are proposed contracts, not descriptions of what
the old code already implements. This plan identifies where they need decisions
or revision; it does not silently settle those questions.

Backwards compatibility is explicitly out of scope. Do not build legacy
readers, migration layers, command aliases, or API shims to preserve earlier
sketches. This freedom does not authorize deleting unrelated user data or
weakening historical correctness under the chosen model.

## 2. Direction and boundaries

### Preserve

- Immutable entry history, explicit revision forests, exact edge endpoints, and
  bounded type/freshness interpretation.
- Validity at every entry prefix. The retired CLI persisted composite additions
  atomically; stage 5's new CLI must preserve that guarantee. The libraries retain
  atomic in-memory commit. Atomic publication never permits invalid prefixes.
- Full-prefix analyzer execution as a robustness and historical compatibility
  check, with explicit implementation/configuration/evaluation context.
- Analyzer-specific typed findings, normalization, and third-party analyzer
  implementations. Avoid replacing domain variants with unstructured strings.
- Shared validation/staging paths and reliable failure behavior.
- Existing semantic tests that exercise retained behavior, independent of their
  current CLI presentation.

### Change

- Remove the current CLI, its command language, and attached-sidecar state.
- Remove `Context` and `EntryContext`, including context-bearing node/edge
  wrappers. Replace them with stable entry identities and a bounded `View` over
  a particular history snapshot. Do not retain the old API behind a facade.
- Reduce field types to a small scalar/aggregate model; settle the precise
  aggregate boundary before changing representations.
- Remove or substantially reduce textual query syntax. Preserve useful query
  operations through a bounded library interface.
- Simplify ownership, borrowing, iteration, and conversion code where this makes
  the invariants easier to review.
- Build authoring around explicit TOML plans in a new `lifecore-cli`, which owns
  storage envelopes and persistence. Keep libraries focused on graph operations,
  analysis, in-memory transactions, and serialization.
- Simplify MessageUniverse responsibilities rather than relocating the whole
  subsystem into the core.

### Do not expand this round into

- A Python rewrite, a generic plugin platform, or a new general query language.
- Multiple storage backends or speculative incremental-analysis infrastructure.
- Automatic graph repair, automatic split winners, or implicit edge retargeting.
- A general persistent issue tracker for analyzer findings.
- A large web application before the inspection and authoring APIs work.
- Markdown document collections, link extraction, relationship authority,
  collection manifests, or document checkout/writeback.

## 3. Decisions to record before dependent changes

Resolve these with small examples and record the decision in SPEC or FILES.
Independent cleanup can proceed while an unrelated decision remains open.

| Decision | Current situation | Proposed direction / required evidence |
| --- | --- | --- |
| Scalar and aggregate shapes | Decided in Stage 0; implemented in Stage 2. | Text, signed integers, booleans, timestamps, nested lists and string-keyed records. No floats, sets, tuples, or unions. |
| Schema representation | Implemented in Stage 2. | Named field records carry `shape` and `presence`; shapes use `kind`, `element` for lists, and `fields` for records. No textual type-expression parser. |
| Type enforcement and metatype | Configured TypeCheck with a fixed built-in metatype. | Mandatory installation remains an authoring-policy decision before Stage 5. No bootstrap graph entry. |
| Plan persistence | Atomic staging retained; library persistence removed at the review checkpoint. | Libraries compare exact original graph/suite state before in-memory commit. Stage 5's `lifecore-cli` owns publication, writer exclusion, stale-base rejection, no-clobber initialization, and receipts. Concrete layout remains open. |
| Finding identity | Revised at the review checkpoint; SPEC section 8. | Database, registered analyzer name, exact `semver::Version`, original configuration string, category, and complete opaque `analyzer_certificate: AnalyzerCertificate` (a String newtype). Built-ins encode typed variants, exact subjects, and details as deterministic JSON. Ordinary boundaries, wording, inputs, and actions belong to observations. |
| Advisory presentation | Stage 4, simplified at review; SPEC section 8. | Current observations plus explicit `retain_in_current_report` advisories; full history selectable. Serializable dismissal has a reason and optional note; persistence belongs to stage 5. Retention and dismissal never establish applicability or resolution. |
| Evaluation context | Explicit at review. | `EvaluationInputs { evaluation_time: Option<jiff::Timestamp> }`, default `None`, frozen through initialization and replay. Missing required time is an error; no implicit clock. `EvaluatedView::Initialization` identifies initialization; `EvaluatedView::Prefix(view)` includes prefix zero. |

Stage 0 decisions: text, signed integers, booleans, and timestamps; nested lists
and string-keyed records; no floats, sets, tuples, or unions. Lists have a declared
element shape; records have named fields and presence policies. `Type` uses
records for named field descriptions, with structured kind/element/fields tags;
the fixed metatype decoder checks that vocabulary without a second schema DSL.
TypeCheck remains configured, with its fixed built-in metatype available in an
empty graph. Correctly shaped empty collections count as present. Plans persist
atomically; every prefix remains checked and inspectable. SPEC/FILES reflect
these choices. Stage 4 settled finding identity and advisory defaults; the review
checkpoint below supersedes its shared facts, normalized configuration identity,
and library persistence choices in SPEC section 8.

Baseline at `5dbb719`: `cargo test --workspace` passed 119 unit tests and one
doctest. Warnings include disabled `cfg(revise)`/`cfg(never)` tests, unused
variables/helpers/imports, and the misplaced root manifest lint setting. The two
ignored `*_witness` tests fail when explicitly run, reproducing ordering/hash
contract defects; Stage 2 removes their obsolete types.

| Retained behavior / contract difference | Cleanup action |
| --- | --- |
| Exact edge duplicates and currently identical nodes are rejected; stale node duplicates are permitted | Preserve, with owned structural equality. |
| A stale endpoint cannot receive a new edge; the old core permits self-links | Preserve freshness, reject self-links under SPEC. Retain SCC self-loop coverage with a direct algorithm fixture. |
| Splits can branch from an old predecessor | Preserve exact parent identity and pre-append transition interpretation. |
| Active declarations reinterpret fresh nodes, and transitions use the preceding schema | Preserve; add revoked-permission and future-lookup regression cases. |
| Empty matching containers count as missing; Type requires a bootstrap graph node | Change to presence by key and a fixed built-in metatype. |
| Composite CLI transactions clone/stage/check/apply/save once; save uses a flushed/fsynced temporary and atomic replacement | Extract shared staging and persistence, retain failed-save and stale-base tests. Existing head reread alone does not exclude concurrent writers. |

CLI salvage: transaction composite/rollback fixtures, state no-clobber/load/
replacement safeguards, malformed-reference fixtures, and bounded freshness
scenarios move into library tests. Keep DAG SCC and typechecking scenarios.
CLI locators, aliases, sidecar workflows, message namespace, and command/output
snapshots retire. GraphViz rendering is frontend-specific; Stage 3 can reuse its
escaping/deterministic-output scenarios from git history without keeping the CLI.

No compatibility requirement should decide these questions. Historical analyzer
reproducibility is a correctness concern within the selected design, not a reason
to support every previous sketch format.

### View contract and operations involving multiple views

Use one `View` for one immutable history snapshot and `end_exclusive` boundary:
its entries occupy `[0, end_exclusive)`. `ResolutionComparison` exposes
`base_view` and `target_view`; added positions occupy
`[base_view.end_exclusive, target_view.end_exclusive)`. Comparing prefixes 3 and 5
adds positions 3 and 4. `ResolutionDifference` contains complete `Resolution`
values accessible through `at_base()` and `at_target()`, with `stale` and an
explicit `TypeResolution` outcome on each. Its constructor requires different
resolutions of the same immutable entry; private fields preserve this invariant.
Keep entry identity separate from inspection: `view.node(id)`, `view.resolve(id)`,
freshness, relationship traversal, and type resolution all use that view's boundary.
Stored nodes and edges carry identities and payloads, not inspection contexts.
Looking up an old entry in a newer view does not change its identity, but can
change its freshness and the meaning of its stored type name.

Most operations need only one view, including comparing two nodes, traversing
their revisions, and checking the graph at one prefix. Multiple views are needed
when the question explicitly compares interpretations at different boundaries:

| Operation | Views and purpose |
| --- | --- |
| Check an append or replacement | The preceding view supplies active transition permissions and endpoint eligibility; the resulting view supplies the graph and schema meaning to check after the append. A predecessor's creation view is not a substitute for the view immediately before the new append. |
| Replay analyzers | Run against successive bounded views. An analyzer examining a historical transition may need its before/after views, but cannot inspect beyond the prefix supplied to that invocation. Replay alone does not require holding every view at once. |
| Preview a plan or compare history | Compare the committed base with a proposed step/final view, or two selected historical boundaries. Show changes in interpretation as well as appended entries. |
| Explain lost relationships or a split | Inspect relationships before replacement and their disposition in a later view. This supports explicit findings; it does not authorize generic automatic resolution. |
| Compare analyzer observations | Retain which view each observation describes. Grouping and dismissal use the finding identity policy, rather than reinterpreting every subject in the newest view. |

The implementation must make these distinctions explicit without recreating
pointed contexts under another name:

- A view is tied to its source as well as its boundary. Equal entry numbers or
  equal boundary numbers do not establish shared history. Cross-view comparison
  must establish a common history prefix; reject unrelated sources where an
  operation requires shared identity.
- A committed base and its candidate can share committed entry identities.
  Proposed entries from independent candidates must remain distinguishable even
  when allocated the same numerical positions. Scope provisional handles to
  their candidate; do not build a general branching/merging framework for this.
- Keep lookups and subviews bounded. A future entry is absent; requesting a
  subview beyond the supplied boundary is an error, not an implicit expansion.
  A bounded analyzer API must not expose a route to the underlying future suffix.
- Use explicit before/after parameters for operations that need both. Rust
  borrowing protects access to storage, but a shared lifetime does not prove
  that two views describe the same source or boundary.
- Do not give views a global ordering that implies ancestry. Keep ordering of
  entry identities, boundary comparison within a history, and contextual
  freshness as separate operations. Initially avoid resolved-type caches; any
  later cache must include source/candidate identity and inspection boundary.
- Scope borrowed views to an immutable inspection phase of a candidate. Finish
  that phase before appending again, then obtain new views. Prefix projections
  can supply before/after inspection without cloning the graph for each view.

## 4. Execution sequence

Keep stages reviewable and leave the active workspace buildable after each.
Stages 0–4 below retain their historical decisions and exit evidence. Their
library stores, normalized configuration identity, shared facts, and repair TOML
export are superseded by the review checkpoint before stage 5; completion of the
earlier stages is not revoked and later stages are not renumbered.
Do not keep obsolete layers solely to avoid updating their callers. Record the
tests run and any known failures at each stage; avoid mixing behavior changes
with unrelated formatting churn.

### Stage 0 — Establish the baseline and resolve contract differences

- [x] Run the existing active workspace tests and record actual failures and
  warnings. Historical reports from another checkout are not baseline evidence.
- [x] Map retained core behavior against SPEC: duplicate rules, endpoint
  freshness, branching, contextual type resolution, transition permissions,
  field presence, metatype behavior, and transaction boundaries.
- [x] Turn reported flakiness into minimal reproducible cases. Separate a bug
  from an intentional contract difference before changing expectations.
- [x] Carry forward the two reproduced trait defects as witnesses:
  `EntryContext::cmp` and `partial_cmp` disagree, and equal `OrderedSet` values
  can hash differently when inserted in different orders. Remove the former
  abstraction in Stage 2; choose the latter's semantics with the value model.
  Test the retained contracts rather than polishing types scheduled for removal.
- [x] Inventory useful CLI fixtures, persistence safeguards, and rendering code
  before removing the package.
- [x] Resolve the value/schema and plan-persistence decisions above. Update
  SPEC/FILES where needed and record the remaining advisory questions.

Exit evidence: a concrete baseline and a list of intended semantic changes.
Useful witnesses include a revision, a split, a type change, a historical cycle,
and a failed ordered plan. Do not preserve an obsolete behavior just because a
test currently asserts it.

### Stage 1 — Retire the old CLI and attached sidecars

- [x] Extract only the reusable persistence, staging, and inspection behavior
  identified in Stage 0 into frontend-independent library code. Preserve the
  existing composite operation's atomic persistence and prefix checks during
  extraction; frontend removal must not weaken those guarantees.
- [x] Move relevant regression scenarios into the retained libraries' tests.
  Do not reproduce the old command/output contract in the new API.
- [x] Remove `lifecore-cli` from the workspace and remove its implementation,
  CLI-specific state, attached-sidecar machinery, and obsolete package docs.
- [x] Remove dependencies and lockfile entries that no remaining target needs.
- [x] Update AGENTS and repository entry documentation to describe the active
  workspace and available verification commands.

Exit evidence: core and analyzer tests run without the old CLI. No retained
library depends on an attached draft to answer a committed-data query. A small
new executable is introduced later for explicit file operations.

Stage 1 evidence: `cargo test --workspace` passes 69 unit tests and one doctest,
with the two Stage 0 defect witnesses ignored. The shared transaction tests cover
composite rollback, missing factories, stale memory/file bases, no-clobber
initialization, failed atomic replacement, malformed references, and a historical
cycle repaired only in the final view. Remaining warnings are in the old core
and disabled tests scheduled for Stage 2. File writers now exclude one another
through an advisory lock held over base verification and replacement. Receipts
for retries after uncertain acknowledgment remain Stage 5 work.

### Stage 2 — Replace contexts, then simplify values and core operations

- [x] Make `View` replacement the first structural change after CLI extraction.
  Delete `Context`, `EntryContext`, their ordering/conversion machinery, and
  context-bearing node/edge representations. Retain a simple entry identity and
  an explicit exclusive boundary; implement the view contract above.
- [x] Move lookup, freshness, relationship, and type-resolution operations onto
  `View`. Update validation, analyzer callers, and retained tests in the same
  buildable change; do not leave an old-context compatibility layer.
- [x] Exercise before/after validation and base/candidate comparison before
  spreading the API. Update AGENTS' Core Model instructions when the replacement
  actually lands.
- [x] Implement the selected scalar/aggregate model and its schema encoding.
  Update parsing, equality, hashing, typechecking, and serialization together.
- [x] Decide whether aggregate order participates in equality. Remove
  `OrderedSet` if unnecessary; otherwise make equality, hashing, and encoding
  obey the selected semantics. Equal values must hash equally. Do not use a
  Rust collection hash as a durable finding identity.
- [x] Remove obsolete type constructors, recursive field-conversion helpers,
  and payload kinds instead of carrying unused variants forward.
- [x] Evaluate replacing interned string/value/sequence/set tables with directly
  owned entry payloads. Prefer the direct representation unless a measured need
  justifies an indirection. Preserve entry identity independently of equality.
- [x] Consolidate repeated enum inspection and conversion through borrowing,
  pattern matching, standard collection operations, and small shared helpers.
  Prefer borrowed node/edge accessors and direct variant checks over cloning
  merely to inspect an entry. Return slices or iterators when ownership is not
  needed; remove pass-through iterator wrapper types.
- [x] If any intern tables remain, replace repeated membership tests and
  insert/index bookkeeping with the collection's insertion/entry APIs. Do not
  optimize interning code that the chosen representation deletes.
- [x] Make the selected type/metatype behavior and failed-append guarantees
  explicit in one mutation path.
- [x] Consolidate nested transaction/add/connect/apply cloning around one
  isolated candidate per atomic operation. Perform private mutations and
  required prefix checks there, then publish once. Keep public mutation
  failure-atomic; do not introduce public unchecked appends or a rollback journal
  where discarding the candidate suffices.
- [x] Separate provisional state from incidental session metadata where this
  removes `Mock` bookkeeping. Keep any metadata whose purpose is demonstrated;
  avoid gathering environment information merely to perform a graph operation.
- [x] Colocate substantive implementations with their types and responsibilities
  instead of leaving ordering, hashing, and traversal hidden in `boilerplate.rs`.
  Keep the module split small: entries/values, views, validation, analysis, and
  persistence as justified by actual code. Keep root exports easy to inspect.

Exit evidence: the reduced model represents its own schemas and round-trips
through storage. Invalid references and payloads fail without mutation; earlier
views retain their original meanings. Reviewers can trace an append without
following multiple representation-conversion layers. The old context types are
gone, and explicit cross-view checks prevent accidental source/boundary mixing.

Stage 2 evidence: `cargo test --workspace` passes 47 unit tests with no ignored
cases; `cargo +nightly fmt --check` and strict workspace Clippy pass. The old
context/hash witnesses are replaced by identity-ordering and record-equality
regressions. Prefix/source isolation, before/after schema and permission changes,
empty-field presence, typed findings, malformed imports, atomic rollback, and
competing writers are covered. Dormant `lifecore-test` scenarios now live in the
active core/typechecker tests; placeholder application crates remain untouched.

At the stage 2 exit, the public path was `LifeCoreList::view()` /
`View::prefix(end)` for inspection,
and `transaction::prepare` followed by memory or `Store` commit for mutation.
Entry handles include an origin plus a position. Independent plans receive
separate suffix identities; committed handles survive reload unchanged.
`Reference::Added(n)` identifies an earlier operation, including through mixed
node/edge/analyzer plans. No implicit session or machine metadata is collected.

The context-bound query representation and type-expression parser retired with
their obsolete models. Stage 3 adds richer selection, comparison reports,
and exports. Migrating the runner also removed its placeholder validation,
panicking output conversion, redundant factory lookups, and diagnostic clones;
Stage 4 adds finding/observation identity, dismissal, and presentation.
The reusable SCC algorithm and typed analyzer/factory boundary remain.

At the stage 2 exit, file publication synced the file and directory. A directory-sync
failure after rename was reported as `transaction::Error::Published`, because the
new state might already be visible. The review checkpoint removes this library
API and carries the safeguard into stage 5. Receipt-based retries after uncertain acknowledgment remain
Stage 5 work. TypeCheck remains opt-in as agreed; no TOML executable is claimed.

### Stage 3 — Build inspection and simplify queries over View

- [x] Use the Stage 2 view API throughout inspection. Avoid parallel query
  contexts or a second freshness/type-resolution implementation.
- [x] Remove the old textual query language and field-path syntax that no longer
  serve the reduced value model.
- [x] Provide direct selection by type/currentness, simple field filtering,
  text search, relationship traversal, revision traversal, and view comparison.
- [x] Produce structured inspection output and a small graph export or viewer
  that distinguishes revision relationships from ordinary edges.

Exit evidence: a revision/split can be inspected at boundaries before, between,
and after its appends. Missing future entries remain absent. Results have
deterministic ordering. Draft errors do not disable committed-data inspection.

Stage 3 evidence: `cargo test --workspace` passes 56 unit tests. Nightly
formatting and strict core Clippy pass. Six inspection regressions cover
before/between/after revision and split boundaries, exact relationship direction,
literal field filtering and recursive text search, type reinterpretation,
base/candidate comparisons, rejection of divergent sources, deterministic JSON
and escaped Graphviz output, reloads, and inspection after draft/check failures.
The implementation is `lifecore::inspection` plus methods on `View`; there is
no inspection context, query parser, or frontend dependency. SPEC section 9
records selection defaults, traversal ordering, comparison direction, and export
semantics.

### Stage 4 — Simplify analyzer output and MessageUniverse

- [x] Preserve full-prefix execution, typed inner reports, explicit
  normalization, and dynamically dispatched analyzer implementations.
- [x] Centralize the typed-to-normalized wrapper methods if doing so removes
  duplication from real implementations; do not make the adapter a project in
  its own right.
- [x] Simplify runner registration/lookups with collection entry/get APIs and
  accumulate initialization output directly. Preserve initialization and replay
  ordering unless an explicit contract change is justified. Missing factories
  remain errors; exercise the empty-history boundary deliberately.
- [x] Expose borrowed diagnostic iterators rather than cloning outputs into
  fresh vectors for each category. Keep owning conversions at actual ownership
  boundaries and retain typed analyzer-specific variants and `dyn` support.
- [x] Remove the always-true `Output::validate` placeholder, or replace it with
  concrete invariant checking where required. Replace the assertion-bearing
  `Into<LifeCoreError>` conversion with an explicit fallible/result-producing
  operation. Expected invalid input must not panic through an infallible API.
- [x] Separate a finding from the observations of it at particular boundaries.
  Record the evaluation context needed to interpret an observation.
- [x] Implement the chosen finding key and deterministic encoding. Deduplicate
  identical observations separately from grouping a finding across boundaries.
  Retain semantic equality checks when hashes coincide.
- [x] Keep the acceptance gate on unfiltered inconsistencies and execution
  errors. Dismissal and display filters must never affect it.
- [x] Replace generic staleness-based automatic resolution with explicit
  observation/report semantics. A historical finding is not "fixed" merely
  because an entry is superseded or the newest invocation does not emit it.
- [x] Reduce frontend behavior to grouping, joining dismissal records, report
  selection, and rendering. Keep restore usable for dismissals whose finding is
  not currently emitted; avoid carrying the old abbreviation namespace forward.
- [x] Route an explicitly requested suggested repair into a newly checked TOML
  plan. Document-aware connection redistribution is backlogged with collections.

Historical stage 4 decisions (superseded where noted at review): identity uses
database/analyzer identity and version, normalized configuration, category, code,
exact subjects, and typed facts.
Boundaries, wording, evaluation inputs, and repair payloads belong to observations
unless deliberately included as distinguishing facts. TypeCheck retains its typed
variant/path/issue facts; DAG Check uses exact cyclic components and normalized
configuration. Complete key equality controls grouping and dismissal, including
on hash collisions. The default report shows current state observations and
retained events with unconfirmed current applicability. No generic resolution
state is inferred. SPEC section 8 and FILES record the full contract.

Exit evidence: repeated historical observations can be grouped without losing
their boundaries; dismiss/restore survives reload and cannot hide an
inconsistency. Current and historical reports are distinguishable. Document the
chosen default and any limitations for event-oriented analyzers.

Stage 4 evidence: `cargo test --workspace` passes 70 unit tests. Scoped nightly
formatting and strict Clippy pass for all three active packages. Regression
coverage includes dynamic and typed analyzers, initialization plus empty-prefix
execution order, fixed evaluation inputs and contextual execution failures,
invalid future/foreign diagnostics, expected TypeCheck and DAG output,
normalized configuration and finding keys, repeated observations, forced hash
collisions, historical/current selection, dismissal file reload/restore and
concurrent edits, unfiltered historical inconsistencies, and retained split
advisories after reconnecting only one branch.

The old message category wrappers and duplicate typed adapters are replaced by
`MessageKind`, a shared `TypedAnalyzer` adapter, and explicit `Report` /
`Observation` records. There is no revived MessageUniverse lifecycle or namespace.
At the stage 4 exit, `analysis::presentation` supplied grouping, filtering,
rendering, and atomic standalone dismissal storage. `analysis::repair::check`
exported freshly checked ordered actions as TOML, preserving exact references,
explicit revision intent, nested payloads, and nanosecond timestamps. It rejected
stale revise intent, invalid aliases, foreign sources, out-of-profile timestamps,
and historical analyzer failures without publication. These exports covered the
output side of the timestamp contract at that exit. The review checkpoint below
removes the stores and TOML exporter; both input and output adapters, the executable,
and durable receipts now belong to stage 5. Event applicability assessment remains
an analyzer responsibility, not an automatic issue-resolution subsystem.

### Review checkpoint — Simplify contracts between stages 4 and 5

- [x] Keep typed findings in each analyzer and replace shared `Fact`/`Finding`
  machinery with a complete opaque `analyzer_certificate: AnalyzerCertificate`,
  a String newtype. Built-ins use deterministic JSON for typed variants, exact
  subjects, and distinguishing details.
  Compare full keys for grouping/dismissal; remove versioned key encoding and
  digest helpers.
- [x] Construct analyzer identity in the runner from registered name, exact
  `semver::Version`, and original configuration string passed unchanged to the
  factory. Equivalent spellings can behave identically but have distinct identities.
  Built-ins parse their own package versions; version equality is exact.
- [x] Use explicit optional evaluation time, frozen for initialization and every
  prefix. Missing required time errors. Replace invocation/scope wrappers with
  `evaluated_view: EvaluatedView` and `retain_in_current_report: bool`; flatten
  observations and preserve typed/dynamic execution, ordering, and validation.
  Distinguish `Initialization` from `Prefix(view)`, including prefix zero.
- [x] Add dismissal reasons and optional notes as serializable display state.
  No dismissal or historical retention establishes resolution or affects acceptance.
- [x] Name exclusive view boundaries `end_exclusive`; use `Resolution` with an
  explicit `TypeResolution` outcome from `View::resolve` and retain both resolutions
  in `ResolutionDifference`. Its constructor requires different resolutions of the
  same entry, preserved by private fields and read-only accessors.
  `ResolutionComparison` exposes comparison views and precise appended ranges;
  preserve shared-history checks.
- [x] Make `ancestors` start at the parent. Preserve self-inclusive behavior for
  roots and current successors through a private lineage walk; descendants and
  relative-degree callers must not skip an additional generation.
- [x] Use `<module>.rs` for single-file modules and `<module>/mod.rs` only when a
  module spans multiple files, keeping `lib.rs` roots and `*::test::test2` paths.
  Extract inline test scaffolding to `test/mod.rs`; keep leaf tests in `test2.rs`.
- [x] Remove library stores and publication machinery. Retain structural validation
  on graph deserialization and atomic in-memory preparation/commit. Compare exact
  database identity, original entries/count, and original ordered suite; reuse the
  candidate's original prefix and snapshot only the original suite separately.
- [x] Replace repair export with `analysis::suggestion::prepare_suggestion`
  returning a checked transaction. Preserve source checks, exact references,
  intent, all-prefix checks, and explicit commit. Remove plan IDs, TOML envelopes,
  and timestamp export restrictions.
- [x] Remove `tempfile`, `toml`, and `blake3`; update SPEC, FILES, and contributor
  guidance together. Replace filesystem-dependent library tests with in-memory
  serialization tests and carry publication acceptance into stage 5.

The libraries now provide graph operations, bounded inspection, analysis,
in-memory transactions, and Serde support. No file envelopes or persistence API
remain. Earlier stage 1–4 publication and export tests are historical evidence;
stage 5 must restore those safeguards in the CLI alongside durable receipts.
No CLI implementation, compatibility adapters, or migrations belong to this checkpoint.

The initial review commit used `InspectedEntry`, `EntryChange`, and `Comparison`,
an optional evaluated view, a plain string analyzer key, and `analysis::repair`.
It also moved every file-backed module to `mod.rs`. Follow-up review supersedes
those choices with the names, enums, newtype, checked difference constructor,
and conditional module layout recorded above.

Initial review evidence: `cargo test --workspace` passed 73 unit tests (47 core,
18 TypeCheck, 8 DAG Check), with no failures or ignored tests. Follow-up review
passes 76 unit tests (50 core, 18 TypeCheck, 8 DAG Check), including constructor
invariants, explicit type-resolution outcomes, and exclusive ancestry with
preserved roots and current successors across boundaries. These checks pass:

```sh
cargo check --workspace
cargo +nightly fmt --check -p lifecore -p lifecore-typecheck -p lifecore-dag-check
cargo clippy -p lifecore -p lifecore-typecheck -p lifecore-dag-check --all-targets --no-deps -- -D warnings
```

Coverage includes exact configuration strings and package versions, complete
opaque keys under forced hash collisions, explicit time and deterministic replay,
initialization versus prefix zero, contextual failures, dismissal reasons/notes
and round trips, unfiltered inconsistencies, exclusive comparisons, revision/type
changes, foreign/divergent histories, exact stale bases including suite-only
changes, rollback, malformed deserialization, and explicitly checked suggestions.
Publication-specific cases are carried forward below as stage 5 requirements.

### Stage 5 — Create lifecore-cli for persistence and TOML authoring

- [ ] Create `lifecore-cli` over shared library operations for checking, applying,
  and inspecting TOML and bounded views. No persistent draft attachment.
- [ ] Select concrete storage layout and envelopes here. The CLI owns graph,
  report, and dismissal persistence; libraries retain serialization and validation.
- [ ] Implement the FILES authoring envelope, complete node payloads, ordered
  entries, exact existing references, local aliases, revision/branch intent,
  and TOML import/export, including explicitly selected checked suggestions.
- [ ] Implement the explicit [timestamp adapter](FILES.md#timestamp-conversion-contract):
  preserve nanoseconds, reject excess precision and leap seconds before lossy
  parsing, check the converted UTC range, and export native UTC datetimes.
  Restore export boundary regressions and verify exact instant round trips.
- [ ] Freeze evaluation inputs and expose the concrete ordered plan, affected
  relationships, and source-local diagnostics. Record original configuration
  strings and exact versions; never implicitly read time in library analysis.
- [ ] Validate every proposed prefix and replay historical prefixes under the
  fixed evaluation context. Deserialization alone cannot certify a transaction.
- [ ] Preserve writer exclusion from exact stale-base verification through
  atomic publication; reject changed database identity, entries/count, and suite,
  including suite-only changes. Two writers against one base cannot both commit.
- [ ] Initialize without clobbering an existing database. Failures before
  publication preserve prior state; synchronize publication and distinguish
  post-publication synchronization failures from rollback.
- [ ] Persist reports and dismissals with exact keys, all reasons, and optional
  notes. Dismiss/restore survives reload and absent current emission; concurrent
  dismissal edits preserve each writer's records under writer exclusion.
- [ ] Publish durable application receipts atomically with the graph. After
  uncertain acknowledgment, an identical retry returns the receipt without
  appending; a reused identity with different content fails. Receipt lookup
  precedes stale-base rejection for an already committed plan.
- [ ] Expose each proposed boundary through the same view operations used for
  committed history, with provisional status clearly shown.

Exit evidence: handwritten TOML can create, revise, split, and connect nodes;
failed checks leave persistence untouched. No-clobber initialization, competing
writers, stale bases, failed replacement, post-publication synchronization
failure, dismissal reload, and safe retries all have executable coverage.
Interrupted apply has a documented recoverable state. Identical checked inputs
produce identical semantic plans. Collection workflows remain backlogged.

### Stage 6 — Finish the round and remove leftovers

- [ ] Remove unreachable helpers, obsolete schema/query parsers, stale docs,
  and unused dependencies exposed by the preceding stages.
- [ ] Make SPEC, FILES, AGENTS, examples, and actual behavior agree. Mark genuine
  deferrals explicitly instead of claiming the full original FILES proposal.
- [ ] Run the active workspace tests, relevant formatting/lint checks, and the
  end-to-end fixture through the new executable.
- [ ] Record what was removed, remaining known failures, commands run, and the
  smallest next increment. Do not add compatibility work to the remainder list.

## 5. Cross-stage acceptance scenarios

| Scenario | Expected evidence |
| --- | --- |
| One entry is inspected before and after a revision/type change | Identity stays stable; visibility, freshness, and type resolution follow the selected view. |
| An analyzer asks for an entry or subview beyond its supplied prefix | Future entries remain absent and boundary expansion is rejected. |
| Two databases or independent candidates reuse numerical entry positions | Shared identity is not inferred from numbers or Rust lifetimes; cross-view operations reject unsupported source combinations. |
| A base view is compared with its candidate at successive steps | Shared committed entries compare correctly; proposed identities remain scoped and intermediate differences are visible. |
| Aggregate insertion order changes | Equality, hashing, and serialization obey the selected ordered/unordered semantics; equal values hash equally. |
| A multi-entry operation fails after valid earlier candidate appends | The published receiver and persisted graph remain unchanged under the atomic contract. |
| A type declaration changes | Earlier views use earlier schemas; the resulting current graph is checked under the new declaration. |
| A transition permission is revoked | Earlier legal replacements remain historical facts; a later branch uses the permission active before its append. |
| A plan contains an invalid intermediate entry and a valid final replacement | Reject the invalid prefix under either persistence policy. |
| An analyzer version starts rejecting an old prefix | Full replay identifies the historical compatibility failure and its context; it does not report only the current graph. |
| An analyzer incorrectly stops emitting a known finding | Expected-output fixtures catch what acceptance-only replay cannot. |
| A warning repeats after an unrelated append | Group the same finding while retaining distinct observations. |
| A grouped finding is dismissed and replayed | The dismissal survives; historical execution still occurs. |
| A dismissed warning changes subject or distinguishing facts | Apply the specified identity policy; do not suppress a genuinely different finding. |
| A missing-field warning disappears after revision | Preserve the historical observations; current reporting does not infer generic resolution from staleness. |
| A transition suggestion is followed by unrelated entries | Demonstrate the selected current/historical reporting contract; do not accidentally lose an actionable suggestion. |
| A split reconnects only one branch | Explicitly distinguish intended distribution from a universally "resolved" warning. |
| A TOML plan revises B, then revises A without reconnecting their old edge | Execute the listed order and show the historical edge at the intermediate boundary. |
| A TOML plan explicitly reconnects unchanged A to a successor of B | Show the replacement edge and the intermediate view where the old edge is historical. |
| An edge references a superseded revision or an ambiguous target | Reject the invalid reference; never implicitly select a successor. |
| A stale base, concurrent writer, or commit interruption occurs | Stage 5 rejects or recovers under the atomic contract; never duplicate entries on retry. |
| A suite changes without any entry append | In-memory commit rejects the exact stale base; stage 5 preserves this in storage. |
| Initialization would overwrite a database, or publication fails before/after replacement | Stage 5 verifies no-clobber and pre-publication rollback, and distinguishes post-publication synchronization failure. |
| Dismissals are reloaded or concurrently edited | Stage 5 retains exact keys, reasons, and notes and preserves both writers' edits. |
| A draft is malformed or a configured analyzer is unavailable | Checking/application fails appropriately; committed-data inspection remains available. |

## 6. Verification discipline

Use the actual active workspace when selecting commands. Initially the baseline
includes `cargo test --workspace`, `cargo test -p lifecore-typecheck`, and
`cargo test -p lifecore-dag-check`; inspect existing warnings rather than assuming
a clean strict workspace lint. Update package-specific commands when the old
CLI is removed and the new authoring package is introduced.

Use targeted `cargo +nightly fmt --check -p <changed-package>` and appropriate
package lint checks. Add agent-authored Rust tests only under `*::test::test2`.
Use `<module>.rs` for single-file modules, including `test/test2.rs`; use
`<module>/mod.rs` only when a module spans multiple files, including `test/mod.rs`
for test scaffolding. Keep crate-root `lib.rs`. Follow AGENTS for comments.
Include meaningful failure and historical-window cases rather than tests that
merely duplicate implementation steps.

New stored formats can be incompatible with old sketches. Within a selected
format, reloads, deterministic interpretation, safe writes, and exact historical
inspection remain required. Keep structural/type validation failures distinct
from analyzer diagnostics in tests and presentation.

This round is complete when the active stages have evidence, not when their code
has only been written. Backlogged collections are not an exit requirement. Completed-stage evidence is recorded above; TOML authoring is not implemented
until Stage 5.

## 7. Backlog — Markdown document collections

Preserve the collection proposal in FILES for a later round. Do not implement
collection-specific infrastructure as speculative preparation for it. Explicit
TOML authoring, plan receipts, bounded graph views, and historical analyzer
reporting remain in the current round for their own purposes.

Before resuming collections, revisit document identities, relationship
authority, manifests, alias handling, and writeback normalization. Select the
minimum needed for exact planning, conflict detection, and historical link
interpretation; unresolved ambiguity may require an explicit TOML edit.

Deferred work:

- Check out document revisions and distinguish edits, moves, copies, missing
  files, and deliberate splits.
- Parse Markdown with TOML frontmatter and derive ordinary, typed, and incoming
  edges from links, including links to mechanical nodes.
- Compare all edited documents together and generate ordered TOML plans with
  explicit reconnections and source-local diagnostics.
- Reconcile externally authored relationships and conflicting declarations.
- Retain view-specific path/alias resolution for historical rendering.
- Recover document writeback after database persistence without overwriting
  concurrent user edits or repeating graph appends.

Deferred acceptance scenarios:

| Scenario | Expected evidence when collections resume |
| --- | --- |
| B changes while A removes its link to B | Generate the selected revision order without an unnecessary old-A/new-B edge. |
| B changes while unchanged A retains a file link | Generate an explicit replacement edge and show the intermediate historical edge. |
| A document declares an incoming edge from a mechanical node | Derive the correct direction and validate both endpoint permissions. |
| A document-owned incoming edge is removed | Show the endpoint revision and all retained relationships affected by it. |
| A mechanical change conflicts with document declarations | Expose the conflict before overwriting or dropping relationships. |
| A target splits or an exact URI points at a superseded revision | Require explicit target selection or navigation-only intent. |
| Database persistence succeeds but document writeback fails | Recover writeback from durable evidence while preserving newer text. |
| An old document refers to a moved path or plan-local alias | Historical rendering uses retained resolution at the selected view. |
