# Initial Data Model and Query System Proposal

Status: design proposal and audit of the refactored list-backed sketch as of
2026-07-21.

## Summary

The current refactor is directionally consistent with the proposed data model,
but it is not yet the expected contract.

The strongest parts of the current sketch are:

- `Payload` is recursive through `Aggregate(Vec<Payload>)`.
- `LifeCoreList` interns strings, values, and aggregate member lists behind
  stable indices.
- query predicates cover field existence, kind, equality, text matching,
  aggregate membership and cardinality, freshness, stored node type names, edge
  relationships, and negation.
- field paths can descend through nested aggregates.
- the type-checking refactor contains a sketch of the intended recursive
  `ValueType` algebra: `Sequence`, `Tuple`, `Set`, and `OneOf`.

Those are the expected foundations. The important differences are:

- the value model has one ordered `Aggregate` and therefore cannot distinguish a
  sequence from a set;
- homogeneous and heterogeneous collection rules exist only in the stubbed type
  schema and are not connected to values or validation;
- `query_pick_node` currently means logical AND while `query_filter_node` means
  logical OR, reversing the established CLI meanings;
- wildcard paths silently change predicates to universal quantification, whereas
  the proposed model makes quantification explicit and uses existential matching
  as the ordinary default;
- a query does not have one governing historical context;
- the public query API cannot construct `QueryFieldPath` values, and some path
  misses panic instead of producing no selected values;
- the Boolean model supports only one flat AND or OR plus leaf-level `Not`, not
  nested expressions such as `A AND (B OR C)`;
- some individual reference predicates are incomplete or incorrect, notably
  non-fresh edge-type predicates that do not compare the edge's type;
- `Serialized` values are currently inserted and resolved as `Text` in one
  reference implementation path.

The initial implementation should retain the simple list backend and its
interning scheme, but establish a backend-independent logical value model and
query AST before a relational implementation is attempted.

## Scope

This proposal covers:

- the logical graph and history model relied upon by queries;
- scalar, sequence, set, tuple, union, and opaque values;
- the initial in-memory/interned representation;
- a backend-independent node-query model;
- evaluation rules for the list reference implementation;
- the expected future relational projection;
- how the current refactored sketch differs from that model.

Detailed validation of TypeCheck and sidecar behavior is deliberately deferred.
Both are currently affected by the wider refactor. Their expected integration
points are recorded so the initial model does not foreclose them.

## Design goals

1. Preserve the typed, versioned graph as the central model.
2. Make every historical query operate within an explicit bounded context.
3. Support recursively nested sequences and sets without encoding them as opaque
   JSON.
4. Represent homogeneous and heterogeneous collection constraints in the type
   declaration, not by trying to infer them from stored values.
5. Keep `Serialized` as an explicit opaque escape hatch for downstream users.
6. Give the list backend and a future SQL backend the same observable query
   semantics.
7. Preserve deterministic result and set-member presentation.
8. Make invalid paths and incompatible values ordinary non-matches or typed
   query-construction errors, never panics.
9. Keep the initial system small enough to validate with conformance tests
   before optimizing it.

## Non-goals for the initial version

- Understanding or recursively validating arbitrary `Serialized` values.
- Querying inside `Serialized` values.
- Defining all future scalar kinds, such as floating-point, decimal, binary, or
  geographic values.
- Building a cost-based query optimizer.
- Moving TypeCheck's full semantics into core storage validation.
- Choosing a final SQL engine or Rust database library.

## Graph and history model

Entries remain append-only and strictly ordered. An entry is either a node or an
edge. A node may name an older node as its parent, thereby superseding it in
contexts that include the child. An edge refers to two older nodes.

`Context` remains a history window from entry zero to an exclusive end.
`EntryContext` remains a particular entry inspected through such a window.

All query evaluation must use a finite, explicit context:

```rust
pub struct NodeQuery {
    pub context: Context,
    pub expression: NodeQueryExpr,
}
```

The public API may offer a `latest_context()` convenience, but the query plan
itself should never silently use `Context::universe()`. This ensures that:

