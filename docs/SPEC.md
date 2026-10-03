# LifeCore Lite Specification

Status: proposed contract for the simplified LifeCore model.

This document specifies a typed graph with explicit replacement history. Every entry
prefix must be valid and inspectable; an ordered plan commits atomically.
Type checking remains a configured analyzer. Its rules below apply when
TypeCheck is configured. Application initialization enables it by default, with
an explicit `init --no-typecheck` opt-out. Application and authoring contracts
are in [INTERFACES.md](INTERFACES.md) and [FILES.md](FILES.md). Appendix A records
the transaction tradeoffs behind prefix validity.

## 1. Purpose

LifeCore supports personal databases in which notes, tasks, projects, concepts,
and other records can share typed relationships and retain their history.

The model has two structures:

- A **revision forest** records replacement and splitting. Each node revision
  has at most one predecessor and may have multiple successors.
- A **relationship graph** connects particular node revisions with typed,
  directed edges.

Replacement determines which revisions are current. A historical view determines
which replacements and type declarations apply. Type checking is configured
explicitly.
Optional analyzers express rules beyond typing.

## 2. History, entries, and transactions

History is an ordered sequence of immutable entries. Each entry is either a node
or an edge.

Every entry has a unique ID and an unambiguous position in append order.
An exact ID includes its origin UUID and position; position alone cannot identify
independently prepared suffixes. Content equality does not imply entry identity.

A transaction appends an ordered plan atomically: all entries commit after the
checks pass, or committed history remains unchanged. Existing entries cannot be
edited or deleted through the mutation interface.

The empty history and every entry prefix are inspection boundaries, including
prefixes inside a committed plan. A view fixes a source and `end_exclusive`,
containing positions `[0, end_exclusive)`. Prefix zero is an empty graph view;
`View::prefix(end_exclusive)` only narrows the visible boundary.
Every proposed prefix passes structural validation. The configured analyzers,
including TypeCheck when installed, replay every historical prefix under fixed
evaluation inputs. Checking only the final graph is insufficient.

The database history is ordered. The branching described in this specification
belongs to individual node revisions; repository branches are not required.

## 3. Nodes and the revision forest

A node has the following logical fields:

```text
Node:
    id
    type_name: string
    payload: map<string, value>
    predecessor: node ID or absent
```

A node is an immutable revision, not a mutable entity containing its own version
counter. Every successor receives a new entry ID. No additional stable entity ID
is required by the model.

A predecessor must identify an older node. A node without a predecessor starts a
new tree. A node with a predecessor is a direct successor of that revision.

There is no limit on the number of successors. A successor may be appended even
when its predecessor already has successors. Each successor has exactly one
predecessor, so the revision structure is a forest. There are no node merges.

One successor expresses a revision. Multiple successors express a split. These
use the same append operation; a split does not require a separate event or
lineage record. Successors are created by separate appends, either consecutively
or with other entries between them.

Each node stores its complete payload. A user interface may offer patch syntax
or inherited defaults while authoring, but committed payloads do not depend on
replaying patches against predecessors.

## 4. Edges and the relationship graph

An edge has the following logical fields:

```text
Edge:
    id
    type_name: string
    from: node ID
    to: node ID
```

Edges are directed and lightweight. They have no payload, predecessor, or
independent replacement history. Their endpoints must be distinct older nodes.

An endpoint identifies one exact revision. It never means the root of a revision
tree, its current descendants, or an automatically selected successor.

Edge type names are relationship labels. The core does not require separate
edge-type declaration nodes; permission to use a label is declared by the node
types at its endpoints.

Cycles in the relationship graph are permitted by core validation. A configured
analyzer may impose acyclicity on all or part of that graph.

## 5. Current and historical data

In a view `h`, a node is **current** if it is present and has no direct
successor present in that view:

```text
current(node, h) =
    present(node, h)
    and not exists successor in h with successor.predecessor = node.id
```

A present node that is not current is **superseded**. These terms correspond to
the earlier names "fresh" and "stale".

An edge is current if it is present and both endpoints are current in the same
view. A present edge with either endpoint superseded is historical rather than
current.

