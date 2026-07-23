# Core Serialization and Typing Refactor

Status: implementation hand-off proposal as of 2026-07-23.

This proposal supersedes the plan to add generic `OrderedSet` payload
conversions in `lifecore` only to make a `NodeField` implementation possible in
`lifecore-typecheck`.

## Decision

`lifecore` should own the complete vocabulary needed to describe and serialize
typed node bodies. The main crate already owns `Body`, `Payload`, `OrderedSet`,
`LifeCore`, and the analyzer interfaces. Keeping the traits that translate
between Rust values and those core values in a downstream analyzer crate creates
an artificial dependency boundary and runs into Rust's orphan rules.

Create two public modules in the main crate:

- `lifecore::typing` owns `ValueType`, `FieldStatus`, `FieldDecl`, and `Type`,
  including type-expression parsing, display, and payload checking.
- `lifecore::serialize` owns `NodeField`, `NodeBody`, serialization errors,
  implementations for core value types, and typed conveniences layered over
  `LifeCore`.

The current trait named `NodeType` is a node-body codec and schema description,
not the stored type declaration itself. This proposal intentionally renames it
to `NodeBody`. The stored declaration remains `typing::Type`.

`lifecore-typecheck` should then contain only TypeCheck's analyzer behavior,
diagnostics, output conversion, and factory. It becomes a small, concrete
example of how an external crate implements `Analyzer<LC>` against core graph,
typing, and serialization APIs.

This relocation does **not** make TypeCheck part of basic storage validation.
`LifeCore::basic_validation` continues to enforce graph integrity, while
TypeCheck remains an optional analyzer that interprets active `Type` nodes in a
historical context.

## Why the boundary belongs in core

The current arrangement splits one abstraction across crates:

- `lifecore` owns the serialized forms (`Body`, `Payload`, and `OrderedSet`);
- `lifecore-typecheck` owns the traits that define how values use those forms;
- `lifecore-typecheck` also owns the schema values needed by any future derive
  crate or domain analyzer.

That split has several costs:

1. A downstream crate cannot implement standard conversion traits for two
   foreign core types. In particular, `lifecore-typecheck` cannot implement
   `From<OrderedSet<T>> for Payload`.
2. Other analyzers must depend on TypeCheck to describe their own node bodies,
   even when they do not want the TypeCheck analyzer.
3. `lifecore-derive` cannot generate the canonical node serialization contract
   without depending on an analyzer implementation.
4. The `LifeCore` mutation boundary accepts `Body`, but its own crate cannot
   offer typed construction without importing downstream concepts.
5. Moving only `NodeField` would still leave its return type, `ValueType`, in
   the wrong crate. Serialization therefore pulls the basic typing constructs
   into core as one coherent unit.

The dependency direction after the refactor is unambiguous:

```text
lifecore
  ├── graph/history and LifeCore
  ├── analysis interfaces
  ├── typing vocabulary
  └── serialization vocabulary
       ↑
lifecore-typecheck
  └── Analyzer implementation
```

There must be no dependency from `lifecore` back to `lifecore-typecheck`.

## Proposed core modules

### `lifecore::typing`

Move these definitions and their behavior from `lifecore-typecheck`:

```rust
pub enum FieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

pub enum ValueType {
    PreciseTime,
    Text,
    Number,
    TextLiteral(String),
    NumberLiteral(i64),
    Sequence(Box<ValueType>),
    Tuple(Vec<ValueType>),
    OrderedSet(Box<ValueType>),
    OneOf(Vec<ValueType>),
}

pub struct FieldDecl {
    name: String,
    ty: ValueType,
    status: FieldStatus,
}

pub struct Type {
    pub name: String,
    pub body: OrderedSet<FieldDecl>,
    pub outgoing: OrderedSet<String>,
    pub incoming: OrderedSet<String>,
    pub additional_override: OrderedSet<String>,
}
```

The move includes:

- `FromStr` and `Display` for `ValueType`, `FieldStatus`, and `FieldDecl`;
- literal string and signed-integer syntax;
- quote-aware `FieldDecl` parsing;
- `ValueType::check(&Payload)`;
- `Type::new`;
- the canonical self-description of `Type`;
- constructors and read-only accessors for `FieldDecl`.