- candidates come only from the selected history window;
- node and edge freshness are evaluated in the same window;
- returned `Node.e` values carry the query window;
- future schema-sensitive predicates can resolve the schema active in that
  window;
- list and relational implementations receive the same context bound.

Absolute contexts remain appropriate for comparing entry identity, not as the
default inspection context.

## Logical value model

### Runtime values

The initial public value model should distinguish ordered and unordered
containers:

```rust
pub enum Payload {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),
    Sequence(Vec<Payload>),
    Set(PayloadSet),
    Serialized(JSON),
}
```

`PayloadSet` should be an opaque collection with checked constructors rather
than a public `Vec<Payload>` alias:

```rust
pub struct PayloadSet {
    canonical_members: Vec<Payload>,
}
```

Its invariants are:

- semantic equality is independent of insertion order;
- duplicate members are rejected rather than silently discarded;
- members have deterministic canonical iteration order;
- type participates in equality, so `Text("1")` differs from `Number(1)`;
- nested sequences remain order-sensitive;
- nested sets remain order-insensitive.

The implementation should define one canonical, type-tagged encoding for payload
equality, hashing, and set ordering. Set members can then be ordered by their
canonical bytes without requiring `Payload: Ord`. The encoding must be
recursive, length-delimited, and deterministic; serialized JSON objects must use
canonical key order. This same encoding can later supply relational value
identity and diagnostic hashing.

No separate runtime variants are needed for homogeneous and heterogeneous
collections. An empty value cannot reveal its intended member type, and the same
value can legitimately be inspected under different historical schemas.
Homogeneity is consequently a property of `ValueType`.

Tuples also do not require a separate runtime payload. They use the ordered
`Sequence` representation and acquire fixed length and positional member types
from their declaration.

`Serialized` remains opaque. Core may compare an entire serialized value for
equality, persist it, and return it, but it does not interpret nested paths or
claim structural type safety for downstream formats.

### Type expressions

The intended recursive schema is:

```rust
pub enum ValueType {
    PreciseTime,
    Text,
    Number,
    Sequence(Box<ValueType>),
    Tuple(Vec<ValueType>),
    Set(Box<ValueType>),
    OneOf(Vec<ValueType>),
    Serialized,
}
```

It expresses the requested collection forms as follows:

| Requirement                     | Type expression                   |
| ------------------------------- | --------------------------------- |
| Homogeneous sequence            | `Sequence(Text)`                  |
| Variable heterogeneous sequence | `Sequence(OneOf([Text, Number]))` |
| Fixed heterogeneous sequence    | `Tuple([Text, Number])`           |
| Homogeneous set                 | `Set(Text)`                       |
| Heterogeneous set               | `Set(OneOf([Text, Number]))`      |

Type expressions should be canonicalized:

- `OneOf` must have at least one distinct alternative;
- duplicate union alternatives are rejected;
- nested `OneOf` expressions may be flattened;
- tuple order is significant;
- set member type is a single expression, which may itself be `OneOf`;
- recursive value types are trees, not cyclic user-defined references.

Field optionality remains a field-policy concern. It is represented by absence
from `Body`, not by a null payload.

## Initial list-backed physical model

The current reference implementation's use of interned tables is a reasonable
prototype for relational normalization. The initial implementation can retain
that approach:

```rust
struct LifeCoreList {
    entries: Vec<StoredEntry>,
    circumstances: Vec<CircumstanceIndex>,
    suite: SuiteConfig,

    circumstance_table: IndexSet<Circumstance>,
    string_table: IndexSet<String>,
    value_table: IndexSet<StoredValue>,
    aggregate_table: IndexSet<StoredAggregate>,
}

enum StoredValue {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),
    Sequence(AggregateIndex),
    Set(AggregateIndex),
    Serialized(StringIndex),
}

struct StoredAggregate {
    members: Vec<ValueIndex>,
}
```

The `Sequence` or `Set` tag belongs on `StoredValue`; an aggregate member list
can then be shared by both kinds without conflating their semantics. An
alternative is to put the tag in `StoredAggregate`, but there must be exactly
one authoritative kind tag.

