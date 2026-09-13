# LifeCore Lite Specification

Status: proposed contract for the simplified LifeCore model.

This document specifies a typed graph with explicit replacement history. It
supersedes the README's description of optional type checking. Every entry
prefix must be valid and inspectable; the default transaction appends exactly
one entry. It does not select a storage engine, authoring language, or user
interface. Appendix A records the transaction tradeoffs behind this decision.

## 1. Purpose

LifeCore supports personal databases in which notes, tasks, projects, concepts,
and other records can share typed relationships and retain their history.

The model has two structures:

- A **revision forest** records replacement and splitting. Each node revision
  has at most one predecessor and may have multiple successors.
- A **relationship graph** connects particular node revisions with typed,
  directed edges.

Replacement determines which revisions are current. A historical view determines
which replacements and type declarations apply. Type checking is mandatory.
Optional analyzers express rules beyond typing.

## 2. History, entries, and transactions

History is an ordered sequence of immutable entries. Each entry is either a node
or an edge.

Every entry has a unique ID and an unambiguous position in append order. An
implementation may use the position itself as the ID. Content equality does not
imply entry identity.

A transaction appends exactly one entry atomically. It either commits that entry
after all required checks pass or leaves committed history unchanged. Existing
entries cannot be edited or deleted through the LifeCore mutation interface.

The empty history and every committed entry prefix are historical boundaries. A
**view** contains all entries from the beginning of history through one such
boundary. Every prefix must satisfy structural and type validation. Each append
must also pass the configured analyzers before it commits.

Implementations may represent boundaries as exclusive entry counts or another
equivalent stable identifier. Queries must resolve a request for the latest view
to one fixed boundary before evaluation.

A command that creates several entries performs a sequence of independently
validated transactions. If a later append fails, earlier successful appends
remain committed. There is no exemption for an invalid prefix that a later entry
would repair. A rejected candidate is not part of history.

Atomic persistence of multiple entries with validation of every prefix is a
possible future convenience, not required by this specification. It must not
hide entry boundaries or weaken the every-prefix-valid invariant. Validation
only at the end of a multi-entry batch is outside this contract.

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
resolutions are not independent sources of truth.

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
    payload_schema
    permitted_incoming_edge_types: set<string>
    permitted_outgoing_edge_types: set<string>
    additional_successor_types: set<string>
```

`name` is the logical name used by ordinary nodes. It is distinct from the
declaration node's own type, which is `Type`.

A successor with the same type name is always permitted by the type-transition
rule. `additional_successor_types` permits other type names in addition to that
implicit permission. An empty set therefore allows only same-type successors; it
does not forbid successors altogether.

The payload schema defines accepted payloads, including each declared field's
value shape and presence policy, and the policy for additional fields. The
concrete schema language and value encoding are implementation choices. They
must provide deterministic validation and equality for supported stored values.

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

Type checking cannot be disabled. Encouraged and deprecated field warnings are
part of type-checker diagnostics and do not require an optional analyzer. They
do not block an append or weaken mandatory schema validation.

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
before the single entry commits.

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
name occurs in the predecessor declaration's `additional_successor_types`. The
predecessor itself need not be current.

The necessary declaration must resolve uniquely at this point. A missing or
ambiguous declaration prevents the append. In particular, a superseded
predecessor's type name may no longer have an active declaration; branching from
that predecessor still requires the declaration to be available.

This is a transition check, not a permanent constraint re-evaluated against
later schemas. Revoking a permission does not retroactively invalidate an
earlier replacement.

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

No append with a type error can commit. A type error is not dismissible and
cannot be downgraded by disabling an analyzer.

Superseded nodes and historical edges are not required to satisfy the current
declarations. Their validity in earlier committed views is evaluated using those
views. Every node and edge is current immediately after its own append and must
pass the applicable checks in that view. A later successor cannot excuse a type
error in an earlier revision, and making an edge historical later cannot excuse
invalid endpoint permissions at its creation.

### 7.4 Failure and persistence

Any failed append, type check, analyzer run, or commit discards the proposed
single-entry transaction without changing committed history. A command that has
already committed earlier entries does not roll those entries back. Staged
objects must not expose mutable aliases capable of changing committed payloads.

Hooks, database constraints, and in-process staging are possible enforcement
mechanisms. None changes the validity contract. An implementation must not
assume imported or externally modified history is valid merely because its
normal write path runs validation. Checking only the final current graph does
not establish the validity of all historical prefixes. Import and integrity
checking must preserve the same prefix and transition rules.

## 8. Analyzers

An analyzer receives a bounded view and explicit configuration and returns
diagnostics. It may inspect that view's history as well as its current graph,
but may not inspect entries beyond the supplied boundary.

Analyzers express application rules such as dependency acyclicity, missing
justification, overdue work, or suggested restructuring. They do not provide the
core type system, mutate history, or implicitly apply repairs.

Diagnostics distinguish:

- **Inconsistency:** blocks a transaction when emitted by a configured analyzer.
- **Warning:** advisory; does not block the transaction.
- **Suggestion:** advisory and may describe a proposed change. Applying it is a
  separate transaction subject to the same checks.

An analyzer must give deterministic results for the same view, configuration,
and explicit evaluation inputs. Time-dependent rules should receive an
evaluation time instead of reading an implicit wall clock. Missing configured
analyzers or analyzer execution failures are errors, not successful checks.

Diagnostic identity, dismissal persistence, and presentation are frontend
concerns outside this specification. Dismissal must not change graph validity.

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

The prototype may use Python classes, runtime introspection, executable
authoring files, SQL, a version-control system, or another existing host.

The specification does not require a custom storage engine, interning, a query
language, analyzer factories, schema hashing, a CLI, or a generic backend
interface. It also does not require physical storage to resemble the logical
entry sequence, provided historical views and ordering rules are preserved.

Supporting code or dependencies needed to interpret historical schemas must be
retained or versioned sufficiently to avoid silently changing their meaning. The
concrete schema language, value encoding, persistence format, and frontend
remain implementation decisions.

The defining contract is: **immutable revisions, tree-shaped replacement,
explicit relationships, contextual type names, and validity at every prefix.**

## Appendix A. Transaction granularity and prefix validity

This appendix records the design discussion and alternatives. Sections 2 and 7
define the selected contract: single-entry transactions and validation at every
prefix.

### A.1 Validation and atomic persistence are separate choices

| Model                              | Full validation                                    | Persistence             | Status                      |
| ---------------------------------- | -------------------------------------------------- | ----------------------- | --------------------------- |
| Single-entry transactions          | Every entry prefix                                 | One entry at a time     | Default                     |
| Batches with prefix validation     | Every entry prefix                                 | Entire batch or nothing | Possible future convenience |
| Batches with final-view validation | Only the final view, plus structural append checks | Entire batch or nothing | Excluded from this contract |

The first two permit the same entry sequences when evaluated with the same
analyzers, configuration, and explicit inputs. They differ in what is saved if a
later append fails. Prefix-valid batches would still expose every entry boundary
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

### A.5 Failure handling, cost, and future batching

With single-entry persistence, a command may finish part of a split or migration
before failing. Earlier appends remain valid and committed. Recovery proceeds by
continuing or adding corrective revisions; it does not erase earlier committed
entries.

A future batch facility could stage an entire sequence, validate every prefix,
and persist none of it if any check fails. This would improve command-level
failure handling without permitting temporary type or analyzer inconsistencies.
It would not repair the construction problem in A.3.

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
