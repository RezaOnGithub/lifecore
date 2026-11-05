# LifeCore Lite Authoring Files

Status: shared authoring contract for JSON/TOML change files, browser drafts,
and the application service: the value encoding, proposals, plan IDs,
references, and operations. [SPEC](SPEC.md) takes precedence for graph
identity, history, typing, and transaction validity. Encodings follow the
implementation; when one changes, update this document with it.

## 1. Purpose and model

JSON and TOML change files serialize the same shared Rust `Proposal` used by
application requests. A browser draft, a handwritten file, and an explicitly
selected analyzer suggestion all reach the same checking and application path.
File parsing is a Serde codec, not a separate authoring interpreter.

A proposal describes ordered additions to history. Drafts are mutable; applying
one stores complete immutable entries. Later edits to a source file must not
change stored payloads or reinterpret committed entries. Local operation names
are authoring handles, not substitutes for exact identities in stored edges.

A future [document-collection planner](backlog/COLLECTIONS.md) can produce
this same proposal type from intended document content and relationships.

## 2. JSON and TOML change files

### 2.1 Syntax and values

Both formats use UTF-8 and deserialize directly into the same Serde types. TOML
input uses [TOML 1.1 syntax](https://toml.io/en/v1.1.0), which the `toml` crate
parses and which accepts every TOML 1.0 document; JSON uses ordinary JSON syntax.
Arrays preserve operation order. Object/table key order does not affect graph
semantics. Files cannot execute code, interpolate environment variables, or fetch
includes. Unknown control fields and value tags are errors.
Payload field names remain subject to contextual schema validation. A payload or
record that repeats a field name is malformed wherever it appears, including
stored snapshots; the last value never silently wins.

Use one payload representation across files and RPC:

| Core value | JSON/TOML representation |
| --- | --- |
| Signed 64-bit integer | Object/table with `kind = "integer"` and decimal string `value`; bare `i64` integers are accepted as input shorthand. |
| Text | Native string, even when its text resembles a timestamp or reference. |
| List | Native array whose elements recursively use this encoding. |
| Boolean | Object/table with `kind = "boolean"` and boolean `value`. |
| Timestamp | Object/table with `kind = "timestamp"` and Jiff's Serde string as `value`. |
| Duration | Object/table with `kind = "duration"` and an ISO 8601 duration string as `value`, such as `"PT1H30M"`. |
| Date | Object/table with `kind = "date"` and a `YYYY-MM-DD` string as `value`. |
| Offset timestamp | Object/table with `kind = "offset_timestamp"` and an RFC 3339 string with a numeric offset as `value`, such as `"2026-10-29T08:15:00+01:00"`. |
| Record | Object/table with `kind = "record"` and an object/table of recursively encoded fields as `value`. |
| Variant | Object/table with `kind = "variant"`, the tag string as `tag`, and an object/table of recursively encoded fields as `value`, `{}` when empty. |

Node payloads are typed maps of field names to values. Their outer map needs no
record wrapper; record-valued fields do. Similarly, known proposal/control
structs use ordinary objects/tables. Wrapping records prevents their field names
from colliding with the tagged value envelope. Canonical output uses native
strings and arrays; all other payload kinds are wrapped. An envelope has
exactly `kind` and `value`, and a variant also `tag`; `tag` on any other kind
is an unknown field. An `enum` value is a native string: `enum` is a field
shape over text ([section 2.6](#26-type-declaration-payloads)), not a payload
kind. [SPEC §6.4](SPEC.md#64-value-kinds-and-shapes) says what each kind
means.

For example, these node payloads represent the same values:

```toml
[payload]
title = "Submit report"
priority = { kind = "integer", value = "2" }
labels = ["work", "writing"]
deadline = { kind = "timestamp", value = "2026-09-30T17:00:00Z" }
completed = { kind = "boolean", value = false }
details = { kind = "record", value = { owner = "Teto" } }
effort = { kind = "duration", value = "PT1H30M" }
due_day = { kind = "date", value = "2026-09-30" }
started = { kind = "offset_timestamp", value = "2026-09-29T08:15:00+02:00" }
per = "week"
decision = { kind = "variant", tag = "committed", value = { cost = { kind = "duration", value = "PT2H" } } }
```

```json
{
  "payload": {
    "title": "Submit report",
    "priority": { "kind": "integer", "value": "2" },
    "labels": ["work", "writing"],
    "deadline": { "kind": "timestamp", "value": "2026-09-30T17:00:00Z" },
    "completed": { "kind": "boolean", "value": false },
    "details": { "kind": "record", "value": { "owner": "Teto" } },
    "effort": { "kind": "duration", "value": "PT1H30M" },
    "due_day": { "kind": "date", "value": "2026-09-30" },
    "started": { "kind": "offset_timestamp", "value": "2026-09-29T08:15:00+02:00" },
    "per": "week",
    "decision": { "kind": "variant", "tag": "committed",
                  "value": { "cost": { "kind": "duration", "value": "PT2H" } } }
  }
}
```

Every enumeration that the snapshot, analysis output, RPC, or change files carry
spells its names in lowercase snake case, such as `node`, `revise`,
`suggestion`, or `false_positive`, and one whose alternatives carry members is
tagged by a `kind` member beside them, as payload envelopes are. Section 2.5
gives the entry shapes and
[ANALYZERS §2](ANALYZERS.md#2-finding-identity-and-observations) the finding
shapes.

Empty lists and records retain their distinct kinds. Floats and null are not
payload variants. Absent optional control fields are omitted when exporting
both formats; JSON null is not an alternative payload encoding. No coercion or
Unicode normalization is implicit. Serde's untagged input alternatives and
tagged variants can express this common model; see its
[enum representation guide](https://serde.rs/enum-representations.html).

#### Integer representation

Always serialize an integer payload as an envelope in both formats, including
integers nested in lists and records:

```toml
count = { kind = "integer", value = "9223372036854775807" }
```

```json
{"count":{"kind":"integer","value":"9223372036854775807"}}
```

Deserialize either that envelope or a bare integer shorthand, such as TOML
`count = 9223372036854775807` or JSON `{"count":9223372036854775807}`. Both formats
accept the shorthand through the same Serde input type; exports always use the
envelope. Decimal strings use exactly the integer's decimal display spelling:
`0`, or an optional `-` followed by ASCII digits without a leading zero, parsing
to `i64`. A leading `+`, leading zeros, `-0`, whitespace, and other spellings are
malformed, so each value has one spelling. Malformed values and overflow in
either input form are errors. Canonical output uses that display spelling.
Bare `-0` is rejected in both formats, as JSON reads it as a float. Outside an
envelope, `"42"` remains text. Floating-point and exponent syntax do not stand in
for integer payloads.

Use a canonical `Serialize` implementation and a deserialization helper accepting
tagged values plus shorthand; no format detection is needed. The decimal-string
field needs a small `Display`/`FromStr` adapter or equivalent Serde implementation.
React keeps these strings intact and can use ordinary JSON parsing for canonical
responses. Raw JSON/TOML file imports go to the Rust codec before any JavaScript
number parsing; otherwise a bare large integer may already have been rounded.

Known potentially large control integers, including `EntryId.position`,
`ViewId.end_exclusive`, and entry counts, use decimal strings directly, without
payload wrappers. Their statically known types define range validation. Small
bounded control values such as protocol error codes, source lines and columns,
and operation indexes within one plan remain numbers. This applies consistently
to application requests/results and files; it does not change the integer types
used by the in-memory graph. These fields accept only decimal strings, with the
same syntax as integer envelopes; bare numbers are not shorthand for them.

#### Timestamp representation

Use `jiff::Timestamp`'s ordinary Serde string representation inside the timestamp
wrapper in both formats. Quoted strings outside that wrapper remain text.
Native TOML dates, times, and datetimes are not timestamp payload syntax, and
no proposal field takes one: they are rejected wherever they appear, including
as a whole payload or an envelope's `value`, with their source location.
Parsing, accepted range, precision, and formatting follow Jiff's Serde behavior;
there is no application-specific datetime normalization or narrower TOML profile.
This adopts Jiff's parser semantics, including any normalization it performs,
rather than promising preservation of input offset spelling or leap-second text.
A known control field of timestamp type uses the Serde string directly,
without a payload wrapper.

Export must preserve each represented core instant and its nanoseconds when
read back by the same shared codec. The React client must retain full timestamp
strings and decimal-string integers for the entire `i64` domain; see
[the client contract](CLIENT.md#3-values). Jiff documents its
[default timestamp serialization](https://docs.rs/jiff/latest/jiff/fmt/serde/timestamp/index.html).

#### Duration, date, and offset timestamp representation

Each is a string inside its envelope in both formats; native TOML dates and
datetimes are refused as above. Parsing normalizes, and canonical output
re-spells the input, as it does for integers: `90m` is stored and exported as
`PT1H30M`.

- **Duration** (`jiff::SignedDuration`): Jiff's parser, either ISO 8601
  (`PT90M`, `-PT1H`, `PT0.5S`) or its friendly form (`1h 30m`, `90m`,
  `1h ago`). Days, weeks, months, and years are refused in either form
  (`1d`, `P1D`, `P1W`), as are empty and padded text. Output is Jiff's
  `Display`, ISO 8601 with hours as the largest unit: `PT1H30M`, `PT0S`,
  `-PT2H`, `PT0.000000001S`.
- **Date** (`jiff::civil::Date`): RFC 3339's `full-date`, exactly
  `YYYY-MM-DD`, a valid day of the years `0000` to `9999`. A year before
  `0000`, which four digits cannot write, takes the form Jiff prints for it, a
  minus sign and six digits (`-000001-12-31`), down to `-009999`; that form
  is refused for any other year. Every date therefore has exactly one accepted
  spelling, the one exported. A plus sign, the basic form `20261106`, and a
  datetime are refused, though Jiff would read them.
- **Offset timestamp** (`lifecore::OffsetTimestamp`): RFC 3339's `date-time`
  with a numeric offset, read from the text itself:
  `YYYY-MM-DDTHH:MM:SS[.fraction]±HH:MM`. The date follows the date rule
  above, so a year before `0000` takes the six-digit form. The separator is
  `T` or `t`, which RFC 3339's grammar allows, but not the space its prose
  permits applications to use; seconds are required; a fraction has one to
  nine digits after a full stop; the offset is at most `±23:59`. Refused,
  since each would be silently narrowed or is outside RFC 3339: `Z` and
  `-00:00`, which RFC 9557 reads as "UTC known, local offset unknown", what
  this kind exists to avoid (`+00:00` is a known zero offset); an offset with
  seconds or a fraction of a second, rather than rounded; a leap second
  (`:60`), rather than clamped to `:59`; a bracketed annotation, a zone such
  as `[Europe/Berlin]` or any other, rather than dropped; and the other
  spellings of Jiff's Temporal parser, such as the basic format, a signed
  six-digit year for a year four digits can write, an hour-only or
  minute-only time, a comma before the fraction, or `+01` for `+01:00`.
  Output is Jiff's `Timestamp::display_with_offset`: the local date and time,
  a fraction only when nonzero, and the offset to the minute, as in
  `2026-10-29T08:15:00.5+01:00`, so the fraction's trailing zeros and a
  lowercase `t` are re-spelled. Every value prints as text that reads back to
  it. Known fields such as `EvaluationInputs.evaluation_time` use the same
  string without an envelope.

#### Variant representation

`{kind = "variant", tag = "committed", value = {…}}`: the three keys in any
order, `tag` a string (any string, the empty one included), and `value` an
object/table of fields encoded like a node payload, required even when empty.
A repeated field name in it is malformed, as everywhere. Which tags exist and
what their bodies hold is the declaration's business
([section 2.6](#26-type-declaration-payloads)), not the codec's.

### 2.2 Envelope and ordered entries

`Proposal` has these fields in both JSON and TOML:

| Field | Required | Meaning |
| --- | --- | --- |
| `plan_id` | No | Token identifying the database, exact graph/suite base, and proposal. |
| `acknowledges` | No | Complete `FindingKey` of a suggestion these entries implement; see [SUGGESTIONS](SUGGESTIONS.md). |
| `entries` | Yes | Ordered array of proposed entries; may be empty. |

There is one value representation, with no separate value-profile selector.
Optional fields are omitted when absent; JSON `null` is rejected rather than
read as absence. There are no separate `database` or `base` fields. The host
selects the database through the [path rule](STORAGE.md#1-database-selection). A
supplied token asserts that this database and its current base match.
`acknowledges` is verified by replay and, on success, dismisses that finding
atomically with the graph changes. There is no general `dismissals` list.
Until verification is built, a proposal carrying `acknowledges` fails with
`acknowledgment_unsupported` ([RPC §3](RPC.md#3-errors),
[PLAN §2.2](PLAN.md#22-suggestions-and-acknowledgment)).

This example assumes compatible type declarations and an existing current node
with the illustrated exact identity:

```toml
plan_id = "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc"

[[entries]]
name = "task_done"
kind = "node"
ty_name = "Task"
predecessor = { existing = { origin = "11111111-1111-4111-8111-111111111111", position = "42" } }
intent = "revise"
payload = { title = "Write proposal", status = "done" }

[[entries]]
name = "explanation"
kind = "node"
ty_name = "Note"
payload = { title = "Why we changed direction", body = "The original approach depended on unavailable equipment." }

[[entries]]
name = "explanation_link"
kind = "edge"
ty_name = "Explains"
from = { added = "explanation" }
to = { added = "task_done" }
```

Each element describes exactly one append. Table key order does not affect
semantics; array order does. Authors cannot claim IDs for new entries. Successful
application returns an operation-name-to-entry-ID mapping. Independent previews
may allocate different provisional identities; the client must use the actual
application result. Returning that mapping does not require a durable receipt.

#### Optional structured plan identity

The token encoding is
`lc1:<database-uuid>:<graph-revision-uuid>:<proposal-uuid>`.
The angle-bracket terms name three UUID components, each in lowercase hyphenated
form, and `lc1` is the token version. Callers obtain a complete token from the
shared mint operation and carry it verbatim.

- The database UUID is the graph's persistent identity, independent of filename.
- The graph revision UUID names the stored graph/suite generation
  ([STORAGE §3](STORAGE.md#3-graph-revision)). Any graph or suite change
  rotates it, including equal-count divergence and suite-only changes, so old
  tokens go stale; dismissal edits, no-op writes, and minting do not.
- The proposal UUID distinguishes proposals against the same database/base.
  Mint a fresh identity for changed operations.

Verify the token's database/revision against the loaded snapshot, then retain
the core's exact database, entry count/content, and ordered suite comparisons
for prepared commit and publication.

Minting is available through HTTP/CLI RPC and the `plan-id` convenience command.
Browser-generated proposals, including exported suggestions, include a token;
other integrations are encouraged to include one. Minting neither appends,
reserves a base, nor dismisses a finding. Malformed or unsupported tokens are
errors, never treated as an absent field.

| Proposal | `check` and `apply` starting state |
| --- | --- |
| Has `plan_id` | Require the selected database and current graph/suite state to match. |
| Omits `plan_id` | Use the selected database's current state for that invocation. |

Either form may carry `acknowledges`; the token does not affect acknowledgment.

A one-off addition can omit the token:

```toml
[[entries]]
name = "task"
kind = "node"
ty_name = "Task"
payload = { title = "Buy groceries", status = "open" }
```

All forms receive complete validation. Each application has a concrete base
protected through publication. A token does not certify an earlier check, freeze
analyzer versions or evaluation time, or recognize a completed retry. Durable
retry recognition is [deferred](backlog/RECEIPTS.md).

### 2.3 References and names

Proposal references use the same Serde enum representation in both formats:

- `{ existing = { origin = "<uuid>", position = "42" } }` names a complete existing
  `EntryId`. The UUID and position together establish identity; the position alone
  is insufficient. It must be present in the selected starting state.
- `{ added = "task_done" }` names a node introduced by an earlier operation in
  this proposal. The application lowers it to `Reference::Added(operation_index)`.
- Every operation has a unique `name` matching `[A-Za-z_][A-Za-z0-9_-]*`. Edge
  names identify results but cannot serve as node endpoints. No forward aliases.
- Predecessors and endpoints must resolve to nodes. Ordinary payload strings are
  literal; they never perform reference lookup.

Search/selection helpers must materialize exact identities. There is no implicit
lookup by title, root, path, or latest descendant. Collection URI spellings and
context are a separate [backlogged](backlog/COLLECTIONS.md#4-destinations)
authoring concern.

### 2.4 Node and edge operations

A node entry requires `name`, `kind = "node"`, `ty_name`, and complete `payload`.
It may also have `predecessor` and `intent`:

- No predecessor creates a root, and `intent` must be absent.
- A predecessor requires `intent = "revise"` or `intent = "branch"`.
- `revise` requires the predecessor to be current immediately before the append,
  including earlier operations in this proposal.
- `branch` permits a superseded predecessor; it still requires the contextual
  transition permission. It is also valid for a current predecessor.

Omitting a field removes it from the successor payload, subject to schema rules.
Patch/default/connection-preservation helpers must expand to complete nodes and
explicit edges before checking. The format has no implicit reconnection. Draft
helpers preserve exact endpoints, require explicit split selection, and expose
intermediate views.

An edge requires `name`, `kind = "edge"`, `ty_name`, `from`, and `to`, with no payload
or predecessor. Endpoint permissions and freshness are checked at its position.
Type declarations are ordinary node proposals with `ty_name = "Type"` and no
bootstrap exemption. No delete, retarget, or in-place update operation exists.
Proposals do not change the analyzer suite; that is the deliberate `add-analyzer`
operation in [INTERFACES](INTERFACES.md#2-cli-surface).
Explicit duplicates fail rather than being silently skipped.

### 2.5 Entry-shaped types

Several types describe "an entry" at different stages. They differ in how they
refer to other entries and in what they may contain, so none can stand in for
another. All of them serialize in the vocabulary of
[section 2.1](#21-syntax-and-values): `kind` (`node` or `edge`), `ty_name`, and
either `predecessor` (omitted for a root), `intent` where an operation carries
one, and `payload`, or `from` and `to`; references are `existing` or `added`.
They differ only in the members of their stage, shown below.

| Type | Stage | Refers to other entries by | Why it is its own type |
| --- | --- | --- | --- |
| `ProposalEntry` (`lifecore-app`) | Authoring and transport: files, RPC, drafts | Operation name (`added = "name"`) or exact ID | Carries author-facing names and this document's vocabulary. Contains only what an author may write: no suite changes. Lowering checks names and resolves them to operation indexes, producing `EntryChange`. |
| `transaction::EntryChange` | Core staging input for node and edge operations | Operation index (`Reference::Added(n)`, serialized `{ added = n }` with a number) or exact ID | The core API, free of names. Has no identity yet; `prepare` assigns IDs. Serialized like a `ProposalEntry` without `name`, as in a suggestion's actions. A lowered proposal and a suggestion's actions are both `Vec<EntryChange>`, so [acknowledgment](SUGGESTIONS.md#3-verification-by-replay) compares by equality, and neither can change the suite. `transaction::Change` wraps it beside `AddAnalyzer`, which only `init` and `add-analyzer` build; no file or payload carries a `Change`. |
| `Entry` (`Node`, `Edge`) | Stored history | Exact ID only | An immutable record with its own assigned `EntryId` in `id`, as stored in the snapshot. |
| `EntryRecord` (`lifecore-app`) | Read results (`entries.list`, `entries.get`) | Exact ID | The stored `Entry` with `current`, its currentness in the selected view, beside its members. |

### 2.6 Type declaration payloads

A type declaration is an ordinary node proposal with `ty_name = "Type"`
([SPEC §6](SPEC.md#6-type-declarations)). Its payload has five fields; all but
`name` are optional and empty when omitted:

| Field | Value |
| --- | --- |
| `name` | The logical type name, a string. |
| `fields` | A record envelope whose fields are field declarations, described below. |
| `outgoing` | List of permitted outgoing edge labels. |
| `incoming` | List of permitted incoming edge labels. |
| `successors` | List of type names permitted for successors besides the type itself. |

A field declaration is a native list of words: the presence policy
(`mandatory`, `encouraged`, `deprecated`, or `optional`) followed by the shape.
A shape is `text`, `integer`, `boolean`, `timestamp`, `duration`, `date`, or
`offset_timestamp`; `list` followed by the element shape; `record` followed by
one record envelope whose fields are themselves field declarations; `enum`
followed by one native list of its members; or `variant` followed by one
record envelope whose fields are its tags, each a record envelope of field
declarations. The element after `record`, `enum`, or `variant` is the last.
Scalar and list shapes therefore need no envelopes; only `fields` itself,
being a record-valued payload field, keeps the envelope that
[section 2.1](#21-syntax-and-values) requires:

```toml
[[entries]]
name = "task_type"
kind = "node"
ty_name = "Type"

[entries.payload]
name = "Task"
fields = { kind = "record", value = { title = ["mandatory", "text"], tags = ["optional", "list", "text"] } }
outgoing = ["DependsOn"]
incoming = ["DependsOn"]
successors = ["Note"]
```

A list of lists is `["optional", "list", "list", "integer"]`. A record-shaped
field is `["optional", "record", { kind = "record", value = { owner = ["mandatory", "text"] } }]`.
The new kinds, as fields of one declaration:

```toml
cost     = ["optional", "duration"]
deadline = ["optional", "date"]
at       = ["mandatory", "offset_timestamp"]
per      = ["mandatory", "enum", ["window", "day", "week"]]
decision = ["mandatory", "variant", { kind = "record", value = {
  committed = { kind = "record", value = { cost = ["mandatory", "duration"] } },
  deferred  = { kind = "record", value = { horizon = ["mandatory", "integer"] } },
  resumed   = { kind = "record", value = {} },
} }]
```

An enum's members are distinct, non-empty strings, at least one; they are a
set, so canonical output lists them in code point order. A variant has at
least one tag; a tag may declare no fields.

Malformed declarations are reported with the path of the offending element,
using the list position for words: a missing presence or shape word (reported at
the position where it was expected), an unknown word, a non-string where a word
belongs, `record` followed by nothing, by something other than a record
envelope, or by further elements, a repeated permission, or more than 64 nested
`list`, `record`, and `variant` words, which bounds recursion on hostile input.
After `enum`, a missing list (`missing_members`), something else than a list,
an empty one (`empty_enum`), a member that is not a string, is empty
(`empty_member`), or repeats an earlier one (`duplicate_member`, at the
repeat). After `variant`, a missing envelope (`missing_tags`), something else
than a record envelope, an empty one (`empty_variant`), or a tag whose value
is not a record envelope of field declarations, reported under the tag's
name (`fields.decision[2].committed`). Tags, like fields, are read in code
point order, so the first malformed one in that order is reported. Canonical
output writes all five keys, with empty lists and an empty `fields` record where
nothing is declared. Forms in the React client generate exactly this encoding,
so a declaration written by hand and one built in the browser are the same
declaration.

## 3. Checking and applying

The application service owns proposal decoding and export, plan-ID minting,
checking, and application; the CLI commands
([INTERFACES §2](INTERFACES.md#2-cli-surface)), CLI `rpc`, and HTTP call the
same operations. Revision intent is validated by `transaction::prepare` for
every plan; source validation for explicit suggestions must remain consistent
with `analysis::suggestion::prepare_suggestion`. `export` converts any proposal
between JSON and TOML, preserving `plan_id` and `acknowledges` verbatim; it
neither refreshes a base nor checks acceptance.

`check` does not append, rewrite the source, or mint a missing token. It freezes
the decoded proposal, verifies any token/base, and evaluates every historical
and proposed prefix under the selected suite and inputs. Reports include the
selected database and exact base, ordered operations, exact references,
provisional mappings, differences, relevant operation/field locations, and
complete analysis context. Structurally
valid proposals with inconsistencies should retain their full analysis output
for preview, including advisories, but must not be committable. Structural and
analyzer execution errors remain distinct from emitted findings.
Include the verified prospective acknowledgment, if any, while saving neither
effect.

`EvaluationInputs` defaults to absent time. If a user explicitly chooses current
time, the host captures one instant and the local offset in effect at it,
together, before replay and reports both
([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)). Neither runner
nor analyzer reads a clock implicitly. Freeze inputs across initialization and
every prefix. Record exact versions, original configuration strings, suite order,
inputs, and the evaluated view. A previous check never authorizes skipping a new
application check. Display filtering and dismissal never affect acceptance.

`apply` freezes its input and always checks anew. A supplied token must match;
without one, it checks the then-current stored state, which may differ from an
earlier standalone preview. Protect the concrete base and evaluation context
through publication. Check exact original entries/count and ordered suite, not
just matching boundaries. Return the actual analysis context and assigned IDs.

When the proposal carries `acknowledges`, verify it against the same replay and
publish the acknowledgment in the same snapshot as the graph changes, following
[SUGGESTIONS](SUGGESTIONS.md#4-atomic-application).

The complete plan publishes atomically after every prefix passes, under writer
exclusion from exact base verification through synchronization
([STORAGE §4](STORAGE.md#4-writers)). Intermediate committed boundaries remain
inspectable. Never silently refresh a
token, reconnect edges, or resume a partially committed suffix. After uncertain
acknowledgment, support inspection without automatically retrying the mutation.

## 4. Acceptance scenarios

These are behavioral acceptance scenarios for an implementation, not additional
exceptions to the core contract. [STORAGE §6](STORAGE.md#6-acceptance-scenarios)
lists those for storage and publication,
[SUGGESTIONS §7](SUGGESTIONS.md#7-acceptance-scenarios) those for export and
acknowledgment, and the [backlog](backlog/) those for collections and receipts.

| Scenario | Required outcome |
| --- | --- |
| Equivalent JSON and TOML proposals | Decode to the same shared values, operation order, and references. |
| Nested lists, wrapped records, booleans, or empty collections | Round-trip without changing value kinds or confusing record fields with tags. |
| Wrapped timestamp is imported/exported | Use Jiff Serde behavior and preserve represented instants and nanoseconds; test its accepted edge cases without a separate datetime profile. |
| Plain timestamp-like text or a native TOML datetime appears | Keep plain strings as text; reject unwrapped native datetime payload syntax. |
| A duration is written `PT90M`, `90m`, or `1h 30m` | Decode to one value; export `PT1H30M`. Two roots differing only in that spelling are duplicates. |
| A duration names days or larger (`1d`, `P1D`, `P1W`) | Reject; a day is a civil unit, not elapsed time. |
| A date is not exactly `YYYY-MM-DD` (`20261106`, `+002026-11-06`, a datetime) or is no valid day | Reject; a year before `0000` is accepted only as `-YYYYYY`, as exported. |
| A date, duration, or offset timestamp built in Rust, such as in a suggestion, at the edges of its range | Export a spelling that reads back to the same value. |
| An offset timestamp ends in `Z` or `-00:00`, carries a bracketed annotation, has an offset with seconds or a fraction, or a leap second | Reject; nothing is dropped, rounded, clamped, or narrowed. `+00:00` is accepted. |
| An offset timestamp uses a Temporal spelling outside RFC 3339 (basic format, an extended year, no seconds, a comma fraction, a space for `T`) | Reject. |
| One instant is written at two offsets | Two different values, each exported at its own offset. |
| A variant envelope lacks `tag` or `value`, or another envelope carries `tag` | Reject. An empty body is `value = {}`. |
| A declaration's enum is empty, repeats or empties a member, or a variant declares no tag | Reject the declaration as malformed, at the offending element. |
| An enum member or a variant tag is removed while a current node uses it | TypeCheck reports the node (`NotAMember`, `UnknownTag`) from that prefix on; configured, it rejects the revision. |
| A variant's body misses a mandatory field of its tag | Report the field under the tag, as `decision(committed).cost`. |
| An integer is outside JavaScript's exact-number range | Preserve the exact `i64` through browser import, editing, RPC, and export. |
| Either integer input form contains `i64` extrema, including inside lists/records | Decode exactly in JSON and TOML; always export tagged decimal-string envelopes. |
| Integer envelope or shorthand is malformed or outside `i64` | Reject; never round or silently coerce. Plain `"42"` remains text. |
| A browser imports raw JSON/TOML containing bare large integers | Decode in Rust before JavaScript number parsing and return canonical string envelopes. |
| Entry positions, view boundaries, or counts exceed JavaScript's exact range | Preserve decimal strings throughout requests/results and files; reject values outside their typed ranges. |
| Rejected proposal has warnings as well as inconsistencies | Retain complete analysis context for display without exposing commit capability. |
| A minted token is supplied and database identity differs despite matching IDs/count | Reject before mutation. |
| A token is malformed or uses an unknown version | Reject; do not fall back to applying without a token. |
| Two proposals are minted against one database/base | Tokens share that database/base and have distinct proposal identities; minting leaves graph history unchanged. |
| No token is supplied and the database changes between standalone check and apply | Check anew against the current state and apply only if valid; leave the source file unchanged. |
| A token is supplied without a previous standalone check | Apply verifies its database/base and performs the full check itself. |
| A standalone check used an earlier evaluation time or analyzer implementation | Apply checks using its selected context and reports the actual result. |
| History mode is requested after evaluation time or implementations change | Show replayed observations under the displayed new context, not an archive of prior runs. |
| Analyzer changes, disappears, fails, or receives a new evaluation time | Check anew; changed inputs may change diagnostics, and missing or failed analyzers reject application. |
| Schema migration is valid only at the final view | Reject the first invalid prefix. |
| Payloads of independent new roots are identical | Apply the core duplicate rule; do not invent identity fields. |
| Check is run twice with identical view/context/inputs | Produce equivalent operations, ordering, and findings, accounting for independently allocated provisional IDs. |