On insertion:

1. recursively intern every child payload;
2. preserve sequence member order;
3. validate set uniqueness;
4. canonicalize set member order before interning;
5. intern the aggregate member list;
6. intern a value carrying the correct sequence/set tag;
7. reference the final `ValueIndex` from the node body.

On resolution, the inverse process must preserve the exact public variant.
`Serialized` must resolve as `Serialized`, never as `Text`.

Every stored reference must be validated before indexing. Basic validation
should eventually check value and aggregate indices recursively in addition to
entry, edge, parent, and circumstance integrity. Reachability checks for
unreferenced interned values are useful integrity diagnostics, but they are not
required for query semantics and should not complicate the initial refactor.

The fact that every scalar currently has a `ValueIndex` indirection is not a
logical-model problem. It is a deliberate normalization tradeoff. A later SQL
backend can store scalar columns on a value row and use the same value identity
for node fields and aggregate members.

## Mutation and validation boundary

Aggregate support should not change the existing transaction boundary:

1. construct a complete public `Payload` tree;
2. stage it through `add_mock` on an isolated candidate;
3. recursively intern values and aggregate membership;
4. run core storage validation;
5. later run configured analyzers, including TypeCheck when it is restored;
6. apply the validated mock suffix or discard the candidate.

A failed mutation must not leave residual strings, values, or aggregate rows in
the live store. The current clone-and-replace reference strategy provides this
property naturally. A relational backend can provide the same behavior with a
database transaction and can query provisional sidecar rows before commit.

Core validation owns structural invariants such as valid indices, valid entry
references, aggregate acyclicity by construction, and set uniqueness. TypeCheck
owns whether a structurally valid payload satisfies the active historical
`ValueType` declaration.

## Query model

### One Boolean expression tree

Core should expose one query operation rather than separate methods whose names
encode only the root connector:

```rust
pub enum NodeQueryExpr {
    All(Vec<NodeQueryExpr>),
    Any(Vec<NodeQueryExpr>),
    Not(Box<NodeQueryExpr>),
    Predicate(NodePredicate),
}

pub trait LifeCore {
    fn query_nodes(&self, query: &NodeQuery) -> Result<Vec<Node>, QueryError>;
}
```

This supports nested expressions such as:

```text
NodeType("Note")
AND
(tags contains "home" OR tags contains "work")
AND
NOT Fresh
```

CLI `filter` and `pick` are conveniences over this tree:

```text
query filter P1 P2 ...  => All([P1, P2, ...])
query pick   P1 P2 ...  => Any([P1, P2, ...])
```

The empty identities are intentional:

```text
All([]) = true   // empty filter returns every node in the query context
Any([]) = false  // empty pick returns no nodes
```

These are the meanings already established by the pre-refactor CLI and are the
meanings that map directly to SQL `AND` and `OR`.

### Predicates

An initial predicate set can consolidate the current variants while adding the
missing comparison capability:

```rust
pub enum NodePredicate {
    FieldExists(QueryFieldPath),
    FieldTest {
        path: QueryFieldPath,
        quantifier: QueryQuantifier,
        test: ValuePredicate,
    },
    NodeType(String),
    Fresh,
    EdgeExists {
        direction: EdgeDirection,
        type_name: String,
        freshness: EdgeFreshness,
    },
}

pub enum ValuePredicate {
    Kind(PayloadKind),
    Equal(Payload),
    TextMatches(QueryRegex),
    Compare(QueryComparison, ScalarPayload),
    AggregateContains(Payload),
    AggregateElementCount(QueryComparison, usize),
}

pub enum QueryQuantifier {
    Any,
    All,
}
```

`Compare` initially applies only to `Number` and `PreciseTime`, comparing like
kinds. `Equal` retains deep equality for sequences and sets. A kind mismatch is
a non-match.

`Fresh` and fresh edge predicates use `NodeQuery.context`. They should not each
carry unrelated contexts in the initial API. If cross-context comparison later
proves necessary, it should be a separately named advanced predicate.