`FieldDecl` needs a public constructor plus `name()`, `value_type()`, and
`status()` accessors. Its fields are currently accessible to TypeCheck only
because the declaration and analyzer share a crate. After the move, the analyzer
must be able to inspect declarations without making their representation
mutable.

Type syntax and runtime matching are core capabilities, not analyzer policy.
Whether a missing mandatory field is an inconsistency, for example, remains
TypeCheck policy.

### `lifecore::serialize`

The serialization traits should directly express the operations they require:

```rust
pub trait NodeField: Sized {
    fn describe_self() -> ValueType;
    fn into_payload(self) -> Payload;
    fn try_from_payload(value: &Payload) -> Result<Self, Error>;
}

pub trait NodeBody: Sized {
    const TYPE_NAME: &'static str;

    fn describe_self() -> Type;
    fn into_body(self) -> Body;
    fn try_from_body(value: &Body) -> Result<Self, Error>;
}
```

This replaces the current supertrait forms:

```rust
Into<Payload> + TryFrom<&Payload>
Into<Body> + TryFrom<&Body>
```

The explicit methods are intentional:

- the blanket `NodeField` implementation can use one known error type;
- implementations are no longer constrained by orphan rules around `From` and
  `TryFrom`;
- the unnecessary trait lifetime disappears because deserialization returns an
  owned value;
- error paths can retain field names and collection indices;
- a future derive macro has one stable contract to generate.

Concrete core types may additionally implement conventional `From` or `TryFrom`
when coherence permits, but those traits are conveniences rather than the
`NodeField` contract. Do not recreate a generic `From<T> for Payload where
T: NodeField`; it overlaps standard conversion implementations and makes the
trait boundary less explicit.

`serialize::Error` should be independent of `LifeCoreError`. Serialization is a
value conversion concern, not a graph mutation, environment, or analyzer
failure. It should distinguish at least:

- an unexpected `PayloadKind`;
- a missing body field;
- an invalid named field with a nested source error;
- an invalid collection element with its stable iteration index;
- an invalid tuple arity;
- invalid type or field-declaration syntax.

TypeCheck converts a failed `Type::try_from_body` into its existing
`MalformedNodeTypeBody` diagnostic. Core callers may choose how to present the
more detailed serialization error.

## The `OrderedSet<T>` blanket implementation

Once `NodeField` and `OrderedSet` are both core concepts, the desired blanket
implementation is straightforward:

```rust
impl<T> NodeField for OrderedSet<T>
where
    T: NodeField + Eq + Hash,
{
    fn describe_self() -> ValueType {
        ValueType::OrderedSet(Box::new(T::describe_self()))
    }

    fn into_payload(self) -> Payload {
        Payload::OrderedSet(
            self.into_iter()
                .map(NodeField::into_payload)
                .collect(),
        )
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        let Payload::OrderedSet(values) = value else {
            return Err(Error::unexpected_payload(
                ValueType::OrderedSet(Box::new(T::describe_self())),
                value,
            ));
        };

        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                T::try_from_payload(value)
                    .map_err(|source| Error::invalid_element(index, source))
            })
            .collect()
    }
}
```

The exact helper constructors on `Error` may differ, but the observable behavior
should not:

- order is preserved;
- an element failure identifies its position;
- an outer non-set payload is distinct from an invalid member;
- duplicates follow `OrderedSet` construction semantics;
- nested `OrderedSet<OrderedSet<T>>` works recursively.

The `Eq + Hash` bounds are required by today's `OrderedSet` definition, not by
the conceptual `NodeField` contract. Keep them only on implementations that the
compiler currently requires. Do not expose `IndexSet`, its iterator types, its
entry API, or slice access through public serialization APIs. If `OrderedSet`
later drops `Hash` or changes its backing collection, the trait and wire format
should remain unchanged.

Implement `NodeField` initially for:

