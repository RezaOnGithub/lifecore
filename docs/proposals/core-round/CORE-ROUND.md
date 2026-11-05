# Core round: value kinds, typed payloads, and indexes

Status: proposed; steps 1 (the benchmark), 2 (core indexes), 3a (value kinds),
3b (their editors), 4 (typed decoding), and 5 (index templates) of section 6
have landed, and the rest is not implemented. This is a design document for
core work, not an analyzer proposal, so it has no explainer or harness. As
each step of section 6 lands, its contracts move into the owning documents
listed in section 7. When the last one has moved, this directory is removed.
It relies on [SPEC](../../SPEC.md), [ANALYZERS](../../ANALYZERS.md),
[FILES](../../FILES.md), and [PERFORMANCE](../../PERFORMANCE.md).

The planning analyzer motivates most of this round, but it is shelved
([AGENTS.md](../../../AGENTS.md#planning-is-shelved)). PLANNING and
`lifecore-prod` are cited as of `59c702c`, their last state, and "Planning"
below means that analyzer.

## 0. Summary

The planning analyzer was built against a core that could not express what it
needed, and the gaps were absorbed by PLANNING instead of fixed in the core.
This round fixes three of them before more domain work is built on top:

1. **Value kinds** (section 2). Add `duration`, `date`, and `offset_timestamp`
   payload kinds; restore `enum` as a field shape over text; add a `variant`
   payload kind for tagged alternatives. One rule governs time: store what
   was observed, never a conversion that a time-zone database would have to
   redo.
2. **Typed payload decoding** (section 3). A `FromPayload` trait and derive
   in the core, strict about kinds and lenient about extra fields, with one
   structured error. The same derive emits a canonical declaration.
3. **Indexes** (section 4). Core indexes owned by `LifeCoreList`, and index
   templates from a fixed catalogue that analyzers declare at instantiation,
   all narrowed by the view boundary. Every index is optional: a user gives
   the same results, order, and errors whether it reads an index or computes
   the answer itself. Analyzers stay functions of the view, and every prefix
   is still replayed.

Section 5 collects what this means for Planning, including a habit with three
check-ins a day, as the starting input for a separate revision round of
PLANNING. Section 6 orders the work.

## 1. Fixed points

These are decisions of the owner, not open questions:

- Edges stay lightweight (no payload), exact-endpoint, and never retargeted.
  Duplicate edges and duplicate current nodes stay rejected
  ([SPEC §7.1](../../SPEC.md#71-append-checks)). Reconnect and the
  re-pointing rules of Planning are the intended price of revision.
- The transaction model (`prepare`, `PreparedTransaction`, `Checked`) is not
  reshaped.
- Every prefix is replayed. A "last N" check, which would replay fewer, is a
  last resort, not planned work (section 4.7).
- Analyzers do not keep state across calls, and do not declare state of their
  own for the runner to carry. Incremental work lives in core-built indexes
  (section 4).
- Compatibility is not a concern: snapshots and change files may stop loading.

## 2. Value kinds

*Landed in steps 3a and 3b.* The contracts below
are owned by the documents section 6 names for that step; where this section
and they differ, they hold, and this section stays as the design's record.

### 2.1 Problems

`Payload` has `Text`, `Integer`, `Boolean`, `Timestamp`, `List`, and `Record`
(`src/values.rs`); field shapes mirror them (`ValueType` in
`src/typing.rs`). Planning's vocabulary shows what is missing:

| Need | Today | Consequence |
| --- | --- | --- |
| An amount of effort or time | `cost`, `budget` integers with no unit; `stale_review_after_days: Option<u64>` converted to hours by hand (`lifecore-prod/src/reminders.rs:44`) | Units live in prose; `cost = "2h"` cannot be written |
| A closed set of words | `Stance.kind` text | "Unknown kind" is a Planning inconsistency (§8.1), not a type error |
| Alternatives with their own fields | `cost` and `horizon` optional, legal only on some kinds | §3.3, §3.4 and §8.1 restate by hand what a tagged value would type |
| A day | `deadline` timestamp | A date must be written as an instant in some zone |
| An event's local day | `at` timestamp, offset discarded on parse | No way to tell which day an event happened on for the user |

String enums existed in an earlier, more elaborate field typing and were cut.
This round restores them in a smaller form.

### 2.2 The rule for time

Time values fall into two domains, and arithmetic stays inside one:

- **Exact.** Instants and elapsed time: `timestamp`, the instant of an
  `offset_timestamp`, and `duration`. Arithmetic is exact and never needs a
  time zone.
- **Civil.** Days and calendar periods: `date`, the local date of an
  `offset_timestamp`, and calendar units such as day, week, and month, which
  are words (an `enum`), not durations. A day is not 24 hours; it is 23 or 25
  on a DST change.

**Store what was observed, never a conversion that a time-zone database would
have to redo.** A past event's instant together with the UTC offset in effect
when it happened is fixed forever, and its local date follows from the two
without any zone lookup. A future wall-clock time in a named zone is not
fixed: the zone's rules can change before it arrives, so precomputing its
instant stores a guess. This round therefore stores no zone names and no
precomputed future instants.

Crossing between domains happens only through an offset carried by a stored
value or an evaluation input, never through a zone name. Section 5.3 uses the
one crossing Planning needs.

jiff already enforces the split: `SignedDuration` refuses `1d`, `P1D`, and
`P1W` ("parsing calendar units … is not supported"), checked against jiff
0.2.38.

### 2.3 New kinds

| Kind | Payload | Rust | Canonical encoding | Accepted input |
| --- | --- | --- | --- | --- |
| `duration` | `Payload::Duration` | `jiff::SignedDuration` | `{kind = "duration", value = "PT1H30M"}` (jiff's `Display`, ISO 8601) | jiff's parser: ISO 8601 (`PT90M`) and the friendly form (`1h 30m`, `90m`); no days or larger |
| `date` | `Payload::Date` | `jiff::civil::Date` | `{kind = "date", value = "2026-11-06"}` | `YYYY-MM-DD` only |
| `offset_timestamp` | `Payload::OffsetTimestamp` | `lifecore::OffsetTimestamp { instant: Timestamp, offset: tz::Offset }` | `{kind = "offset_timestamp", value = "2026-10-29T08:15:00+01:00"}` (`Timestamp::display_with_offset`) | RFC 3339 with a numeric offset |
| `variant` | `Payload::Variant { tag: String, value: Body }` | serde-free; see section 3 | `{kind = "variant", tag = "committed", value = {…}}` | the same; `value` required, `{}` when empty |
| `enum` | a shape over `Payload::Text` | a unit-only Rust enum | native string | the same |

`timestamp` stays, for instants with no local meaning: evaluation times,
machine-recorded instants, and anything authored in UTC on purpose.

Details:

- **`offset_timestamp` rejects `Z` and `-00:00`.** RFC 9557 reads `Z` as
  "UTC known, local offset unknown", which is exactly what this kind exists
  to avoid; `+00:00` is a known zero offset. jiff's `fmt::temporal::Pieces`
  distinguishes the three (`Zulu`, `Numeric`, and `Numeric` with
  `is_negative` for `-00:00`). A bracketed zone annotation
  (`…+01:00[Europe/Berlin]`) is rejected rather than dropped, so no input is
  silently narrowed. An offset with nonzero seconds is rejected: RFC 3339 has
  no syntax for it.
- **Equality is field by field.** Two `offset_timestamp` values with the same
  instant and different offsets are different values, and two `duration`
  inputs `PT120M` and `2h` are the same value after parsing. All new kinds
  derive `Eq` and `Hash`, which the duplicate rule and the analysis recorder
  rely on. jiff's `Span` is deliberately not used: it has no `Eq` (only the
  field-wise `SpanFieldwise`, under which `2h` and `120m` differ) and its
  ordering needs a reference date.
- **Canonical output re-spells input,** as integers already do: `90m` is
  stored and exported as `PT1H30M`. This is the normalization that
  [SPEC §6](../../SPEC.md#6-type-declarations) allows authoring to
  perform before a payload is stored.
- **`enum` is a shape, not a payload kind.** The value is ordinary text, so
  native strings stay native and a change file does not need an envelope for
  `per = "day"`. TypeCheck checks membership. Removing a member while a
  current node uses it is a type error, exactly like removing a field.
- **`variant` is a payload kind,** because a tagged value must carry its tag
  even where no declaration is at hand (export, the client's generic value
  view). Its `value` is a body whose fields are validated against the tag's
  field declarations, with the usual four presence policies.

### 2.4 Declaration words

Following [FILES §2.6](../../FILES.md#26-type-declaration-payloads):

```toml
cost     = ["optional", "duration"]
deadline = ["optional", "date"]
at       = ["mandatory", "offset_timestamp"]
per      = ["mandatory", "enum", ["window", "day", "week"]]
decision = ["mandatory", "variant", { kind = "record", value = {
  committed  = { kind = "record", value = { cost = ["mandatory", "duration"] } },
  deferred   = { kind = "record", value = { horizon = ["mandatory", "integer"] } },
  resumed    = { kind = "record", value = {} },
} }]
```

- `enum` takes one native list of distinct, non-empty strings as its last
  element. An empty list is malformed, since no value could satisfy it.
- `variant` takes one record envelope whose fields are tags, each a record
  envelope of field declarations, as its last element. At least one tag.
- New schema errors: `DuplicateMember`, `EmptyEnum`, `EmptyVariant`. New
  field issues: `NotAMember { value }`, `UnknownTag { tag }`. A variant's
  field is reported under its tag; the path display (`path_text`) needs one
  more part, for example `decision(committed).cost`. Tentative.

`ValueType` gains `Duration`, `Date`, `OffsetTimestamp`,
`Enum { members: BTreeSet<String> }`, and
`Variant { tags: BTreeMap<String, BTreeMap<String, FieldDecl>> }`;
`PayloadKind` gains `Duration`, `Date`, `OffsetTimestamp`, and `Variant`.

### 2.5 Evaluation time

`EvaluationInputs.evaluation_time` becomes `Option<OffsetTimestamp>`. The host
captures the instant and the local offset together, once, before replay, and
reports both. Determinism holds because the offset is an input like the
instant. Nothing in the core or an analyzer reads the system zone.

### 2.6 Left out

- **A wall-clock time in a named zone** ("every Monday at 09:00, Berlin").
  This is the one case where zone-rule changes bite. If a workflow needs it,
  store the civil datetime and the zone name as given, resolve them only at
  evaluation against a time-zone database bundled into the binary (jiff's
  `tzdb-bundle-always`), and count the database version as part of the
  analyzer's implementation version.
- **`Span`, floats, decimals, and null.**
- **Range constraints** (`≥ 0`, `≥ 1`) in declarations. They would grow the
  declaration language for rules that analyzers already state with better
  messages. Planning keeps them.

### 2.7 Client

The client takes the new kinds in two stages (section 6, steps 3a and 3b), so
the core step does not wait for editors.

**Showing them (step 3a).** The contract and every reader learn the new
shapes, and the client shows any value or field shape it has no editor for
read-only, as its canonical JSON. This touches `contract.ts`,
`model/values.ts`, `model/payload.ts`, `model/schema.ts`, and
`model/columns.ts`, with their tests and the recorded fixtures:

- **Values.** As with timestamps today, the client keeps canonical strings
  verbatim and lets Rust validate on check. The raw-JSON reader accepts the
  new envelopes, including `variant`'s third key, `tag`.
- **Declarations.** The declaration reader accepts the new words of section
  2.4, so a type that uses them still has a table, columns, and an
  inspector outline.
- **Authoring.** A form field whose shape has no editor is read-only and
  says so. Its value is written through the draft's raw JSON, which already
  reaches every shape, and a declaration using the new words likewise.
- **Fixtures.** The recorded fixtures include at least one value and one
  declaration of each new kind, so the type-check covers what Rust emits.

The fallback stays after step 3b. A kind added later shows the same way until
it gets an editor, so a core change is complete once the client can show its
values correctly, without being able to edit them.

**Editing them (step 3b),** a UI stage in `editors/ValueEditor.tsx` and
`editors/TypeDeclarationEditor.tsx`, with UI's decisions recorded in
[UI](../../UI.md):

- `duration`: a text input accepting either spelling; the canonical spelling
  comes back from check.
- `date`: `<input type="date">`.
- `offset_timestamp`: a local datetime input that fills the offset from the
  browser at the edited instant (`Date.prototype.getTimezoneOffset`), so
  recording "now" captures the user's offset without asking.
- `enum`: a select over the declared members.
- `variant`: a tag select followed by the tag's fields.

## 3. Typed payload decoding

*Landed in step 4.* The contracts below are owned by the documents section 6
names for that step; where this section and they differ, they hold, and this
section stays as the design's record.

### 3.1 Problem

`lifecore-prod/src/payload.rs` hand-writes `integer`, `optional_integer`,
`timestamp`, `optional_timestamp`, and `text` readers with their own
`PayloadError`, and returns `String` errors through the analyzer trait. Every
analyzer that reads payloads would repeat it, and Planning's canonical
declarations (PLANNING §11) are TOML kept in step with `vocabulary.rs` by
hand.

### 3.2 Design

In the core:

```rust
pub trait FromPayload: Sized {
    // The shape this type decodes, for declaration generation.
    fn shape() -> ValueType;
    fn from_payload(value: &Payload, path: &mut Vec<PathPart>) -> Result<Self, DecodeProblem>;
}

pub trait FromBody: Sized {
    fn fields() -> BTreeMap<String, FieldDecl>;
    fn from_body(body: &Body) -> Result<Self, DecodeError>;
}

pub struct DecodeError { pub path: Vec<PathPart>, pub problem: DecodeProblem }
pub enum DecodeProblem {
    Missing,
    WrongKind { expected: PayloadKind, found: PayloadKind },
    NotAMember { value: String },
    UnknownTag { tag: String },
}

impl Node { pub fn decode<T: FromBody>(&self) -> Result<T, PayloadError> }
// PayloadError = DecodeError + the node's EntryId and type name.
```

| Rust | Payload | Declared as |
| --- | --- | --- |
| `String` | text | `text` |
| `i64` | integer | `integer` |
| `bool` | boolean | `boolean` |
| `Timestamp` | timestamp | `timestamp` |
| `OffsetTimestamp` | offset_timestamp | `offset_timestamp` |
| `SignedDuration` | duration | `duration` |
| `civil::Date` | date | `date` |
| `Vec<T>` | list | `list` + element |
| struct with `#[derive(FromBody)]` | record, or a node's body | `record` |
| unit-only enum with `#[derive(FromPayload)]` | text | `enum` |
| enum with fields, `#[derive(FromPayload)]` | variant | `variant` |
| `Option<T>` as a struct field | absent or `T` | `optional`, overridable to `encouraged` or `deprecated` by attribute |

Rules:

- **Strict about kinds.** A `String` field never accepts a timestamp, and a
  `Timestamp` field never accepts text that looks like one.
- **Lenient about extra fields.** Decoding ignores fields the Rust type does
  not name, because users may add fields to an analyzer's types
  ([PLANNING §3.4]).
  TypeCheck, not decoding, rejects undeclared fields.
- **Names.** Snake case by default, `#[lifecore(rename = "…")]` otherwise.
- **Bodies only.** Decoding reads a body and nothing else. It never looks at
  the node's `ty_name`, its edges, or its declaration. The caller chooses
  which nodes to decode, by whatever names it uses. The type name in
  `PayloadError` only serves the message.
- **A decode type names only the fields the analyzer reads.** A user may
  change or remove any canonical field the analyzer does not read
  ([PLANNING §3.4]), so
  decoding such a field could fail on a payload TypeCheck accepted. The
  canonical declaration therefore comes from a separate type.

A type-level derive, `Declare`, produces the canonical declaration from a
type that lists every canonical field. Field shapes come from
`FromPayload::shape`, so a `Declare` type needs no `FromBody`:

```rust
#[derive(Declare)]
#[lifecore(type = "Stance", outgoing = ["About", "Until"])]
struct StanceDeclaration { decision: Decision, at: OffsetTimestamp, reason: Option<String> }

// What Planning's rules read; `at` and `reason` are free under §3.4.
#[derive(FromBody)]
struct Stance { decision: Decision }

#[derive(FromPayload)]
enum Decision {
    Committed { cost: SignedDuration },
    Deferred { horizon: i64 },
    Waiting { horizon: i64 },
    Resumed {}, Sufficient {}, Abandoned {},
}
```

`StanceDeclaration::declaration()` returns the canonical `Type`. Once
Planning is reworked (section 6, step 6), its bootstrap suggestions are built
from it, and a test compares it with the TOML in PLANNING §11, which stays the
owning text. A second test checks each decode
type against its declaration: every field of `FromBody::fields()` occurs in
the declaration with the same shape. A field the analyzer reads as `T` is
declared `mandatory`, and one read as `Option<T>` may have any policy. Those
are also the field requirements an analyzer's own declaration rule must keep
(PLANNING §3.4), so that half of the table can be derived from the decode
types. Whether Planning derives it is the Planning round's choice (section 6,
step 6).

Edge and successor attributes:

- **They feed `Declare` and nothing else.** Edges carry no payload (section
  1), so decoding has nothing to check on them. TypeCheck checks each current
  edge against the user's stored declarations
  ([SPEC §7.3](../../SPEC.md#73-resulting-view-checks), step 4), and an
  analyzer's own declaration rule checks that the stored declarations still
  permit what it relies on (PLANNING §3.4, `unmet` in
  `lifecore-prod/src/declarations.rs`). Reworked in step 6, that rule
  compares with `declaration()` instead of the hand-written
  `PlanningType::canonical()`.
- **An omitted `incoming`, `outgoing`, or `successors` is the empty set.**

The names in `Declare` (the type name and the labels) are defaults, not
identity. No analyzer configures its names today. Planning owns its names
(PLANNING decision 35). DAG Check and Reconnect take names in their
configuration, but they decode no payloads. Once namespacing
([PLAN §3](../../PLAN.md#3-backlog)) or a configurable analyzer needs other
names, `declaration()` becomes a convenience over a variant that takes a name
mapping. Decoding needs no change, since it never reads names. Rejected: a
per-field `#[lifecore(declare_only)]` attribute that keeps one type for both
roles. It hides which fields are read behind an attribute, while separate
types make the read set what the test above checks.

Both derives go in `lifecore-derive`, today an empty stub outside the
workspace, which it joins in step 4.

### 3.3 Why not serde

Implementing `serde::Deserializer` for `Payload` would hand timestamps,
durations, and dates to visitors as strings, so a `String` field would accept
a duration and a `SignedDuration` field would accept any text that parses as
one. Keeping kinds strict through serde needs the private-magic-name trick the
`toml` crate uses for datetimes, which would be a permanent `HACK:`. A small
trait with a derive is less code than the workaround.

### 3.4 Analyzer errors

`TypedAnalyzer::analyze_inner` returns `Result<_, String>`. Give it an
associated `type Error: std::error::Error`, converted to the `String` of
`ExecutionFailure` at the `dyn Analyzer` boundary, so tests can assert an
analyzer's errors structurally as they already assert its findings.

A `PayloadError` stays an execution failure, never a finding. An analyzer may
assume that payloads fit their declarations, and may fail at a prefix
TypeCheck rejects (PLANNING decision 53). Under section 3.2 a decode type
names only fields the declaration rule keeps, so with TypeCheck configured
that is the only kind of prefix where decoding can fail.

## 4. Indexes

*Sections 4.3 and 4.4 landed in step 2, and section 4.5 in step 5.* Their
contracts are owned as section 8.1 recommends, option (C): the principle, the
core indexes and the templates with the reference definitions of their
queries, the boundary rules, and the admission criteria by
[SPEC §9.1](../../SPEC.md#91-indexes); `Indexing`, `index_templates`, and the
equivalence helper by
[ANALYZERS §1](../../ANALYZERS.md#1-execution-and-normalization); costs and
the declined template candidates by [PERFORMANCE](../../PERFORMANCE.md).
Where those and these sections differ, they hold, and these sections stay as
the design's record.

### 4.1 Where the cost comes from

Measured costs and their causes are in
[PERFORMANCE](../../PERFORMANCE.md). Reading the code adds:

- `LifeCoreList::validate` (`src/graph.rs`) checks each entry's duplicates by
  scanning the prefix before it (`fresh_nodes().any(…)`, `edges().any(…)`),
  so validation is quadratic. It runs on every snapshot load, which the
  server performs per request, and in full again for every staged entry, so
  staging k entries costs about k·n².
- Per replayed prefix, `View::declarations`, `edges_of`, `children`, and
  `fresh_successors` scan the view, and TypeCheck re-parses every current
  declaration: linear work per prefix, quadratic in total.
- Planning's seal asks per-item questions that each scan every current edge:
  items × edges per seal.

### 4.2 Full-check estimates at 1,000 entries a week

Fitted to the 4,000-entry hand measurements (PERFORMANCE §1.2), so beyond
that an order of magnitude only:

- Baseline, TypeCheck and Reconnect: about `1.1e-7 · n²` seconds.
- Planning seals: about `1.2e-9 · n²` seconds each.

The hand-measured planning snapshots seal every 7 entries, so they measured
`1.7e-10 · n³` in total.
A database growing by 1,000 entries a week with one review a week has
`n / 1,000` seals, so the seal term is about `1.2e-12 · n³`.

| Age | Entries | Hand-measured density, one seal per 7 entries | Weekly reviews, one seal per 1,000 entries |
| --- | ---: | ---: | ---: |
| 1 month | 4k | 13 s | 2 s |
| 3 months | 13k | 6.5 min | 21 s |
| 6 months | 26k | 51 min | 1.6 min |
| 1 year | 52k | 7 h | 7.8 min |
| 2 years | 104k | 2.2 days | 42 min |
| 5 years | 260k | ~5 weeks | 8 h |

With weekly reviews the quadratic baseline dominates for the first year and
the seal term takes over in the second. Either way, writes pass the 10-second
busy window of [STORAGE §4](../../STORAGE.md#4-writers) within about
ten weeks (about 9,500 entries). Removing the cubic term lets analyzers stay
plain functions of the view; the quadratic baseline that remains is an
analyzer's per-prefix work, which should track what changed at each prefix
([PERFORMANCE §3](../../PERFORMANCE.md#3-backlog)). "Last N" (section 4.7)
is a last resort.
Snapshot loading (JSON parsing plus validation) is not in these numbers and
grows too.

Step 1 of section 6 added a benchmark of planning-shaped histories at both
seal densities; PERFORMANCE §1.1 records its baseline, which includes
snapshot loading. Its seal terms become measurements once an analyzer that
seals at reviews returns.

### 4.3 Indexes are optional

This principle governs every index in this section, core and template alike,
and would go into [SPEC §9](../../SPEC.md#9-inspection-and-queries) and
[ANALYZERS §1](../../ANALYZERS.md#1-execution-and-normalization):

- **An index caches a query.** Every index has a reference definition: a scan
  of the view that computes the same answer. An index is correct exactly when
  it agrees with its reference at every boundary.
- **Every index is optional.** A view may carry no indexes (a view over an
  ad-hoc slice, `View::without_indexes()`, or a runner run with
  `Indexing::Off`), the core indexes only, or the core indexes and the
  templates its suite declared. Access returns `Option`.
- **Users behave identically with or without them.** `validate`, `View`'s own
  queries, TypeCheck, and every analyzer give the same results whether they
  read an index or compute the answer themselves. "The same" covers
  everything observable: values, order (always append order), errors, the
  position of the first failure, and findings with their ranges. Assuming a
  correct index, an index changes cost and never behaviour.
- **Hence indexes are invisible to identity.** They take no part in finding
  keys, analyzer identity, acceptance, or any serialized output. Building
  them, or not, is the host's choice and needs no record.

The staleness index, which `View::new` requires today, becomes optional
under the same rule. Without it, `View::is_stale` scans for a child.

Access:

```rust
impl<'a> View<'a> {
    pub fn indexes(self) -> Option<Indexes<'a>>;                        // core, section 4.4
    pub fn template(self, t: &IndexTemplate) -> Option<TemplateIndex<'a>>; // section 4.5
    pub fn without_indexes(self) -> View<'a>;
}
```

`Indexes<'a>` and `TemplateIndex<'a>` carry the view's length and expose
only boundary-checked queries. `View`'s own queries (`is_stale`, `children`,
`edges_of`, `fresh_successors`, `declarations`, and the new
`nodes_of_type` and `edges_labelled`) use an index when one is present and
their reference scan otherwise, so most users never see the `Option`. A user
that wants its own fallback takes the `Option` directly. For example, an
analyzer whose own computation is cheaper in context.

Enforcement:

- **Indexes against their references.** For each core index and each
  template, property tests over generated histories compare the index with its
  reference scan at every boundary. Cases include keys that occur only past
  the boundary, and supersessions just before and just after it.
- **Users against themselves.** The runner takes
  `Indexing::{All, Core, Off}`. A test helper runs a history under all three
  and asserts equal `AnalysisOutput`, or equal failures. Every analyzer's
  test suite runs its histories through it. `validate`'s tests run with and
  without indexes and assert the same `Result`.
- **Debug builds** keep the existing assertion that a rebuilt index equals
  the maintained one.

### 4.4 Core indexes

`LifeCoreList` owns derived indexes that generalize the staleness index.
They are rebuilt on load and extended by `stage` with validated entries only:

| Index | Content | Reference scan it replaces | Users |
| --- | --- | --- | --- |
| K1 children | child positions per node | `children`; the first child gives staleness | `is_stale`, `validate` (stale endpoints), `fresh_successors`, Reconnect's siblings |
| K2 incident edges | edge positions per node | `edges_of` | TypeCheck's endpoint checks, Reconnect |
| K3 nodes by type name | node positions per `ty_name` | type filters over `nodes()` | `declarations` (`Type`), TypeCheck re-judging a revised declaration's nodes, Planning's reviews and items |
| K4 edges by label | edge positions per `ty_name` | label filters over `edges()` | DagCheck, Planning |
| K5 node content | hash of `(ty_name, predecessor, payload)` to positions | the duplicate-node scan | `validate` |
| K6 edge triple | `(from, to, ty_name)` to position | the duplicate-edge scan | `validate`; Reconnect's equivalent-edge test |
| K7 root | the root of each node, fixed at append | the walk in `root_of` | `root_of`, lineage templates, Planning's grace |
| K8 parsed declarations | the parsed schema or schema error of each `Type` node | re-parsing in `declarations` | `declarations`, TypeCheck |

Boundary rules, the same the staleness index follows today:

- **Positions.** Each list is in append order, so a view narrows it by
  `partition_point(|&p| p < view.len())`. The view keeps its length and a
  reference to the indexes, which keeps `View::prefix` constant time.
- **Entry-keyed values.** K7 and K8 are values of an entry, readable only
  through an `EntryId` the view holds.
- **Hidden channels.** Indexes are never serialized, and are excluded from
  `Debug`, `PartialEq`, and `Hash` of `LifeCoreList` and `View`. No query
  enumerates keys, so a key that occurs only past the boundary is invisible.
- **Hash collisions.** A collision in K5 still compares complete values, as
  the analysis recorder does.

With K5 and K6, `validate` becomes linear apart from hashing. `stage` then
checks only the appended entry against the existing indexes, which is the
backlog's incremental `basic_validation`. Its unindexed path keeps today's
scans, with the same results.

### 4.5 Index templates

Core indexes answer questions every user asks. An analyzer with a narrower
question declares an **index template**: a parameterized index shape from a
fixed catalogue in the core.

- **Declaring.** An analyzer declares its templates once, at instantiation.
- **Building.** The runner builds them in the same pass and with the same
  machinery as the core indexes.
- **Reading.** The analyzer reads them through `View::template` and receives
  `None` whenever one was not built.

```rust
pub trait Analyzer: Send + Sync {
    // A function of the analyzer's version and configuration only, asked once
    // after instantiation. A request: the runner may build none of them.
    fn index_templates(&self) -> Vec<IndexTemplate> { Vec::new() }
    // version, initialize, analyze unchanged
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IndexTemplate {
    EdgesAt { label: String, end: End, other: Option<BTreeSet<String>> },
    EdgesAtLineage { label: String, end: End, other: Option<BTreeSet<String>> },
}

pub enum End { From, To }
```

Rules:

- **Templates are data.** They are comparable, hashable, and printable, and
  two analyzers that declare equal templates share one index.
- **Fixed for a run.** The set depends only on configuration, never on the
  view or the evaluation inputs.
- **Built per run.** The runner builds them over the run's head view in one
  O(n) pass and narrows them per replayed prefix like the core indexes. They
  are not stored in `LifeCoreList` or the snapshot. Keeping them in
  `LifeCoreList` and extending them on `stage` becomes worthwhile once a
  server cache keeps lists alive across requests.
- **Reference scans ship with them.** Each template has its scan in the core,
  so an analyzer's fallback is one call. The analyzer may write its own
  instead.

Admission criteria for the catalogue:

1. **Fixed at append.** An entry's contribution is fixed when it is
   appended. Keys come from the entry's own content, its endpoints, and their
   roots, all immutable. Nothing is removed or rewritten later, so narrowing
   is a position comparison. Currentness is never stored; queries check it
   through staleness, which is monotone.
2. **Cheap to maintain.** O(1) amortized work per appended entry, besides
   hashing.
3. **Declarative.** Parameters are data, and no analyzer code runs inside the
   core.
4. **Earned.** A named consumer today, with a gain over the core indexes for
   that consumer.

The catalogue:

| Template | Key → ascending positions | Consumer | Gain over core indexes |
| --- | --- | --- | --- |
| `EdgesAt { label, end, other }` | The node at `end` → edges with `label` whose `end` endpoint is the key and whose other endpoint's type is in `other`, if given | Planning: `Enables` and `Helps` sources and targets of an item; the `Stance` and `Done` about a revision; `Until` | K2 lists every edge of a node, so a habit with a year of `Did` events pays for each of them in every question about its `Helps` edges |
| `EdgesAtLineage { label, end, other }` | The root of the node at `end` (by K7) → the same edges, over every revision of the lineage | Planning: `Did` events about any revision of a habit (Shortfall, section 5.2); the rule that each later `About` targets a descendant of an earlier one; Dropped connection's search for an equivalent connection on a current descendant | One lookup instead of walking the lineage and every revision's edges |

Queries on a `TemplateIndex<'a>`, all boundary-checked and in append order:

| Query | Returns | Cost |
| --- | --- | --- |
| `positions(key)` | every listed position before the boundary | proportional to the result |
| `count(key)` | how many such positions | binary search |
| `in_range(key, a..b)` | the positions within a window | binary search plus the result |
| `current(key)` | the positions whose edge is current at the boundary | the length of the key's list |

Considered and not admitted:

| Candidate | Why not now |
| --- | --- |
| `NodesByField { ty, field }`, a payload value → node positions | No consumer among today's analyzers. The first plausible one is collections (documents by path, in the backlog); admit it then. |
| The current members of a type at a boundary (an interval or stabbing index) | K3 plus staleness enumerates every node of the type ever appended. With weekly seals that is small: about 50,000 tasks × 260 seals ≈ 1.3·10⁷ checks over five years. Revisit if a consumer enumerates current members at every prefix. |
| Sums of a field over a position range | No consumer. Budget overrun sums over current stances, which is not a position range. |
| Reachability or transitive closure over labels | Not fixed at append: one edge can change the reachability of many pairs, and history may hold cycles. DagCheck and Planning compute it at the prefixes they judge. |
| A key computed by an analyzer-supplied function | Analyzer state by another name. The core could neither test it against a reference nor share it, and it reopens the per-analyzer state contract rejected in section 4.6. A shape more than one analyzer needs is added to the catalogue instead. |
| Per-entry derived values, such as parsed declarations | Admitted as core index K8, because the parse is core code. There is no analyzer-defined analogue. |

### 4.6 Rejected: state handed from prefix to prefix

An earlier draft let each analyzer return a `State` that the runner handed to
the next prefix as `Option<&State>`. The owner rejected it: it grows the
analyzer contract with a state type, equality of states, rules for handing
them over, and type erasure behind `dyn Analyzer`. With indexes, analyzers
stay functions of the view. What is lost is a seal whose cost tracks the
open items rather than the planning types' history; at weekly reviews that
difference is small (section 4.8).

### 4.7 "Last N", a last resort

Not part of this round, and not planned. It changes what a check means: an
observation that began before the replayed window is cut off, and the full
check must still run somewhere. It is kept only for the case where an
analyzer's per-prefix work cannot be made to track what changed at the
prefix. A check would replay prefixes `[b − N, head]` instead of
`[0, head]`. Indexes are built over the whole head view either way, so they
would need nothing new. Points to settle if it is ever designed:

- **Cut-off observations.** An observation whose range began before `b − N`
  is cut off. Acknowledgment verification (PLAN §2.2) compares against
  observations whose range begins within the committed base, from the same
  replay, so the two designs interact.
- **Choosing N.** Fixed, or reaching back to the last seal, or to the last
  declaration revision.
- **Where the full check runs.** In the background, with its output keyed by
  the exact base, suite, implementations, and inputs.

### 4.8 Planning under indexes

- **Windows.** Reviews come from K3 (`"Review"`), and `window_of` is a binary
  search, so `windows::Reviews` stops scanning.
- **Items.** A seal enumerates its items through K3 and filters out the
  superseded ones by staleness: linear in the planning types' history.
- **Per-item questions.** These read `EdgesAt`, and `Did` counts read
  `EdgesAtLineage` with `in_range` or `positions`. Each costs the matching
  edges only.
- **Total.** A seal costs time linear in the planning history instead of
  items × edges. Replay becomes quadratic with a small constant at weekly
  reviews, `n / 1,000` seals each linear. The benchmark (section 6, step 1)
  measures the constant.

## 5. Planning: input to its next round

PLANNING goes through its own round: a proposal under
`docs/proposals/planning/` written with the `analyzer-proposal` skill, the
owner's feedback, implementation, and feedback again. This section does not
revise PLANNING. It records what the core round makes possible and the
directions already settled in review, so the round starts from them rather
than from a blank page. The proposal owns every rule, and it may still
reject anything here.

Settled directions, each to become a design decision of the proposal:

- Quotas carry a mandatory `per` (`window`, `day`, or `week`).
- Weeks are ISO weeks, Monday first.
- No planning field stays a plain `timestamp`.
- The valuation reads no clock. A `per = "day"` habit ranks in tier 2 by
  the short days of the latest seal, keeping decision 32 (section 8).

The rest of this section is a sketch to measure the core design against. The
owner's decisions in PLANNING stand unless named here.

### 5.1 Vocabulary

| Type | Fields after |
| --- | --- |
| `Goal` | `title` text; `criterion` optional text; `deadline` optional date |
| `Project` | `title` text; `deliverable` optional text; `deadline` optional date |
| `Habit` | `title` text; `quota` integer; `per` enum `window`, `day`, `week` |
| `Concern` | `title` text; `quota` integer; `per` enum `window`, `day`, `week` |
| `Stance` | `decision` variant (section 3.2); `at` offset_timestamp; `reason` optional text |
| `Done`, `Did` | `at` offset_timestamp; `note` optional text |
| `Review` | `budget` duration; `at` offset_timestamp; `note` optional text |

`per` is mandatory so that a quota never silently means windows (decided,
section 8).

What disappears from PLANNING: §3.3's Fields column and the paragraph after
the table, the `Stance` row of §3.4 (replaced by "the `decision` variant
keeps each canonical tag with its canonical fields"), and from §8.1 unknown
`kind`, `committed` without `cost`, `deferred`/`waiting` without `horizon`,
and either field on the wrong kind. `lifecore-prod/src/payload.rs` is
deleted.

What stays in Planning: negative `cost`, `budget`, or `quota`; `horizon`
below 1 or above `max_deferral_horizon`; which item types each tag may be
about. These are domain rules about values, not shapes.

Budget overrun compares sums of `SignedDuration` with checked addition; an
overflow is an inconsistency rather than a panic.

### 5.2 A habit with three check-ins a day

```toml
# Habit: quota = 3, per = "day"
# Did:   at = { kind = "offset_timestamp", value = "2026-10-29T08:15:00+01:00" }
```

Let `L(x)` be the local date of an `offset_timestamp`, from its own offset.
Planning's existing grace rule for a new habit is unchanged.

- **Which day a check-in counts toward:** `L(Did.at)`. A `Did` counts only
  if the prefix holds it, so a check-in appended after the seal that judged
  its day counts toward nothing. Planning may warn about such a backdated
  check-in.
- **Which days a seal judges:** seal k judges the days `D` with
  `H(k − 1) ≤ D < L(Review k.at)`, where `H(k − 1)` is the greatest
  `L(Review j.at)` over the earlier reviews `j < k`. The review's own local
  day is incomplete, so the next seal judges it. Taking the high-water mark
  means a review dated earlier than a previous one judges no day twice and
  leaves no gap. That can happen after flying west, or with an `at` entered
  out of order.
- **What a seal reports:** one sticky Shortfall per habit per seal, naming
  the short days with `{date, have, quota}`. Its subjects are the habit, the
  review, and the counted `Did` events.
- **`per = "week"`:** ISO weeks, Monday first, judged once the sealing
  review's local date falls in a later ISO week (`Date::iso_week_date`).
- **`per = "window"`:** today's rule, counting `Did` events by position in
  the window, and needing no time values at all.
- **Travel:** each check-in uses its own offset, so a day spent flying
  east is short and one spent flying west is long. That is the user's day,
  and no zone database is consulted.

This reverses a stated principle of Planning, that a review's position and
not its `at` is what the rules measure (§3.1). `per = "window"` keeps that
principle, while `per = "day"` and `per = "week"` make the review's `at` and
the events' `at` part of the rules on purpose.

### 5.3 Deadlines

`deadline` is a date. Whether something finished on time is
`L(Done.at) ≤ deadline`, with no zone lookup.

`DeadlineIn` estimates the number of reviews left. It needs the deadline as
an instant, which is the one sanctioned crossing from civil to exact time:
the start of the day after the deadline, at the offset of the latest review's
`at`. The error is at most an offset change, an hour across DST, which is
negligible for an estimate measured in reviews.

### 5.4 Configuration

`stale_review_after_days` becomes a count of local days between
`L(latest review.at)` and the evaluation time's local date, now that
evaluation inputs carry an offset (section 2.5). The alternative is an exact
duration such as `"168h"`; days match how the nudge reads.

## 6. Order of work

Each step lands with its tests, its documents, and, for client-visible
shapes, the fixtures and `contract.ts` together. Only step 3 changes a
client-visible shape; steps 1, 2, 4, and 5 stay inside Rust, with no
client work. A step's brief should say so, because the repository's checks
and the UI stages of PLAN §2 otherwise read as asking for work all the way to
the browser.

1. **Benchmark.** *Landed.* PERFORMANCE §1.1 describes the benchmark and
   records its baseline; the `lifecore-workload` crate generates its
   histories. With Planning shelved, the seal terms of section 4.2 are
   measured only once step 6 brings an analyzer back. A statistical harness
   such as criterion or divan does not fit runs of seconds to minutes; one
   may come later, for micro-benchmarks of index queries. The generated
   histories are valid and raise no finding, which is not yet enough for
   step 2's equivalence helper: step 2 extends the generator with the
   variation an index could get wrong, such as revised declarations, stale
   endpoints, longer revision chains, the other planning edges, and nodes
   that raise findings. The core's property tests in `lifecore` need a small
   generator of their own, because `lifecore` cannot depend on Planning's
   vocabulary.
2. **Optional core indexes and incremental validation.** `Indexing` modes, the
   equivalence helper, and the property tests come first, then the indexes.
   Results are unchanged by construction. Measure again. *Landed.* Its
   contracts now live where section 4 says, and PERFORMANCE §1.1 records the
   measurements before and after. `lifecore-workload` gained
   `varied_history`, whose seed adds the variation step 1 names, and the
   core's property tests in `lifecore` generate histories of their own,
   invalid appends included. Settled while landing: K5 and K6 key by a
   content hash and compare complete values, and K8 is parallel to K3's
   `Type` list; `Declarations` borrows K8's parses instead of copying them;
   `Indexes` also answers `descendants`, `has_child`, `declaration`, and
   `type_declarations`, and `View` gained `nodes_with_content` and
   `edge_between`, the queries of K5 and K6; `select_nodes`, `select_edges`,
   and `relationships` read K3, K4, and K2; `Indexing::Core` builds the core
   indexes for a view that carries none, and `All` equals `Core` until
   templates exist; `prepare` no longer revalidates its base, because every
   `LifeCoreList` is valid, and debug builds compare each staged entry with
   validating the whole history from scratch; TypeCheck reads K8 and K3
   through `declarations` only, since it emits every current finding at every
   prefix and so reads every current node anyway; Reconnect uses K1, K2, and
   K6 and keeps its single scan as its own fallback; DAG Check reads K3 and K4
   when its filter names types or labels.
3. **Value kinds,** in two steps (section 2.7):
   - **3a.** Core, TypeCheck, FILES encodings, evaluation inputs with an
     offset (section 2.5), `contract.ts` and the fixtures, and the client's
     read-only display of the new kinds. Done when the client shows every
     new value and declaration correctly and authors them through raw JSON.
     *Landed.* Its contracts now live in
     [SPEC §6.4](../../SPEC.md#64-value-kinds-and-shapes),
     [FILES §2.1, §2.6, §4](../../FILES.md#21-syntax-and-values),
     [ANALYZERS §1](../../ANALYZERS.md#1-execution-and-normalization),
     [RPC §1](../../RPC.md#1-conventions), [CLIENT §2–§4](../../CLIENT.md#3-values),
     and [UI §5](../../UI.md#5-drafts-and-writes) ("Kinds without an
     editor"), and section 2 here is history for them. Settled while
     landing: jiff 0.2.33, the locked version, already has
     `fmt::temporal::Pieces` and refuses days for `SignedDuration`, so no
     upgrade was needed, but `offset_timestamp` is read by LifeCore's own
     RFC 3339 reader rather than `Pieces`, which rounds a fractional offset
     second, clamps a leap second, drops calendar annotations, and takes
     Temporal spellings RFC 3339 does not have; every bracketed annotation
     is refused, not only zones; `OffsetTimestamp` keeps its fields private
     behind a constructor that refuses offsets with seconds or beyond
     ±23:59, so every value reads back from its text; a date before year
     0000 takes Jiff's six-digit form, its only spelling; `now` fails with
     `local_time_unknown` when the host cannot determine its zone, instead
     of reading it as UTC; three more schema errors name what the
     section left unnamed (`missing_members` and `missing_tags`, like
     `missing_record_fields`, and `empty_member` for the non-empty rule);
     `variant` counts toward the 64-level depth limit like `record`; and a
     tag in a field path is the part `{"tag": …}`, displayed in parentheses.
   - **3b.** The editors, as a UI stage of its own. Nothing else waits for it;
     it may come after steps 4 and 5. *Landed.* Its decisions, including
     those settled while landing, live in
     [UI §5](../../UI.md#5-drafts-and-writes) ("Editors for the value kinds"
     and "Declaration form"), with [CLIENT §3](../../CLIENT.md#3-values);
     the read-only fallback stays for kinds added later.
4. **`FromPayload`, `FromBody`, and `Declare`.** This lands in this round
   (decided, section 8). Its tests use the basic shape of PLANNING's types at
   `59c702c` (section 5.1): records with mandatory and optional fields, a
   unit-only enum such as `per`, and a variant with per-tag fields such as
   `Stance.decision`, each with a `Declare` type and a narrower decode type,
   their declarations compared with TOML the tests hold. These are test
   fixtures, not an analyzer. Porting Planning's payload reads and its error
   type moves to step 6. *Landed.* Decoding and `Declare` now live in
   [SPEC §6.5](../../SPEC.md#65-typed-decoding), the analyzer error type and
   the rule that a `PayloadError` is an execution failure in
   [ANALYZERS §1](../../ANALYZERS.md#1-execution-and-normalization), and the
   fixtures in `src/decode/test/test2.rs`. Settled while landing:
   `from_payload` takes only the value and returns a `DecodeError` whose path
   is relative to it, which a caller extends at the front, instead of
   threading a `&mut Vec<PathPart>`; `Declare` is a trait, and the derives
   are re-exported from `lifecore::decode` beside the traits, with
   `#[lifecore(crate = "…")]` for a renamed dependency; `#[derive(FromBody)]`
   also implements `FromPayload` as a record, and `#[derive(FromPayload)]`
   on a struct is refused in its favour; a unit variant in an enum with
   fields is a tag with no fields; generic types are refused; and the
   subset check of section 3.2 became a recursive fit (a decode type may
   read fewer fields, at any depth, and know more members and tags than
   declared), and is a test helper, not an API, until Planning chooses
   whether to derive its declaration rule from it.
5. **Index templates.** `index_templates` and the two admitted templates
   with their reference scans. Planning reading them moves to step 6.
   *Landed* (decision 11), with tests as their only users. Its contracts now
   live in [SPEC §9.1](../../SPEC.md#index-templates) (the templates, their
   queries and scans, and the choices settled while landing: `other` by the
   other endpoint's own `ty_name`, lineage keys through K7, edges returned in
   append order, exact position keys), in
   [ANALYZERS §1](../../ANALYZERS.md#1-execution-and-normalization)
   (requesting, the per-analyzer grant, and the one-call fallback
   `View::template_or_scan`), and in
   [PERFORMANCE §3, §4](../../PERFORMANCE.md#4-query-costs). Settled while
   landing and not owned there: a test analyzer in `lifecore-workload` reads
   both templates with their scans as fallback over varied histories, so
   `All` and `Core` differ in what they build and not in output, and
   `varied_history` needed no change.
6. **Planning round.** A proposal revising PLANNING on the new kinds,
   starting from section 5 and from PLANNING at `59c702c`. Then the owner's
   feedback, implementation (PLANNING, PLANNING-EXPLAINER, the code, and the
   Plan screen, taken from that commit and reworked), and feedback again. It
   is its own round, after the App and the client take up the new kinds.
7. **Not planned:** a "last N" check (section 4.7), a last resort.

Steps 1 to 5 change no analyzer's results: step 5 cannot, by section 4.3,
and the others touch no analyzer that remains. Step 6 defines Planning's
findings and certificates anew. All of this goes before acknowledgment
(PLAN §2.2), which gives certificates weight.

Housekeeping along the way: in step 4, make `lifecore-derive` a workspace
member. `lifecore-kman` stays as it is.

## 7. Documents that would change

| Document | Change |
| --- | --- |
| SPEC §6, §9 | New shapes; typed decoding (§6.5); a subsection "Indexes": the optional-index principle, boundary rules, reference definitions of core queries and templates, admission criteria (section 8.1) |
| FILES §2.1, §2.6, §4 | Encodings, declaration words, acceptance scenarios for the new kinds |
| ANALYZERS §1 | The analyzer error type, and decode errors as execution failures (section 3.4); `index_templates`; results independent of indexes (section 4.3) |
| RPC, CLIENT §3 | Evaluation inputs with offset; client value handling |
| UI | The read-only fallback (step 3a); editors for the new kinds (step 3b) |
| PERFORMANCE | New measurements; the benchmark; query costs; declined template candidates as backlog (section 8.1) |
| PLANNING, PLANNING-EXPLAINER | Section 5, when step 6 brings them back |
| AGENTS.md | Two invariants: arithmetic stays within the exact or the civil domain and crosses only through a stored or input offset; every index is optional, and its users behave identically with and without it, linking SPEC §9 |

## 8. Decisions and open questions

Decided by the owner:

1. `variant` is a payload kind, not a type-level discriminator.
2. `per` is mandatory.
3. No Planning field stays a plain `timestamp`.
4. The kind is named `offset_timestamp`.
5. Weeks are ISO weeks, Monday first, not configurable.
6. Template indexes are built per run, not kept in `LifeCoreList`.
7. "Last N" is a last resort, not planned work; its interaction with
   acknowledgment is settled only if it is ever designed (section 4.7).
8. Day shortfalls in the valuation keep PLANNING decision 32: the valuation
   reads no clock, and a `per = "day"` habit ranks in tier 2 by the short
   days of the latest seal, a result already judged. Rejected: ranking by
   today's shortfall at the evaluation time's local date whenever an
   evaluation time is given, which is more current but reverses decision 32.
   The Planning round records this as a design decision (section 5).
9. Typed decoding (section 3) lands in this round, without waiting for
   Planning's rework, tested on types of PLANNING's basic shape (section 6,
   step 4). It has been planned since the start, as the long-standing
   `lifecore-derive` stub shows.
10. The index catalogue is owned as option (C) of section 8.1, settled when
    step 2 landed: SPEC §9.1, ANALYZERS §1, PERFORMANCE, and one AGENTS.md
    invariant. Step 5 adds the templates to the same places.
11. Step 5 lands without Planning. Planning was the only consumer of the two
    templates, and admission criterion 4 of section 4.5 asks for a named
    consumer today. They land now, with tests as their only users and
    Planning's rework (step 6) named in advance as their consumer, instead of
    moving into step 6.

No question is open.

### 8.1 Owning the index catalogue

*Settled as (C) when step 2 landed (decision 10).*

The "catalogue" is five kinds of content, and they have different readers:

| Content | Kind | Reader | Asks |
| --- | --- | --- | --- |
| The principle: indexes are optional caches of reference queries, users behave identically, indexes are invisible to identity | Correctness rule | Everyone touching the core | Can my change observe an index? |
| Boundary rules for indexes | Correctness rule | Core authors | How must an index narrow? |
| Each core query and template: key, contents, order, query results | Semantics | Analyzer authors, including third-party `dyn Analyzer` | What does `EdgesAt` return at this boundary? |
| The analyzer's side: declaring at instantiation, `None` handling, the equivalence test | Analyzer obligation | Analyzer authors | What must my analyzer do? |
| Admission criteria, declined candidates, costs | Policy, backlog, measurements | Whoever extends the catalogue | Should this be a template, and what does it cost? |

[AGENTS.md](../../../AGENTS.md) asks for one owning document per contract, with
links elsewhere. The options:

- **(A) PERFORMANCE owns it all.** Indexes exist for cost, and PERFORMANCE §3
  already lists them as backlog. But PERFORMANCE is a status document of
  measurements and remaining work. The optional-index principle is a
  correctness rule about what users may observe, the same kind of rule as the
  boundary invariants SPEC §9 owns for the staleness index. Putting it in
  PERFORMANCE hides a correctness rule where reviewers of a change do not
  look, and makes PERFORMANCE the only status document with normative
  content.
- **(B) SPEC §9 owns it all.** SPEC §9 already owns the staleness index and
  the rule that a prefix-bounded value reveals nothing through any channel,
  and each template is a query, which §9 defines. But the declined
  candidates and the measurements are backlog and status. SPEC would also
  hold the analyzer's obligations, which ANALYZERS owns for everything else
  an analyzer must do.
- **(C) Split by kind of content.** Recommended:
  - **SPEC §9, a new subsection "Indexes":** the principle, the boundary
    rules, the reference definition of each core query and each template,
    and the admission criteria. Criteria 1 to 3 (fixed at append, O(1) per
    append, declarative) are what make the principle cheap to uphold, so
    they belong with it. Criterion 4 (a named consumer) is the rule for
    extending a closed list, and sits beside the list.
  - **ANALYZERS §1:** `index_templates`, declared once at instantiation as a
    function of version and configuration; a request the runner may decline;
    identical behaviour with `None`; every analyzer's histories run through
    the equivalence helper. It links SPEC §9 for semantics.
  - **PERFORMANCE:** the cost of each query, the measurements, and the
    declined candidates as backlog items, each naming the consumer or the
    blocker it waits for. Admitting one moves its definition into SPEC §9.
  - **AGENTS.md, Invariants:** one bullet stating that indexes are optional
    and users behave identically, linking SPEC §9.
  - **Rust doc comments** on `IndexTemplate` and the query types link SPEC §9
    and do not restate it. The core's reference scans are the executable form
    of the definitions, and the property tests hold the two to each other.
- **(D) A new INDEXES document.** Everything in one place, but the principle
  would still need a foothold in SPEC §9, because it is one of the boundary
  invariants. With two templates, a new document adds an entry to the
  AGENTS.md map for little gain. Revisit if the catalogue grows past a
  handful.

Under (C), each reader has one place to look:

| Question | Document |
| --- | --- |
| What does `EdgesAt` return at a boundary? | SPEC §9 |
| Can this code path behave differently with an index? | SPEC §9, via AGENTS.md |
| How does my analyzer declare and use templates? | ANALYZERS §1 |
| Should this new shape be a template? | SPEC §9 (criteria), PERFORMANCE §3 (declined, and why) |
| What does a query cost? | PERFORMANCE |

Section 7's table changes accordingly: SPEC §9 gains the catalogue, and
PERFORMANCE keeps only costs and the declined candidates.

[PLANNING §3.4]: https://github.com/RezaOnGithub/lifecore/blob/59c702c/docs/PLANNING.md#34-amending-the-declarations
