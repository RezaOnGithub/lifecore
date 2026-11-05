# LifeCore Lite Specification

Status: core contract for the simplified LifeCore model, implemented by the
`lifecore` crate, with the type rules in `lifecore-typecheck` and the derives
of typed decoding in `lifecore-derive`.

This document specifies a typed graph with explicit replacement history. Every entry
prefix must be valid and inspectable; an ordered plan commits atomically.
Type checking remains a configured analyzer. Its rules below apply when
TypeCheck is configured. Application initialization enables it by default, with
an explicit `init --no-typecheck` opt-out. [ANALYZERS](ANALYZERS.md) details
analyzer execution, finding identity, and dismissal;
[PREFIX-VALIDITY](PREFIX-VALIDITY.md) records the transaction tradeoffs behind
prefix validity.

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
    ty_name: string
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
    ty_name: string
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

A type declaration is a node whose `ty_name` is the reserved name `Type`. Its
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
Section 6.4 lists the value kinds and shapes.

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

Within a view, a name resolves by the current declarations that claim it. A
declaration claims the text of its payload's `name` field, whether or not the
rest of the payload parses; one without a text `name` claims no name, not even
the empty one, which is a legal type name. `Type` always resolves to the
built-in metatype, whatever declarations claim it. Any other name resolves by
its own claimants alone:

| Claimants | Resolution |
| --- | --- |
| None | Missing |
| One that parses | Declared by that declaration and its schema |
| One that does not parse | Malformed, with its structured schema error |
| Two or more | Ambiguous, listing every claimant in append order, each with its schema or its error |

A malformed declaration therefore affects only the name it claims.
`View::declarations` indexes a view's current declarations by the names they
claim, keeping those that claim none apart. `View::resolve_type`, inspection,
TypeCheck, and the application's type list and inspector all resolve through
it, so they agree on every name.

- Every type name used by a current node must resolve as declared.
- Two current declarations with the same name are invalid, even if their schemas
  are identical or no ordinary node currently uses that name.
- A missing declaration for a current node is invalid.

TypeCheck reports `MalformedDeclaration` for each malformed declaration, named
or not; `ReservedTypeName` for each parsing declaration that claims `Type`; and
`DuplicateType` for every claimant of an ambiguous name after the first,
malformed ones included. Field, edge endpoint, and successor checks run only
against a name that resolves as declared. A node, endpoint, or predecessor whose
type is ambiguous or malformed gets no finding of its own, not even
`MissingType`, because the declarations of its name already have theirs.
`MissingType` reports only a missing name, and `UnknownSuccessorType` only a
missing successor name in a declared type's `successors`.

A declaration revision changes the meaning of its name for the resulting view.
Every current node with that name must satisfy the resulting declaration,
regardless of when the node was created. Current edges must likewise satisfy the
resulting endpoint permissions.

Earlier committed views resolve names using their own declarations. Inspecting
an earlier view must not use today's declaration merely because it has the same
name.

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

### 6.4 Value kinds and shapes