The **current graph** is the graph induced by current nodes and current edges.
It is derived from history. Current flags, active-edge lists, and cached type
resolutions are not independent sources of truth. The implementation's staleness
index ([section 9](#9-inspection-and-queries)) is likewise derived from validated
history, and it is never serialized or persisted.

All leaves of a revision tree may be current simultaneously. There is no
automatic winner among siblings and no unique "latest version" of a split tree.
Finding current descendants returns a collection.

Superseding a node does not carry its connections forward. To preserve a
relationship, append a new edge involving the intended successor.

For example, suppose `A --Supports--> P` exists and `B` and `C` succeed `A`. The
original edge becomes historical when `A` is superseded. Appending
`B --Supports--> P` retains that relationship for `B`; also appending
`C --Supports--> P` retains it for both successors. Appending neither leaves
neither successor connected to `P` by that relationship. The original edge is
retained in history in every case: "assigning" a connection means creating new
edges, not changing the original edge's endpoints.

The same rule applies to incoming connections. For an original
`P --Supports--> A`, the new edges would be `P --Supports--> B` and/or
`P --Supports--> C`. Each new edge must independently satisfy endpoint freshness
and type permissions; the historical edge grants no exemption.

Historical inspection retains the original nodes and edges. Queries for
historical edges must distinguish them from current edges explicitly.

## 6. Type declarations

A type declaration is a node whose `type_name` is the reserved name `Type`. Its
payload describes:

```text
TypeDeclaration:
    name: string
    fields: map<string, field declaration>
    incoming: set<string>
    outgoing: set<string>
    successors: set<string>
```

`name` is the logical name used by ordinary nodes. It is distinct from the
declaration node's own type, which is `Type`.

A successor with the same type name is always permitted by the type-transition
rule. `successors` permits other type names in addition to that implicit
permission. An empty set therefore allows only same-type successors; it
does not forbid successors altogether.

`fields` defines accepted payloads: each declared field's value shape and
presence policy. Undeclared fields are rejected. The encoding of a declaration
payload is fixed in [FILES §2.6](FILES.md#26-type-declaration-payloads); it
provides deterministic validation and equality for supported stored values.

Each declared field has one of four presence policies:

| Policy       | Field absent                         | Field present                                             |
| ------------ | ------------------------------------ | --------------------------------------------------------- |
| `mandatory`  | Type error                           | Validate its value shape                                  |
| `optional`   | Accepted without a presence warning  | Validate its value shape                                  |
| `encouraged` | Accepted with a warning to supply it | Validate its value shape                                  |
| `deprecated` | Accepted without a presence warning  | Validate its value shape and warn to remove or replace it |

"Required" means `mandatory`. These policies govern presence, not whether a
present value is checked. An invalid value is a type error under all four
policies. A field has one policy at a time; changing a mandatory field to
deprecated makes its absence valid.

`encouraged` and `deprecated` support gradual schema evolution through valid
prefixes. Introduce a future mandatory field as encouraged, revise current nodes
to supply it, then make it mandatory. Deprecate a field before revising nodes to
omit it and removing its declaration. Optional fields express accepted absence
without migration pressure. Section 10.3 gives migration examples.

Field absence must remain distinguishable from a present value, including null
if null is supported. Validation must not rewrite committed data, inject newly
introduced defaults, or silently coerce historical values. Authoring helpers may
normalize input before constructing the payload to be stored.

TypeCheck remains configurable in the core. Its inconsistencies block writes
when configured; encouraged and deprecated field warnings are advisory.
Application `init` adds its configuration by default; `--no-typecheck` omits it.

### 6.1 Built-in metatype

`Type` has a fixed built-in metatype defining the declaration shape. It is
available in the empty history, requires no bootstrap entry, and is not
replaceable through graph mutations. A declaration may not claim the reserved
logical name `Type`.

The metatype permits declaration revisions to succeed other declaration
revisions. Ordinary type declarations otherwise follow the same predecessor and
freshness rules as other nodes.

### 6.2 Name resolution

Within a view, resolve a name by finding the current declaration with that
`name`. Resolution of `Type` returns the built-in metatype.

- Every type name used by a current node must resolve to exactly one
  declaration.
- Two current declarations with the same name are invalid, even if their schemas
  are identical or no ordinary node currently uses that name.
- A missing declaration for a current node is invalid.

A declaration revision changes the meaning of its name for the resulting view.
Every current node with that name must satisfy the resulting declaration,
regardless of when the node was created. Current edges must likewise satisfy the
resulting endpoint permissions.

Earlier committed views resolve names using their own declarations. Inspecting
an earlier view must not use today's declaration or today's Python class merely
because it has the same name.

Replacing a declaration with one having a different logical name is possible
only if the resulting view remains valid. Splitting a declaration into multiple
current declarations requires distinct logical names. A name cannot disappear
while any current node still uses it. To migrate those nodes, first provide
their destination declarations and any needed additional successor permissions,
then revise the nodes, and only then retire the old name by revising its
declaration. Every step must independently remain valid.

### 6.3 Declaration fingerprints

A deterministic fingerprint may identify or deduplicate declaration content. It
is optional and does not replace the declaration's entry ID or contextual name
resolution.

If used, a declaration fingerprint must cover the complete declaration,
including relationship and successor permissions. A payload-schema hash alone
identifies only the schema document. Arbitrary executable validation behavior is
not assumed to be captured by that hash.

## 7. Mandatory validation

Validation has two responsibilities: checking an append against the preceding
committed view and checking the resulting candidate view. Both must succeed
before the containing plan commits. Type rules in this section apply when
TypeCheck is configured.

### 7.1 Append checks

Before accepting a candidate entry:

1. Check its storage shape and ID uniqueness.
2. Check that every predecessor or endpoint identifies an older node.
3. For an edge, check that its endpoints differ and are both current immediately
   before the edge is appended.
4. Reject an edge if any earlier edge has the same type name, source, and
   target.
5. Reject a node if a current node immediately before the append has the same
   predecessor, type name, and payload. Entry IDs are excluded from this
   comparison. An identical superseded node does not prevent the append.
6. For a successor, check the permitted type transition as described below.

An edge is current in the view immediately after its append. It may become
historical through a later replacement, but it must pass all checks before that
replacement can be attempted. An edge may not be appended after either endpoint
has already been superseded.

Payload equality is structural under the selected value model and independent of
map key presentation order. Hashing may accelerate equality checks but does not
define entry identity.

### 7.2 Successor type permission

Immediately before appending a successor, resolve its predecessor's stored type
name against the current declarations in the preceding committed view.

The transition is permitted if the successor has the same type name, or its type
name occurs in the predecessor declaration's `successors`. The
predecessor itself need not be current.

The necessary declaration must resolve uniquely at this point. A missing or
ambiguous declaration prevents the append. In particular, a superseded
predecessor's type name may no longer have an active declaration; branching from
that predecessor still requires the declaration to be available.

This is a transition check, not a permanent constraint re-evaluated against
later schemas. Revoking a permission does not retroactively invalidate an
earlier replacement.

TypeCheck judges each transition once, at the prefix whose newest entry is the
successor, against the view immediately before it; later prefixes do not repeat
the judgment. Full replay ([section 8.1](#81-execution-and-normalization))
evaluates every prefix, so every transition in history is still judged and its
inconsistency still rejects any plan whose replay includes it. The finding key
omits the evaluated boundary, and reports select inconsistencies at any boundary
([section 8.3](#83-presentation-and-dismissal)).

Append ordering must make the declaration needed for each transition available
before that transition occurs.

### 7.3 Resulting-view checks

After adding the candidate entry to an isolated copy or equivalent provisional
view:

1. Derive the current nodes and edges.
2. Validate current declaration payloads against the metatype and reject
   duplicate declaration names.
3. Resolve every current node's type name and validate its payload.
4. Check each current edge's label against the source type's outgoing permission
   set and the target type's incoming permission set. Both must permit it.
5. Run the configured analyzers and reject blocking inconsistencies.

With TypeCheck configured, no append with a type error can commit. Presentation
and dismissal cannot downgrade its inconsistencies.

Superseded nodes and historical edges are not required to satisfy the current
declarations. Their validity in earlier committed views is evaluated using those
views. Every node and edge is current immediately after its own append and must
pass the applicable checks in that view. A later successor cannot excuse a type
error in an earlier revision, and making an edge historical later cannot excuse
invalid endpoint permissions at its creation.

### 7.4 Failure and persistence

The libraries own structural validation, analysis, serialization, and atomic
in-memory transactions. `transaction::prepare` stages one candidate, checks every
prefix structurally, and replays the analyzers. Node and edge changes are
`EntryChange` values, the same type a suggestion's actions and a lowered proposal
use; only `Change` additionally carries a suite change. A node change with a
predecessor must state `revise` or `branch`, and a root must omit intent; `revise`
additionally requires the predecessor to be current immediately before that
append, including earlier operations of the same plan. Structural, reference,
intent, and analyzer execution failures are errors. Otherwise `prepare` returns a
`Checked` outcome, since an inconsistent plan is a checked result rather than a
preparation error. `Checked::Accepted` holds a `PreparedTransaction`, the only
committable form, which owns the candidate graph and its complete
`AnalysisOutput`. `Checked::Rejected` holds a `Rejection` with the complete
`AnalysisOutput` and the provisional operation-to-entry mapping but no candidate
graph, so a state that failed its checks can be neither committed nor inspected
as history.
`PreparedTransaction::commit` replaces the receiver only when its
database identity, original entry count and contents, and original ordered
analyzer suite exactly match the prepared base. The candidate already retains
the original entry prefix; only the original suite needs a separate snapshot.
There is no serialized-state fingerprint or digest-based acceptance guard.
Every failed core mutation leaves its receiver unchanged.

Graph deserialization validates structural integrity before exposing traversal.
A structurally valid import remains inspectable without installed analyzers;
preparing any transaction still requires the configured analyzers and full replay.
Deserializing a report does not certify a new transaction.

Filesystem envelopes, locking, and publication belong to the application service
shared by HTTP and CLI ([FILES §8](FILES.md#8-atomic-publication)); the
graph/analyzer libraries remain independent of storage and HTTP. Proposals, plan
IDs, and the snapshot are application contracts
([FILES §2](FILES.md#2-json-and-toml-change-files),
[INTERFACES §4](INTERFACES.md#4-database-selection-and-concurrency)). They do
not replace the core's exact commit comparisons: a plan ID matches a stored
revision, while commit compares the entries and the suite themselves.

## 8. Analyzers

An analyzer receives a bounded view and explicit configuration and returns
diagnostics. It may inspect that view's history as well as its current graph,
but may not inspect entries beyond the supplied boundary.

Analyzers express application rules such as dependency acyclicity, missing
justification, overdue work, or suggested restructuring. TypeCheck implements the type rules above as a configured analyzer. Analyzers
do not mutate history or implicitly apply repairs.

Diagnostics distinguish:

- **Inconsistency:** blocks a transaction when emitted by a configured analyzer.
- **Warning:** advisory; does not block the transaction.
- **Suggestion:** advisory and may describe a proposed change. Applying it is a
  separate transaction subject to the same checks.

An analyzer must give deterministic results for the same view, configuration,
and explicit evaluation inputs. Time-dependent rules must receive an
explicit evaluation time and return an error when it is absent. Neither an analyzer
nor the runner may implicitly read the clock. Missing configured
analyzers or analyzer execution failures are errors, not successful checks.

### 8.1 Execution and normalization

See [Analyzer message flow](MESSAGES.md) for diagrams of field ownership,
normalization, group selection, dismissal and transaction checks in the current
implementation, including when a sticky advisory's certificate reaches a returned
`FindingGroup`.

`TypedAnalyzer` returns analyzer-specific typed findings as `Vec<Self::MSG>` from
initialization and analysis. The shared adapter converts each finding with the
implementation's `convert_message(&self, message)` and returns `Vec<Message>`,
preserving order and duplicates for runner normalization. Conversion can use the
analyzer's state and configuration. Direct third-party `dyn Analyzer`
implementations return `Vec<Message>` and are also supported. There is no shared
`Output` wrapper, `InnerOutput` trait, or associated output container.

Keep domain findings structured and assert their variants and details in analyzer
tests; presentation text cannot substitute for that evidence. Factories and
domain findings remain in their analyzer packages. Shared `Message` output
contains an opaque `analyzer_certificate: AnalyzerCertificate`, exact subjects,
optional diagnostic location, relevance set, text, `MessageKind`, and `sticky: bool`.
`AnalyzerCertificate` is a String newtype, serialized transparently as a string.

Both analyzer traits return `version() -> semver::Version`. The runner constructs
`AnalyzerIdentity` from the registered name, returned version, and original
configuration string supplied to the factory unchanged. Version equality is
exact, including prerelease and build metadata; it is not version-requirement
matching. TypeCheck and DAG Check parse their own `CARGO_PKG_VERSION`. Semantic
or encoding changes intended to invalidate earlier dismissals require a version
change.

`AnalysisRunner::new(EvaluationInputs)` freezes one input value for initialization
and every replayed prefix. Its complete shape is:

```rust
pub struct EvaluationInputs {
    pub evaluation_time: Option<jiff::Timestamp>,
}
```

The default is `None`. No clock is read implicitly; a time-dependent analyzer
must error when the required time is absent. All configured implementations are
constructed first, initialized in suite order, then each replays boundaries
`0..=end_exclusive` in suite order. Empty histories execute both initialization
and prefix zero. Missing factories, invalid configuration, execution failures,
and malformed subject/location output are errors.

An observation or execution failure records `evaluated_view: EvaluatedView`.
`EvaluatedView::Initialization` identifies initialization;
`EvaluatedView::Prefix(view)` identifies the exact evaluated prefix, including
zero. Failures also retain analyzer identity and frozen inputs.
Initialization has no graph access or graph diagnostic location. Every replay
location must identify an actual prefix of the evaluated view, with its subject
visible there. Message subjects and relevance must be visible in the evaluated
view. Initialization therefore cannot name graph entries.

### 8.2 Finding identity and observations

`FindingKey` has four fields: database identity, `AnalyzerIdentity`, message
category, and `analyzer_certificate: AnalyzerCertificate`. The wrapped string is
the complete analyzer-owned finding identity, opaque to shared orchestration.
There is no shared `Fact`, nested `Finding`, versioned key envelope, or digest helper.
Grouping and dismissal
compare complete keys, including the entire string; hashes are only collection
implementation details and collisions do not establish equality.

Configuration identity preserves exact original spelling. TypeCheck may accept
empty text and empty JSON objects as equivalent behavior, but `""`, `" "`, and
`"{}"` identify different configurations. DAG Check may normalize its parsed sets
and empty filter/range records internally; order, duplicate, or whitespace
changes in the original string still produce distinct analyzer identities.

Built-ins deterministically JSON-encode their typed variants and distinguishing
details. TypeCheck includes the variant, typed path/issue or schema details, and
exact diagnostic subject while skipping the ordinary location boundary. DAG
Check includes the cyclic-component variant and sorted exact node/edge identities.
Typed domain data stays inside each analyzer; presentation wording is not a key.

Ordinary evaluation boundaries, evaluation time, wording, relevance ordering,
and suggested actions do not by themselves change finding identity. When a time
threshold or transition distinguishes an issue, the analyzer must deliberately
include that detail in its certificate. Different subjects, distinguishing details,
versions, original configuration strings, categories, or databases stay distinct.
Repeated observations of a dismissed key remain dismissed.

The runner consumes each `Message`, moves its certificate into `FindingKey`, and
stores occurrence and presentation fields directly on `Observation`: frozen
inputs, evaluated view, subjects, location, relevance, wording, kind/actions, and
sticky flag. It retains no second nested diagnostic containing the same key.
Whole-observation equality controls deduplication; grouping by finding key is
separate and retains distinct boundaries and presentation. Duplicate suite entries
still execute in full. The runner returns `AnalysisOutput`, retaining the selected
view, inputs, and ordered implementation identities even when no messages were
emitted. It supports Serde round trips. Outputs are ephemeral;
[INTERFACES §6](INTERFACES.md#6-analysis-lifetime-and-dismissal-persistence)
owns their lifetime and the independent persistence of dismissals.

### 8.3 Presentation and dismissal

The default report selects initialization output, observations emitted at the
selected boundary, and advisories with `sticky = true`. A sticky advisory stays
visible at later prefixes until dismissed or filtered; it has no automatic expiry.
With fixed implementation, configuration, and inputs, replay recovers the earlier
observation even if the analyzer emits nothing at the selected head. Sticky
advisories from earlier prefixes are labeled historical with current applicability
unconfirmed. The flag does not mean that a finding still applies, has been resolved,
or should be automatically repaired.

An analyzer chooses the flag by what an advisory describes. Mark sticky an advisory
about an event that the analyzer judges once, at the prefix that event ends, and
that later prefixes do not re-emit; otherwise it would leave current reports as
soon as history grows. Keep nonsticky an advisory about current state: it is
emitted again at every prefix where it still holds, so it leaves current reports
once resolved or superseded. TypeCheck's warnings (`UnknownSuccessorType`, and
encouraged or deprecated field issues) describe current declarations and current
nodes, so they are nonsticky. Its once-judged transition findings are
inconsistencies, which reports select at any boundary
([section 7.2](#72-successor-type-permission)).

History mode admits all replayed observations, including nonsticky advisories,
subject to category and dismissal filters. Grouping first collects all observations
by complete finding key, then selects occurrences for display. Only groups with a
nonempty selected subset are returned; each exposes all its observations as well
as that subset, in deterministic first-emission order.

No generic resolution status is inferred. Non-emission, subject supersession,
or reconnecting only one split branch cannot establish resolution. Historical
observations remain available. An analyzer requiring a current-applicability
assessment must express that assessment through its own domain semantics.

`analysis::presentation` groups, filters, and renders reports. `Dismissals` is
serializable display state, with exact-key listing, lookup, dismissal, and restore
even when the finding is not currently emitted. Each record has a
`DismissalReason` (`Acknowledged`, `FalsePositive`, or `NoLongerRelevant`) and
`note: Option<String>`. Updating a record replaces its reason and note. No reason
establishes resolution. Dismissal loading, writer exclusion, and atomic
publication belong to the shared application service used by HTTP and CLI.

A proposal may name the suggestion it implements; applying it acknowledges the
finding without asserting resolution ([SUGGESTIONS.md](SUGGESTIONS.md)).

All inconsistencies remain visible across history regardless of advisory filters,
report selection, or imported dismissal records. Dismiss operations reject
inconsistency keys. Transaction acceptance uses the unfiltered report and execution
errors, independently of dismissal reasons, notes, or presentation.

### 8.4 Explicit suggestions

`analysis::suggestion::prepare_suggestion(graph, observation, runner)` returns
`Result<Checked, Error>` for an explicitly selected suggestion.
It checks only that the observation source matches the supplied history and
that the observation is a suggestion; its ordered actions, which share
`Change`'s node/edge shapes, are then checked anew with `transaction::prepare`
under the runner's frozen evaluation inputs, including historical and
intermediate prefixes and the revision-intent rules of section 7.4. Exact
existing references and earlier-operation references retain their meaning.
An accepted candidate and its prefixes can be inspected before explicit commit;
a rejection carries only its analysis and operation mapping.
Preparation neither applies the suggestion nor dismisses a finding or infers
reconnections. Exporting suggestions as proposals and verifying their
acknowledgment are application contracts in [SUGGESTIONS.md](SUGGESTIONS.md).

## 9. Inspection and queries

The implementation must support these logical operations without prescribing
their API syntax:

- Inspect a node or edge by ID within a specified view.
- Enumerate all or current nodes and edges in that view.
- Find a node's predecessor, direct successors, ancestors, and current
  descendants within that view.
- Resolve a type name in that view.
- Inspect incoming and outgoing relationships, explicitly selecting current or
  all historical edges.
- Compare two committed views and report added entries and changes in which
  nodes, edges, and declarations are current.

Every lookup and predicate in a query uses the same view unless an explicit
cross-view operation is requested. Results must have deterministic ordering;
append order is the default for entry collections.

Querying a historical node does not make it current and does not redirect its ID
to a descendant. An unavailable entry is reported as absent from the view, even
if it exists later in the database.

These operations use `View`, with selection and structured results in
`lifecore::inspection`. `NodeSelection` combines an optional exact
type name, currentness, top-level field presence/absence/equality predicates,
and a case-sensitive substring search across text values (including nested
lists and records). Predicates are ANDed. Keys and type names are not searched;
there is no field-path or textual query language. Empty collections are present.
`EdgeSelection` selects an exact relationship type and currentness.
Both selections default to current entries; `Historical` selects only stale
entries and `All` includes both. Relationships retain direction and exact
endpoints. `ancestors` and `descendants` exclude the starting revision.
Ancestors run from the parent toward the root; a root has no ancestors.
`root_of` returns the starting node itself when it is a root, and
`fresh_successors` includes the starting revision when current. Entry collections
use append order. Traversal of unavailable identities yields no results.

`resolve` returns absence for unavailable identities. `inspection` produces a
serializable, ordered snapshot with source/boundary and `Resolution` values.
Each resolution directly holds its borrowed entry, `stale`, and
`type_definition: TypeResolution`. Its variants distinguish `NotApplicableToEdge`,
`MissingDeclaration`, `Resolved(TypeDefinition)`, and `Failed(ResolutionError)`;
there is no interpretation wrapper.
An old node's type is interpreted in the requested view, just as with a direct
`resolve_type` call; this does not assert that TypeCheck checks stale nodes.
`base.compare(target)` requires the target to extend the entire base history,
comparing exact entry identities and contents. `ResolutionComparison` exposes
`base_view` and `target_view`. Added entries occupy
`[base_view.end_exclusive, target_view.end_exclusive)`: comparing prefixes 3 and 5
adds positions 3 and 4. Existing freshness or type changes are
`ResolutionDifference` values holding complete resolutions. The constructor
returns no difference for equal resolutions or different stored entries, including
altered contents under the same identity. Private fields and the read-only
`at_base()` and `at_target()` accessors preserve those invariants.
Comparisons support committed base/candidate views and reject unrelated sources,
reversed boundaries, and divergent histories, even if boundary/head IDs match.
Currentness is answered from a derived staleness index held by the graph: each
node's slot records the position of its first direct successor. History is
append-only, so a slot is set once and one index serves every view. A node is
superseded in a view of length `L` exactly when its recorded position is less than
`L`; an edge is historical when either endpoint is. A recorded position may lie
beyond the view, so every query compares it with the view boundary and reveals
nothing else. The index is never serialized. Deserialization rebuilds it in the
same pass as structural validation, and `transaction::prepare` extends it for each
staged entry only after that entry validates, so an invalid entry never affects an
earlier boundary. Resolved types are not cached.

`View::to_dot` exports all visible history as deterministic Graphviz text.
Revision links are dashed blue; ordinary directed edges are solid. Stale nodes
and edges are gray. Labels contain positions, types, and node payloads with
escaped text; position-based Graphviz names are local to the exported view.
Inspection and export require neither a valid draft nor installed analyzers.

## 10. Examples

### 10.1 Revision

In one view, `A` is current and an edge connects `A` to `Project`.

An append creates `B` with predecessor `A`. In the resulting view, `A` and its
edge are historical, and `B` is current. To retain the relationship, a separate
append creates an edge from `B` to `Project` while both are current. The view
between these appends has no replacement connection and must itself be valid.

### 10.2 Split

Starting from the same `A`, append `B` and then `C`, each with predecessor `A`.
Append their desired connections separately. Each intermediate view must pass
validation and the configured analyzers.

```text
Revision forest:       Current relationships:

    A                  B --Supports--> Project
   / \
  B   C                C --Requires--> Equipment
```

`B` and `C` are both current. Creating `C` remains legal after creating `B`,
because only new edge endpoints must be current; a predecessor need not be.

An old connection can be retained on both branches by adding an edge for each,
or dropped from both by adding neither. Connection distribution is explicit.

### 10.3 Gradual schema migration

The current declaration of `Task` requires `title`. We want `status` to become
mandatory too:

1. Append a successor declaration of `Task` with `status` encouraged and with
   its intended value shape. Current tasks without `status` remain valid and
   produce warnings.
2. Append successor revisions of those tasks, supplying valid `status` values.
   Restore desired connections with separate edge appends. Every intervening
   view remains subject to normal validation and analyzer checks.
3. Once every current task supplies `status`, append a declaration revision
   making it mandatory. Attempting this step earlier fails.

To remove a field, first mark it deprecated. Its continued presence is valid but
produces a warning. Revise current nodes to omit it, then remove its
declaration. Whether an undeclared field is rejected follows the schema's
additional-fields policy; the migration must not depend on silently discarding
data.

For a field rename, the transition declaration can deprecate the old field and
encourage the new field. Revise the nodes, then remove the old field declaration
and make the new field mandatory if desired. During that transition, either
field may be absent; warnings communicate unfinished migration. Any stronger
cross-field constraint must explicitly allow the intended intermediate states.

Earlier views continue to expose their own node revisions, declarations, and
presence policies. Migration never rewrites those historical values.

### 10.4 Historical transition permission

A `Draft` declaration permits successors of type `Task`. Replacing a `Draft`
revision with a `Task` is legal while that permission is active immediately
before the replacement.

A later declaration may revoke the permission. This blocks subsequent such
transitions, including new branches from older `Draft` revisions, but does not
invalidate the already recorded transition.

## 11. Implementation scope

The application uses a Rust HTTP adapter and React client, plus CLI initialization,
one-shot RPC, and change-file conveniences. Shared application operations and
core graph/analyzer APIs remain synchronous. Async execution belongs at the
HTTP boundary. See [INTERFACES.md](INTERFACES.md) for ownership and execution.

The payload and control-integer encoding shared by files, RPC, and the snapshot
is specified in [FILES §2.1](FILES.md#21-syntax-and-values).

A custom storage engine, interning, general query language, schema hashing, or
generic storage-backend interface is not required. Physical storage may differ
from the logical entry sequence while preserving history and ordering.
Supporting code needed to interpret historical schemas must remain available or
versioned sufficiently to avoid silently changing their meaning.

The defining contract is: **immutable revisions, tree-shaped replacement,
explicit relationships, contextual type names, and validity at every prefix.**

## Appendix A. Transaction granularity and prefix validity

This appendix records the design discussion and alternatives. Sections 2 and 7
define the selected contract: atomic plans and validation at every prefix.

### A.1 Validation and atomic persistence are separate choices

| Model                              | Full validation                                    | Persistence             | Status                      |
| ---------------------------------- | -------------------------------------------------- | ----------------------- | --------------------------- |
| Single-entry transactions          | Every entry prefix                                 | One entry at a time     | Not selected                |
| Batches with prefix validation     | Every entry prefix                                 | Entire batch or nothing | Selected                    |
| Batches with final-view validation | Only the final view, plus structural append checks | Entire batch or nothing | Excluded from this contract |

The first two permit the same entry sequences when evaluated with the same
analyzers, configuration, and explicit inputs. They differ in what is saved if a
later append fails. Prefix-valid batches expose every entry boundary
for historical inspection, even if those entries were persisted together.

Final-view validation permits additional sequences with invalid intermediate
states. That is a semantic change, not just a storage optimization.

### A.2 Why final-view validation was rejected

The earlier draft allowed a transaction to append an invalid revision `B` and
then append a valid successor `C`. At the transaction boundary only `C` was
current, so `B` could remain in history without ever passing payload validation.
Similarly, an edge with an impermissible label could evade type checking if a
later replacement made it historical before the final check.

Checking every newly introduced object separately would require selecting a
schema context for each object and deciding which intermediate changes count.
Every-prefix validation answers those questions directly: each entry must be
valid in the view immediately following its append, and predecessor-type
permission is checked in the preceding view.

This gives every stored revision an inspectable view in which it was current and
validated. A later repair cannot legitimize an invalid earlier append.

### A.3 Costs for graph operations and analyzer design

Some apparently reasonable invariants admit no valid single-entry construction
sequence. Suppose an analyzer blocks any current Task without a current outgoing
`Supports` edge:

- Appending the Task first fails because the edge is absent.
- Appending the edge first fails because the Task does not yet exist.
- Revising a connected Task fails because replacement makes its old edges
  historical before new edges can be appended.

Such a rule must allow an explicit incomplete state, remain advisory, or be
reformulated to admit the intended construction sequence. Permission to use an
edge label is different from requiring that an edge already exist; ordinary
payload validation and allowed-label checks do not create this construction
problem.

Splits and connection redistribution likewise proceed through valid partial
states. Analyzer authors must consider those states. Atomic persistence with
prefix validation would not remove this requirement.

### A.4 Schema evolution uses field policies

`encouraged` and `deprecated` are intentional tools for gradual migration, not
merely presentation annotations. They let the type checker direct users toward a
new schema while keeping every prefix valid:

```text
Introduce: undeclared -> encouraged -> mandatory
Retire:    declared   -> deprecated -> undeclared
```

Data revisions occur between the declaration changes. Present values always
remain typechecked. Encouraged warns about missing data; deprecated warns about
data that should be removed or replaced. Optional accepts absence without that
pressure. Section 10.3 specifies examples and the role of additional-field
policies.

This incurs extra declaration and node revisions, and node revisions require
explicit reconnection where relationships should remain current. It provides
visible migration progress without admitting invalid history. It does not make
every possible schema change constructible; other constraints must also permit
the migration path.

### A.5 Failure handling and cost

A plan stages the entire sequence, validates every prefix, and persists none of
it if any check fails. Atomic publication does not repair the construction
problem in A.3.

Validating after every append may cost more than checking one final graph. The
prototype should favor the simpler invariant; caching or incremental checks must
preserve the result of full prefix validation.

Prefix validity concerns structural and contextual type rules. Analyzer
acceptance additionally depends on the configured suite and explicit evaluation
inputs used for a check. Introducing a new analyzer or advancing evaluation time
does not imply that historical commits satisfied that different evaluation.

The decision can be revisited if a concrete essential workflow has no acceptable
valid-prefix formulation. Any proposal to allow invalid intermediate states must
explicitly replace this contract and settle historical inspection, schema
context, and transition-checking semantics.