### Field paths and quantification

A path selects zero or more payloads:

```rust
pub struct QueryFieldPath {
    field_name: String,
    selectors: Vec<QueryFieldPathSelector>,
}

pub enum QueryFieldPathSelector {
    Last,
    Any,
    Index(usize),
}
```

The type needs public checked constructors or a public parser. Its fields need
not be public.

Resolution rules are:

- a missing top-level field selects no values;
- `Index` and `Last` operate only on sequences;
- an out-of-range index or `Last` on an empty sequence selects no values;
- `Any` visits every immediate member of either a sequence or a set;
- a selector applied to a scalar selects no values;
- paths do not descend into `Serialized`;
- resolution never panics;
- sequence indices are zero-based in core, while a CLI may translate from its
  one-based user convention.

Wildcards should not silently impose universal semantics. The path selects
values; `QueryQuantifier` says how to combine the tests.

For both `Any` and `All`, no selected values means false. This preserves the
useful current rule that field predicates imply path existence while avoiding
vacuous truth surprises. Explicit negation can still select nodes where a path
does not exist:

```text
Not(FieldExists(path))
```

Existential matching should be the ordinary CLI default because it maps to
`EXISTS` and matches common wildcard expectations. Universal matching remains
available explicitly and maps to `EXISTS` plus `NOT EXISTS` a counterexample.

### Reference evaluation algorithm

`LifeCoreList::query_nodes` should be intentionally simple:

1. enumerate nodes in `query.context` in append order;
2. evaluate `NodeQueryExpr` recursively with short-circuiting;
3. resolve paths by borrowing interned values where practical;
4. evaluate `Any` or `All` explicitly;
5. inspect only edges included in `query.context`;
6. use `is_stale` with the same context for freshness;
7. return matching nodes with `Node.e` set to the query context.

The reference implementation is the semantic oracle, not the performance
implementation. It should favor clear behavior and conformance tests over
index-aware shortcuts.

## Relational projection

The logical model maps to a normalized database without changing query
semantics:

```text
entry(id, kind, type_name_id, parent_id, from_id, to_id, circumstance_id)
node_field(node_entry_id, field_name_id, value_id)
value(id, kind, precise_time, text_id, number, aggregate_id, serialized_id)
sequence_member(aggregate_id, ordinal, member_value_id)
set_member(aggregate_id, member_value_id)
string(id, value)
```

Sequences enforce unique `(aggregate_id, ordinal)`. Sets enforce unique semantic
members. Global value interning can make `member_value_id` sufficient for set
uniqueness; otherwise the backend needs a canonical value identity or
collision-safe canonical representation.

Every field predicate can compile to an independent correlated `EXISTS`
subquery. `All` joins those conditions with `AND`; `Any` joins them with `OR`.
Independent subqueries are necessary so two predicates may match different
members of the same collection. A query builder is an implementation detail; the
`NodeQueryExpr` AST remains the portability boundary.

## Comparison with the current sketch

### Data model

| Area                            | Current sketch                                      | Proposed initial contract              | Assessment                                         |
| ------------------------------- | --------------------------------------------------- | -------------------------------------- | -------------------------------------------------- |
| Recursive values                | `Aggregate(Vec<Payload>)`                           | recursive `Sequence` and `Set`         | good foundation, incomplete distinction            |
| Sequence identity               | ordered aggregate vector                            | explicit ordered sequence              | behavior exists but is unnamed                     |
| Set identity                    | absent                                              | explicit unordered unique set          | missing                                            |
| Tuple values                    | no runtime distinction                              | sequence value plus tuple schema       | compatible                                         |
| Homogeneous/heterogeneous rules | stubbed `ValueType` only                            | recursive schema used by TypeCheck     | desired algebra is sketched; integration deferred  |
| Value interning                 | strings, values, and aggregate lists use `IndexSet` | retain as reference normalization      | expected                                           |
| Aggregate storage               | interned `Vec<ValueIndex>`                          | interned members plus sequence/set tag | close                                              |
| Set canonicalization            | absent                                              | deterministic and order-independent    | missing                                            |
| Opaque values                   | `Serialized` variant exists                         | preserve opaque variant exactly        | concept correct; reference conversion is defective |
| Context-bearing nodes           | `Node.e` preserves requested context                | preserve query context                 | core access is compatible                          |