A payload value is one of ten kinds: `text`, `integer` (signed 64-bit),
`boolean`, `timestamp`, `duration`, `date`, `offset_timestamp`, `list`,
`record`, and `variant`. Their encodings and accepted spellings are fixed in
[FILES §2.1](FILES.md#21-syntax-and-values). Values compare and hash field by
field after parsing: `PT120M` and `2h` are one duration, and one instant at
two offsets is two offset timestamps. The duplicate rule
([§7.1](#71-append-checks)) and the analysis recorder rely on this.

Time values fall into two domains, and arithmetic stays inside one:

- **Exact:** instants and elapsed time. A `timestamp` is an instant with no
  local meaning (an evaluation time, a machine-recorded instant, anything
  authored in UTC on purpose). A `duration` (`jiff::SignedDuration`) is
  elapsed time, at most hours: a day is not 24 hours across a DST change, so
  days and larger units are refused.
- **Civil:** days and calendar periods. A `date` (`jiff::civil::Date`) is a
  calendar day. Calendar units such as day, week, and month are words, an
  `enum`, not durations.

An `offset_timestamp` (`lifecore::OffsetTimestamp`) is an instant with the
UTC offset in effect where it was observed, in whole minutes, and belongs to
both: its local date follows from the two without any zone lookup. The rule
is to store what was observed, never a conversion that a time-zone database
would have to redo, so no value carries a zone name or a precomputed future
instant. Crossing between the domains happens only through an offset carried
by a stored value or an evaluation input
([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)); nothing in the
core or an analyzer reads the system zone.

A `variant` is a tag beside a body of fields, so a tagged value carries its
tag even where no declaration is at hand.

A field's shape names a kind, with these refinements:

- `list` declares its element shape and `record` its field declarations.
- `enum` is a shape over `text`, not a kind: it declares a non-empty set of
  non-empty member strings, and a text value outside it is a type error
  (`NotAMember`).
- `variant` declares at least one tag, each with its own field declarations.
  A value whose tag is not declared is a type error (`UnknownTag`), and its
  body is not judged further; otherwise its body is checked against the tag's
  fields under the usual presence policies, and each issue is reported under
  the tag, as in `decision(committed).cost`.

A value of another kind than its shape is a type mismatch. Because every
current node must satisfy the resulting declaration
([§6.2](#62-name-resolution)), removing an enum member or a variant tag that a
current node uses is a type error, exactly like removing a field it uses.

### 6.5 Typed decoding

`lifecore::decode` reads payloads into Rust types and derives canonical
declarations from them, so an analyzer neither hand-writes payload readers
nor keeps its declarations in step with its code by hand. Three traits carry
it, each with a derive of the same name from `lifecore-derive`, which
`lifecore::decode` re-exports:

- `FromPayload`: `shape() -> ValueType`, the shape a field of the type is
  declared with, and `from_payload(&Payload)`.
- `FromBody`: `fields()`, the field declarations it reads, and
  `from_body(&Body)`. `Node::decode::<T: FromBody>()` decodes a node's
  payload.
- `Declare`: `declaration() -> Type`, a canonical declaration.

| Rust | Payload | Declared as |
| --- | --- | --- |
| `String` | text | `text` |
| `i64` | integer | `integer` |
| `bool` | boolean | `boolean` |
| `Timestamp` | timestamp | `timestamp` |
| `OffsetTimestamp` | offset_timestamp | `offset_timestamp` |
| `SignedDuration` | duration | `duration` |
| `civil::Date` | date | `date` |
| `Vec<T>` | list | `list` and the element's shape |
| struct, `#[derive(FromBody)]` | record, or a node's body | `record` |
| unit-only enum, `#[derive(FromPayload)]` | text | `enum` |
| other enum, `#[derive(FromPayload)]` | variant | `variant` |
| `Option<T>`, as a struct field only | absent or `T` | `optional`, or `encouraged` or `deprecated` by attribute |

In an enum with fields, each variant is a tag and its named fields are the
tag's fields; a unit variant there is a tag with no fields. A field read as
`T` is declared `mandatory`. A field read as `Option<T>` is `None` exactly
when absent, since there is no null; a present value must decode.

Decoding follows four rules:

- **Strict about kinds.** A value decodes only from its own kind: a `String`
  field never accepts a timestamp, and a `Timestamp` field never accepts text
  that looks like one.
- **Lenient about fields.** Decoding ignores every field the Rust type does
  not name, in a body, a record, or a variant's tag. Rejecting undeclared
  fields is TypeCheck's job ([§6](#6-type-declarations)), not decoding's.
- **Names** are a field's identifier and a variant's in snake case
  (`OffsetTimestamp` is `offset_timestamp`), or what
  `#[lifecore(rename = "…")]` gives.
- **Bodies only.** Decoding reads a body and nothing else: never a node's
  `ty_name`, its edges, or a declaration. The caller chooses which nodes to
  decode.

A failure is one `DecodeError { path, problem }`. `path` leads from the
decoded value to the failing part with the parts of a field path
([§6.4](#64-value-kinds-and-shapes)), so a variant's field is reported under
its tag, as in `decision(committed).cost`. `problem` is one of `Missing` (a
field read as `T` is absent), `WrongKind { expected, found }` (an enum
expects text), `NotAMember { value }` (text naming no variant of a unit-only
enum), and `UnknownTag { tag }` (a tag naming no variant; its body is not
read). `Node::decode` returns a `PayloadError`: the decode error with the
node's `EntryId` and `ty_name`, which only serves the message.
[ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization) says how an
analyzer reports it.

A decode type names only the fields its reader reads. A user may change or
remove any field of a declaration that the reader does not read, so decoding
such a field could fail on a payload TypeCheck accepts. A canonical
declaration therefore comes from a separate type that lists every canonical
field, with `#[derive(Declare)]` and these type attributes:

```rust
#[derive(Declare)]
#[lifecore(type = "Stance", outgoing = ["About", "Until"])]
struct StanceDeclaration { decision: Decision, at: OffsetTimestamp, reason: Option<String> }

#[derive(FromBody)]
struct Stance { decision: Decision }
```

`type` is required; `incoming`, `outgoing`, and `successors` are string
lists, each the empty set when omitted. They feed `Declare` and nothing else:
edges carry no payload, so decoding has nothing to check on them. Field
shapes come from `FromPayload::shape`, so a `Declare` type needs no
`FromBody`, and since it is never constructed it needs `allow(dead_code)`.
Its names are defaults, not identity: decoding never reads them.

A decode type fits a declaration when every field of its `fields()` is
declared with the same shape, and every field it reads as `T` is declared
`mandatory`; a field read as `Option<T>` may have any policy. Those are also
the field requirements a reader's declarations must keep for its decoding to
succeed wherever TypeCheck accepts the payload.

The derives reject misuse at compile time: generics, unions, tuple fields,
an empty enum, a duplicate name, an empty member or tag, a presence
attribute on a field that is not `Option<T>`, and `Declare` without `type`
or claiming `Type`. A recursive type has no finite shape. The generated code names `lifecore`
through the absolute path `::lifecore`; a crate depending on it under another
name says `#[lifecore(crate = "path")]`.

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

The necessary declaration must resolve as declared at this point. A missing,
ambiguous, or malformed declaration prevents the append: a missing one through
the transition's `MissingType`, the others through their declarations' own
findings ([section 6.2](#62-name-resolution)). In particular, a superseded
predecessor's type name may no longer have an active declaration; branching from
that predecessor still requires the declaration to be available.

This is a transition check, not a permanent constraint re-evaluated against
later schemas. Revoking a permission does not retroactively invalidate an
earlier replacement.

TypeCheck judges each transition once, at the prefix whose newest entry is the
successor, against the view immediately before it; later prefixes do not repeat
the judgment. Full replay ([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization))
evaluates every prefix, so every transition in history is still judged and its
inconsistency still rejects any plan whose replay includes it. The finding key
omits the evaluated boundary, and reports select inconsistencies at any boundary
([ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal)).

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
analyzer suite exactly match the prepared base, and returns the analysis that
certified the candidate. The candidate already retains
the original entry prefix; only the original suite needs a separate snapshot.
There is no serialized-state fingerprint or digest-based acceptance guard.
Every failed core mutation leaves its receiver unchanged.

Graph deserialization validates structural integrity before exposing traversal.
A structurally valid import remains inspectable without installed analyzers;
preparing any transaction still requires the configured analyzers and full replay.
Deserializing a report does not certify a new transaction.

Filesystem envelopes, locking, and publication belong to the application service
shared by HTTP and CLI ([STORAGE](STORAGE.md)); the graph/analyzer libraries
remain independent of storage and HTTP. Proposals, plan IDs, and the snapshot
are application contracts ([FILES §2](FILES.md#2-json-and-toml-change-files),
[STORAGE §3](STORAGE.md#3-graph-revision)). They do not replace the core's
exact commit comparisons: a plan ID matches a stored revision, while commit
compares the entries and the suite themselves.

## 8. Analyzers

An analyzer receives a bounded view and explicit configuration and returns
diagnostics. It may inspect that view's history as well as its current graph,
but may not inspect entries beyond the supplied boundary.

Analyzers express application rules such as dependency acyclicity, missing
justification, overdue work, or suggested restructuring. TypeCheck implements
the type rules above as a configured analyzer. Analyzers do not mutate history
or implicitly apply repairs.

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

[ANALYZERS](ANALYZERS.md) specifies execution and normalization, finding
identity, presentation and dismissal, and explicit suggestions.

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
lists, records, and variant bodies). Predicates are ANDed. Keys, variant tags,
and type names are not searched;
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
`type_resolution`: a node's `TypeResolution` in the view
([section 6.2](#62-name-resolution)), or none for an edge, which carries a
relationship label rather than a declared type.
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

Inspection requires neither a valid draft nor installed analyzers. Graphviz
drawings of a view are an application method, `views.render`
([RPC §2](RPC.md#2-methods)).

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

Core graph and analyzer APIs remain synchronous; the application built on them
is specified from [INTERFACES](INTERFACES.md). The payload and control-integer
encoding shared by files, RPC, and the snapshot is specified in
[FILES §2.1](FILES.md#21-syntax-and-values).

A custom storage engine, interning, general query language, schema hashing, or
generic storage-backend interface is not required. Physical storage may differ
from the logical entry sequence while preserving history and ordering.
Supporting code needed to interpret historical schemas must remain available or
versioned sufficiently to avoid silently changing their meaning.

The defining contract is: **immutable revisions, tree-shaped replacement,
explicit relationships, contextual type names, and validity at every prefix.**