- `String` as `ValueType::Text`;
- `i64` as `ValueType::Number`;
- `Timestamp` as `ValueType::PreciseTime`;
- `FieldStatus` using its literal `OneOf` declaration;
- `ValueType` using its text syntax;
- `FieldDecl` using its three-element sequence/tuple representation;
- `OrderedSet<T>` through the blanket implementation.

The same pattern can later cover `Vec<T>` as `Sequence(T)` and Rust tuples as
fixed `Tuple` types without involving TypeCheck.

## `Body` and `OrderedSet` ergonomics

The earlier collection-ergonomics work remains useful, but it should now land
as supporting core serialization rather than as an orphan-rule workaround.

`Body` should provide a map-like facade:

- `new`, `get`, `get_mut`, `insert`, `remove`, and `contains_key`;
- `iter`, `iter_mut`, `keys`, `values`, and `values_mut`;
- `len` and `is_empty`;
- `FromIterator` and `Extend` for convertible field names and payloads;
- convenient construction from `(&str, Payload)` arrays and slices.

`OrderedSet` should provide a representation-independent collection facade:

- `new`, `len`, `is_empty`, `contains`, positional `get`, `first`, and `last`;
- `insert`, order-preserving `remove`, `clear`, and `retain`;
- `FromIterator`, `Extend`, array/vector construction, and owned/borrowed
  iteration.

If standard `IntoIterator` is implemented, use Lifecore-owned iterator wrapper
types whose private storage may change. Do not name an `indexmap` type in a
public associated type. Mutable element iteration must not be offered because
changing a value in place can invalidate set equality and hashing invariants.

`NodeField for OrderedSet<T>` should consume only this public facade. Core code
may technically access the private backing field, but dogfooding the facade
ensures it is sufficient for downstream serialization and survives a backing
collection change.

## Relationship to `LifeCore`

`LifeCore` should remain a backend and graph-history abstraction. Its required
mutation method continues to accept the stable storage representation:

```rust
fn add_mock(
    &mut self,
    ty: &str,
    payload: Body,
    parent: Option<EntryContext>,
) -> Result<Node, LifeCoreError>;
```

Do not require every backend to add generic typed methods. Instead,
`lifecore::serialize` should define a blanket extension trait:

```rust
pub trait LifeCoreSerializeExt: LifeCore {
    fn add_typed_mock<T: NodeBody>(
        &mut self,
        value: T,
        parent: Option<EntryContext>,
    ) -> Result<Node, LifeCoreError> {
        self.add_mock(T::TYPE_NAME, value.into_body(), parent)
    }
}

impl<LC: LifeCore> LifeCoreSerializeExt for LC {}
```

This gives every backend typed insertion without expanding the required
`LifeCore` implementation surface. Direct `add_mock` remains available for
dynamic type names, CLI input, sidecars, analyzer suggestions, and malformed
data tests.

Do not initially add a typed lookup that silently resolves or validates a
node's schema. A node's applicable `Type` depends on its event and inspection
window. Callers should obtain a context-preserving `Node` through `LifeCore`,
check its stored `ty_name` when appropriate, and explicitly call
`T::try_from_body(&node.field)`. This keeps serialization separate from
historical type resolution.

Likewise, moving `Type` into core does not mean `LifeCore::add_mock` should run
TypeCheck. Core mutation still gates on `basic_validation`; applications such as
the CLI may continue to impose the stricter analyzer-gated transaction policy.

## What remains in `lifecore-typecheck`

After the move, the crate should retain:

- `TypeCheckMessage` and its conversion into generic analysis messages;
- `TypeCheckOutput` and its `InnerOutput` implementation;
- `TypeCheck<LC>` and `TypeCheckFactory<LC>`;
- active-type discovery and all historical TypeCheck policy;
- bootstrap, duplicate declaration, field policy, edge permission, and override
  diagnostics;
- analyzer-focused integration tests.

It should import the basic pieces explicitly:

```rust
use lifecore::{
    analysis::{Analyzer, AnalyzerFactory, InnerMessageType, InnerOutput},
    serialize::{NodeBody, NodeField},
    typing::{FieldDecl, FieldStatus, Type, ValueType},
    LifeCore,
};
```