The `Serialized` defect is concrete: `insert_payload` creates `Value::Text` for
a `Payload::Serialized`, and `get_payload` also resolves `Value::Serialized` as
`Payload::Text`. This loses the payload kind and can corrupt equality and query
results.

### Query model

| Area                      | Current sketch                                                 | Proposed initial contract                       | Assessment                                      |
| ------------------------- | -------------------------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| Candidate domain          | all nodes in `Context::universe()`                             | nodes in explicit query context                 | must change                                     |
| `filter`                  | logical OR / `any`                                             | logical AND / `all`                             | reversed                                        |
| `pick`                    | logical AND / `all`                                            | logical OR / `any`                              | reversed                                        |
| Empty filter              | rejected by panic                                              | all nodes in context                            | reversed identity                               |
| Empty pick                | empty result                                                   | empty result                                    | result agrees, rationale should be `Any([])`    |
| Boolean composition       | flat root plus predicate `Not`                                 | recursive `All`/`Any`/`Not` AST                 | incomplete                                      |
| Field existence           | implemented                                                    | implemented                                     | close                                           |
| Kind and equality         | implemented                                                    | implemented with explicit quantifier            | close                                           |
| Regex                     | Rust regex over text                                           | backend-neutral predicate with backend compiler | usable reference behavior; portability deferred |
| Aggregate contains        | immediate membership                                           | immediate sequence/set membership               | close for sequences only                        |
| Cardinality               | exact equality only                                            | comparison operator                             | partial; `QueryComparison` is unused            |
| Scalar comparison         | absent                                                         | number/time comparison                          | missing                                         |
| Field paths               | nested `Last`, `Any`, `Index`                                  | same selectors, safe and constructible          | close in shape, unsafe/inaccessible in API      |
| Wildcard semantics        | implicit universal `all`                                       | explicit `Any` or `All`, existential default    | semantic mismatch                               |
| Missing/out-of-range path | some cases panic through `unwrap`                              | select no values                                | defect                                          |
| Stored node type          | implemented                                                    | implemented                                     | expected                                        |
| Edge existence            | implemented variants                                           | direction/type/freshness predicate              | close in shape                                  |
| Edge type check           | non-fresh variants only check that the string exists somewhere | compare each candidate edge's type              | defect                                          |
| Freshness context         | context carried by individual predicates                       | one query-wide context                          | should be simplified                            |
| Public construction       | `QueryFieldPath` fields private and no constructor             | checked constructors/parser                     | currently unusable downstream                   |
| Result ordering           | append order                                                   | append order                                    | expected                                        |
| Error surface             | `Vec<Node>` and assertions                                     | `Result<Vec<Node>, QueryError>`                 | incomplete                                      |

The current universal wildcard behavior is internally consistent because every
field predicate first checks that the path selects at least one value. It is not
inherently unsound, but it is surprising as an implicit property of a path and
differs from the previously outlined existential wildcard. The initial API
should preserve universal matching as an explicit quantifier rather than remove
it.

## Deferred integration areas

### TypeCheck

The current `lifecore-typecheck` source contains the expected `ValueType`
variants, but the crate is in the middle of the refactor and still mixes old and
new core names and interfaces. This proposal does not validate its current
behavior or compilation state.

When resumed, TypeCheck should:

- resolve the active `Node_Type` in the inspected historical context;
- recursively validate sequences, tuples, sets, and unions;
- diagnose nested failures with a field/member path;
- reject duplicate set members independently of member-type checking;
- treat `Serialized` as an opaque kind only;
- avoid making the core query engine depend on active type resolution.

### Sidecars and singular CLI input

The current sidecar and singular-add paths still support only text and integer
payloads and live outside the active workspace during this refactor. Their
current behavior was not validated here.

When resumed, both should share one explicit tagged conversion grammar:

```toml
title = "Buy Milk"
priority = 2
tags = { set = ["errand", "home"] }
history = { sequence = ["created", 1] }
when = { precise_time = "2026-07-21T10:00:00Z" }
opaque = { serialized = { downstream = true } }
```

The conversion layer should construct values only. TypeCheck should decide
whether the value satisfies the active homogeneous, heterogeneous, tuple, or set
declaration.

### CLI queries

The CLI still contains the earlier event-query implementation and old core
names. Once the core query AST stabilizes, CLI `query filter` and `query pick`
should compile into `All` and `Any` respectively instead of maintaining a second
predicate evaluator. Alias-based historical selection should establish
`NodeQuery.context` before evaluation.

## Validation status of this audit

The following checks were performed against the current tree:

- `cargo check -p lifecore --lib` succeeds, with refactor-related warnings.
- `cargo test -p lifecore --lib --no-run` does not compile because the active
  root tests still use removed pre-refactor names and mutation methods.
- the reference query and aggregate behavior was inspected directly because
  query-specific tests are disabled or absent and `QueryFieldPath` cannot be
  constructed through the public API.
- TypeCheck and sidecar functionality were intentionally not validated, as
  requested.

Consequently, this document validates the shape and implementation logic of the
new sketch, not a passing end-to-end system.

The principal sources inspected were [`src/lib.rs`](../src/lib.rs),
[`src/reference_impl/mod.rs`](../src/reference_impl/mod.rs), and
[`src/reference_impl/boilerplate.rs`](../src/reference_impl/boilerplate.rs). The
deferred integration observations came from
[`lifecore-typecheck/src/lib.rs`](../lifecore-typecheck/src/lib.rs) and
[`lifecore-cli/src/sidecar.rs`](../lifecore-cli/src/sidecar.rs).

## Minimum conformance suite

Before implementing SQL, every `LifeCore` backend should pass the same cases:

- every payload kind round-trips without changing its kind;
- nested sequence order affects equality;
- set insertion order does not affect equality, hashing, or display;
- duplicate set members are rejected atomically;
- homogeneous, union, and tuple-shaped values can all be represented without
  using `Serialized`;
- a query never returns a node outside its context;
- returned node contexts equal the query context;
- `filter` combines predicates with AND and `pick` with OR;
- empty filter and pick use `All([])` and `Any([])` identities;
- nested Boolean expressions and negation short-circuit correctly;
- wildcard `Any` and `All` are distinct and both fail on an empty selection;
- missing fields, scalar traversal, empty aggregates, and invalid indices do not
  panic;
- two `contains` predicates may match different members of one aggregate;
- non-fresh and fresh edge predicates both check direction and edge type;
- node and edge freshness changes when the query context changes;
- query results remain in append order.

## Recommended implementation order

1. Correct `Serialized` insertion and resolution so payload kinds round-trip.
2. Split the runtime aggregate into `Sequence` and `Set`, retaining the interned
   value/member tables.
3. Define checked set construction, duplicate behavior, canonical equality,
   hashing, and presentation.
4. Add public safe constructors and tests for `QueryFieldPath`.
5. Introduce `NodeQuery`, `NodeQueryExpr`, explicit quantifiers, and one
   `query_nodes` method.
6. Make `filter = All`, `pick = Any`, and lock down their empty identities with
   generic conformance tests.
7. Make path resolution total: missing fields, wrong kinds, empty aggregates,
   and out-of-range indices must not panic.
8. Complete and test every initial predicate, including edge type matching,
   comparison, context bounds, negation, and nested Boolean expressions.
9. Run the same query conformance suite against `LifeCoreList` and any future
   relational backend.
10. Restore TypeCheck against the stable recursive value/type interfaces.
11. Restore CLI and sidecar conversion on top of the shared core model.

The list implementation should remain deliberately simple through steps 1-9. Its
most important job is to define correct, inspectable semantics that a later SQL
compiler must reproduce.