The crate should no longer contain `parse.rs`, `serialize.rs`, primitive codecs,
or the definitions of the typing model. Tests for syntax, codecs, collection
round trips, and `ValueType::check` move to `lifecore`. Tests in
`lifecore-typecheck` should demonstrate analyzer construction, initialization,
historical-prefix analysis, diagnostic conversion, and factory registration.
That focused layout makes the crate a useful template for
`lifecore-kman`, `lifecore-prod`, and other analyzer packages.

Temporary `pub use lifecore::{serialize::..., typing::...}` re-exports may be
used for one migration commit if needed to keep the workspace compiling, but
they should be removed before the refactor is considered complete.
`lifecore-typecheck` must not remain the public home of core typing names.

## Migration sequence

Implement the refactor in dependency order:

1. Add representation-independent `Body` and `OrderedSet` methods and tests in
   `lifecore`. Replace new `.0` access in downstream code with those methods.
2. Add `typing` to `lifecore`; move the four typing data structures, parsers,
   displays, `ValueType::check`, constructors, accessors, and their unit tests
   without changing the wire syntax.
3. Add `serialize::Error`, `NodeField`, and `NodeBody`. Implement primitive
   fields, schema fields, `Type` body serialization, and the recursive
   `OrderedSet<T>` field implementation.
4. Add `LifeCoreSerializeExt` and tests against `LifeCoreList`.
5. Change `lifecore-typecheck` to import the new core modules. Move
   serialization/typing tests to core and leave analyzer tests downstream.
6. Remove the old `NodeType`, `NodeField`, `ValueType`, `FieldStatus`,
   `FieldDecl`, and `Type` definitions and the temporary re-exports from
   `lifecore-typecheck`.
7. Update `lifecore-derive` design notes and repository guidance so generated
   node bodies target `lifecore::serialize::NodeBody`.

The move should preserve the current persisted representation:

- value type expressions remain text;
- text literals remain unescaped double-quoted text;
- integer literals remain signed decimal `i64`;
- `FieldDecl` remains a three-element `Payload::Sequence`;
- statuses retain canonical serialization and canonical/lowercase parsing;
- `Type` bodies retain the existing field names and ordered-set values;
- the stored type name returned by the current declaration type remains stable.

No state migration should be required solely because Rust module paths and trait
names change.

## Verification and acceptance criteria

The refactor is complete when:

1. A crate depending only on `lifecore` can define a `NodeBody`, describe its
   `Type`, serialize it to `Body`, and deserialize it again.
2. `OrderedSet<String>`, `OrderedSet<i64>`,
   `OrderedSet<Timestamp>`, and a recursively nested ordered set satisfy
   `NodeField` and round-trip through `Payload`.
3. Collection failures report whether the outer payload or a particular member
   failed without mentioning `IndexSet`.
4. `LifeCoreList::add_typed_mock` works through the extension trait while the
   required `LifeCore` trait surface remains unchanged.
5. Existing type syntax and `FieldDecl`/`Type` wire-format tests pass from the
   core crate.
6. `lifecore-typecheck` has no private copy of the typing or serialization
   model and remains a functioning `Analyzer`/`AnalyzerFactory`.
7. TypeCheck still analyzes every historical prefix and produces the same
   diagnostics for the same graph.
8. The complete workspace tests pass, targeted strict Clippy checks introduce
   no new warnings, formatting is clean for touched files, and unrelated
   working-tree changes remain intact.

## Deferred work

This refactor establishes ownership and serialization mechanics. It does not:

- move TypeCheck's semantic rules into `basic_validation`;
- resolve the active `Type` inside `LifeCore::node`;
- add implicit typed reads that ignore historical context;
- implement the pending field-policy checks in TypeCheck;
- design derive attributes for `lifecore-derive`;
- remove `Eq + Hash` from `OrderedSet`;
- choose a replacement for `IndexSet`;
- add sequence, optional, map, or arbitrary serialized-value `NodeField`
  blankets.

Those can build on the core traits later without changing the crate dependency
direction established here.
