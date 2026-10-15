# LifeCore Lite Authoring Files

Status: proposed shared authoring contract for JSON/TOML change files, the React
client, and the synchronous application service. [INTERFACES.md](INTERFACES.md)
defines CLI/HTTP routing; [PLAN.md](PLAN.md) lists remaining implementation work.
Section 8 fixes the application-owned snapshot and lock layout; section 2.2
fixes the plan-ID encoding. Analysis outputs remain ephemeral.
[SUGGESTIONS.md](SUGGESTIONS.md) defines suggestion export and the replay-verified
acknowledgment of findings. Exact encodings here are provisional until the
[first slice](PLAN.md#41-first-slice-change-files-storage-and-cli) exercises them;
update this document with the implementation when they change.

Change-file commands, browser authoring, atomic persistence, and bounded history
inspection are in scope. Markdown document collections and durable application
receipts are [backlogged](PLAN.md#6-backlog); their contracts are retained in
[BACKLOG.md](BACKLOG.md) without making collection planning or retry recovery a
prerequisite for the web application.

This document complements [SPEC.md](SPEC.md), which takes precedence for graph
identity, history, typing, and transaction validity. Command option spellings
beyond those fixed in INTERFACES remain illustrative.

## 1. Purpose and model

JSON and TOML change files serialize the same shared Rust `Proposal` used by
application requests. A browser draft, a handwritten file, and an explicitly
selected analyzer suggestion all reach the same checking and application path.
File parsing is a Serde codec, not a separate authoring interpreter.

A proposal describes ordered additions to history. Drafts are mutable; applying
one stores complete immutable entries. Later edits to a source file must not
change stored payloads or reinterpret committed entries. Local operation names
are authoring handles, not substitutes for exact identities in stored edges.

A future [document-collection planner](BACKLOG.md#1-document-collections) can
produce this same proposal type from intended document content and
relationships.

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
| Record | Object/table with `kind = "record"` and an object/table of recursively encoded fields as `value`. |

Node payloads are typed maps of field names to values. Their outer map needs no
record wrapper; record-valued fields do. Similarly, known proposal/control
structs use ordinary objects/tables. Wrapping records prevents their field names
from colliding with the tagged value envelope. Canonical output uses native
strings and arrays; all other payload kinds are wrapped.

For example, these node payloads represent the same values:

```toml
[payload]
title = "Submit report"
priority = { kind = "integer", value = "2" }
labels = ["work", "writing"]
deadline = { kind = "timestamp", value = "2026-09-30T17:00:00Z" }
completed = { kind = "boolean", value = false }
details = { kind = "record", value = { owner = "Teto" } }
```

```json
{
  "payload": {
    "title": "Submit report",
    "priority": { "kind": "integer", "value": "2" },
    "labels": ["work", "writing"],
    "deadline": { "kind": "timestamp", "value": "2026-09-30T17:00:00Z" },
    "completed": { "kind": "boolean", "value": false },
    "details": { "kind": "record", "value": { "owner": "Teto" } }
  }
}
```

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
bounded control values such as protocol error codes, source lines and
columns, and operation indexes within one plan remain numbers. This applies consistently to application requests/results and files;
it does not change the integer types used by the in-memory graph. These fields
accept only decimal strings, with the same syntax as integer envelopes; bare
numbers are not shorthand for them.

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
Known timestamp fields such as `EvaluationInputs.evaluation_time` already have a
static type and use the Serde string directly, without a payload wrapper.

Export must preserve each represented core instant and its nanoseconds when
read back by the same shared codec. The React client must retain full timestamp
strings and decimal-string integers for the entire `i64` domain; see
[the client contract](INTERFACES.md#5-react-client). Jiff documents its
[default timestamp serialization](https://docs.rs/jiff/latest/jiff/fmt/serde/timestamp/index.html).

### 2.2 Envelope and ordered entries

`Proposal` has these fields in both JSON and TOML:

| Field | Required | Meaning |
| --- | --- | --- |
| `plan_id` | No | Token identifying the database, exact graph/suite base, and proposal. |
| `acknowledges` | No | Complete `FindingKey` of a suggestion these entries implement; see [SUGGESTIONS](SUGGESTIONS.md). |
| `entries` | Yes | Ordered array of proposed entries; may be empty. |

There is one value representation, with no separate value-profile selector.
Optional fields are omitted when absent; JSON `null` is rejected rather than read
as absence. There are no separate `database` or `base` fields. The host selects the database
through the [path rule](INTERFACES.md#4-database-selection-and-concurrency).
A supplied token asserts that this database and its current base match.
`acknowledges` is verified by replay and, on success, dismisses that finding
atomically with the graph changes. There is no general `dismissals` list.

This example assumes compatible type declarations and an existing current node
with the illustrated exact identity:

```toml
plan_id = "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc"

[[entries]]
name = "task_done"
kind = "node"
type = "Task"
predecessor = { existing = { origin = "11111111-1111-4111-8111-111111111111", position = "42" } }
intent = "revise"
payload = { title = "Write proposal", status = "done" }

[[entries]]
name = "explanation"
kind = "node"
type = "Note"
payload = { title = "Why we changed direction", body = "The original approach depended on unavailable equipment." }

[[entries]]
name = "explanation_link"
kind = "edge"
type = "Explains"
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
- The graph revision UUID is stored in the snapshot envelope. Generate a fresh
  UUID whenever graph contents or the exact ordered analyzer suite changes and
  publish it atomically with that state. Equal-count divergent histories and
  suite-only changes therefore invalidate old tokens. Dismissal edits, no-op
  writes, and token minting leave this UUID unchanged.
- The proposal UUID distinguishes proposals against the same database/base.
  Mint a fresh identity for changed operations.

The revision is an application-maintained generation identifier, not a content
hash or a graph boundary. Verify the token's database/revision against the loaded
snapshot, then retain the core's exact database, entry count/content, and ordered
suite comparisons for prepared commit and publication. Moving or copying an
entire snapshot preserves its identities; a subsequent graph/suite mutation
allocates a fresh revision. Never combine a revision with unrelated graph state.
Direct edits of the stored snapshot that retain its revision are outside this
guarantee; use change files and application operations to mutate it. The revision
cannot detect arbitrary manual content changes that leave its metadata intact.

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
type = "Task"
payload = { title = "Buy groceries", status = "open" }
```

All forms receive complete validation. Each application has a concrete base
protected through publication. A token does not certify an earlier check, freeze
analyzer versions or evaluation time, or recognize a completed retry. Durable
retry recognition is [deferred](BACKLOG.md#7-deferred-receipts-and-retry-recovery).

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
context are a separate [backlogged](BACKLOG.md#14-destinations) authoring concern.

### 2.4 Node and edge operations

A node entry requires `name`, `kind = "node"`, `type`, and complete `payload`.
It may also have `predecessor` and `intent`:

- No predecessor creates a root, and `intent` must be absent.
- A predecessor requires `intent = "revise"` or `intent = "branch"`.
- `revise` requires the predecessor to be current immediately before the append,
  including earlier operations in this proposal.
- `branch` permits a superseded predecessor; it still requires the contextual
  transition permission. It is also valid for a current predecessor.

Omitting a field removes it from the successor payload, subject to schema rules.
Patch/default/connection-preservation helpers must expand to complete nodes and
explicit edges before checking. The format has no implicit reconnection.

An edge requires `name`, `kind = "edge"`, `type`, `from`, and `to`, with no payload
or predecessor. Endpoint permissions and freshness are checked at its position.
Type declarations are ordinary node proposals with `type = "Type"` and no
bootstrap exemption. No delete, retarget, or in-place update operation exists.
Proposals do not change the analyzer suite; that is the deliberate `add-analyzer`
operation in [INTERFACES](INTERFACES.md#2-cli-surface).
Explicit duplicates fail rather than being silently skipped.

### 2.5 Entry-shaped types

Several types describe "an entry" at different stages. They differ in how they
refer to other entries and in what they may contain, so none can stand in for
another. All of them serialize in this document's vocabulary, so the snapshot,
analysis output, RPC results, and change files speak one language: `kind`
(`node` or `edge`), `type`, and either `predecessor` (omitted for a root),
`intent` where an operation carries one, and `payload`, or `from` and `to`;
references are `existing` or `added`. They differ only in the members of their
stage, shown below.

| Type | Stage | Refers to other entries by | Why it is its own type |
| --- | --- | --- | --- |
| `ProposalEntry` (`lifecore-app`) | Authoring and transport: files, RPC, drafts | Operation name (`added = "name"`) or exact ID | Carries author-facing names and this document's vocabulary. Contains only what an author may write: no suite changes. Lowering checks names and resolves them to operation indexes, producing `EntryChange`. |
| `transaction::EntryChange` | Core staging input for node and edge operations | Operation index (`Reference::Added(n)`, serialized `{ added = n }` with a number) or exact ID | The core API, free of names. Has no identity yet; `prepare` assigns IDs. Serialized like a `ProposalEntry` without `name`, as in a suggestion's actions. A lowered proposal and a suggestion's actions are both `Vec<EntryChange>`, so [acknowledgment](SUGGESTIONS.md#3-verification-by-replay) compares by equality, and neither can change the suite. `transaction::Change` wraps it beside `AddAnalyzer` (`kind = "analyzer"`, with `name` and `configuration`), which only `init` and `add-analyzer` build. |
| `Entry` (`Node`, `Edge`) | Stored history | Exact ID only | An immutable record with its own assigned `EntryId` in `id`, as stored in the snapshot. |
| `EntryRecord` (`lifecore-app`) | Read results (`entries.list`, `entries.get`) | Exact ID | The stored `Entry` with `current`, its currentness in the selected view, beside its members. |

### 2.6 Type declaration payloads

A type declaration is an ordinary node proposal with `type = "Type"`
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
A shape is `text`, `integer`, `boolean`, or `timestamp`; `list` followed by the
element shape; or `record` followed by one record envelope whose fields are
themselves field declarations, which is then the last element. Scalar and list
shapes therefore need no envelopes; only `fields` itself, being a record-valued
payload field, keeps the envelope that [section 2.1](#21-syntax-and-values)
requires:

```toml
[[entries]]
name = "task_type"
kind = "node"
type = "Type"

[entries.payload]
name = "Task"
fields = { kind = "record", value = { title = ["mandatory", "text"], tags = ["optional", "list", "text"] } }
outgoing = ["DependsOn"]
incoming = ["DependsOn"]
successors = ["Note"]
```

A list of lists is `["optional", "list", "list", "integer"]`. A record-shaped
field is `["optional", "record", { kind = "record", value = { owner = ["mandatory", "text"] } }]`.

Malformed declarations are reported with the path of the offending element,
using the list position for words: a missing presence or shape word (reported at
the position where it was expected), an unknown word, a non-string where a word
belongs, `record` followed by nothing, by something other than a record
envelope, or by further elements, a repeated permission, or more than 64 nested
`list` and `record` words, which bounds recursion on hostile input. Canonical
output writes all five keys, with empty lists and an empty `fields` record where
nothing is declared. Forms in the React client generate exactly this encoding,
so a declaration written by hand and one built in the browser are the same
declaration.

## 3. Planning, checking, and applying

The synchronous application service owns proposal parsing/export, plan-ID
minting, checking, graph/suite and dismissal persistence, and publication. CLI
convenience commands, CLI RPC, and HTTP call the same operations. Revision intent
is validated by `transaction::prepare` for every plan; source validation for
explicit suggestions must remain consistent with
`analysis::suggestion::prepare_suggestion`; exports use the shared `Proposal` type.
No compatibility reader or migration is required.

Keep dedicated change-file commands:

```sh
lifecore plan-id
lifecore check changes.toml
lifecore apply changes.toml
lifecore check changes.json
lifecore apply changes.json
lifecore export changes.json --format toml
```

Analysis results pair each suggestion observation with a ready-made proposal, as
described in [SUGGESTIONS](SUGGESTIONS.md#2-export). `export` converts any
proposal between JSON and TOML, preserving `plan_id` and `acknowledges` verbatim;
it neither refreshes a base nor checks acceptance. The collection planner remains
deferred. `rpc` is independently the one-line request/response adapter specified
in [INTERFACES](INTERFACES.md#2-cli-surface); it has no method
argument or change-file option.

`check` does not append, rewrite the source, or mint a missing token. It freezes
the decoded proposal, verifies any token/base, and evaluates every
historical and proposed prefix under the selected suite and inputs. Reports
include the selected database and exact
base, ordered operations, exact references, provisional mappings, differences,
relevant operation/field locations, and complete analysis context. Structurally
valid proposals with inconsistencies should retain their full analysis output
for preview, including advisories, but must not be committable. Structural and
analyzer execution errors remain distinct from emitted findings.
Include the verified prospective acknowledgment, if any, while saving neither
effect.

Analysis outputs are ephemeral, and in-memory caching is deferred. The snapshot
stores no report archive.
After restart, analysis replays the requested history; history-mode selection
is across prefixes within that run, not past runs at earlier evaluation times.
Changed time or implementations may yield different findings. Persist dismissal
records independently, including keys with no currently emitted observation.
See [analysis lifetime](INTERFACES.md#6-analysis-lifetime-and-dismissal-persistence).

`EvaluationInputs` defaults to absent time. If a user explicitly chooses current
time, the host captures one instant before replay and reports it. Neither runner
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

The complete plan publishes atomically after every prefix passes. Preserve writer
exclusion from exact base verification through publication and synchronization.
Intermediate committed boundaries remain inspectable. Never silently refresh a
token, reconnect edges, or resume a partially committed suffix. After uncertain
acknowledgment, support inspection without automatically retrying the mutation.

## 8. Atomic publication

These requirements belong to the shared synchronous application service used
by HTTP and CLI; graph/analyzer libraries do not own filesystem publication.

Store one JSON snapshot at the selected state path, defaulting to
`./LifeCoreState.json`. Its envelope contains:

| Field | Stored content |
| --- | --- |
| `graph_revision` | UUID identifying this graph/suite generation, as specified in section 2.2. |
| `graph` | The complete graph: database identity (`id`), ordered `entries` in the vocabulary of [section 2.5](#25-entry-shaped-types), and the exact ordered analyzer `suite`, each a `name` and its verbatim `configuration`. |
| `dismissals` | Exact finding keys, dismissal reasons, and optional notes. |

Graph, revision, and dismissals form one atomic publication unit. There is no
stored `AnalysisOutput` or report directory. Rewriting the full snapshot is an
accepted initial cost. Reject malformed envelopes and structurally invalid
graphs on load; ordinary inspection does not require installed analyzers.

Append `.lock` to the state path for the stable lock file, defaulting to
`./LifeCoreState.json.lock`. Keep this file across publications; do not lock the
replaceable snapshot inode or delete/recreate the lock file on release. All
cooperating writers use this target, including initialization and dismiss/restore.

A mutation acquires writer exclusion, reloads the current snapshot, verifies
the expected base, checks/prepares the change, and stages any verified
acknowledgment alongside concurrent dismissal records.
Allocate a fresh graph revision only if graph/suite state changes.
Write the complete candidate to a temporary file in the same directory, with
the published snapshot's permissions applied before any content, flush and
synchronize it, then atomically replace the published snapshot. Synchronize
the containing directory before releasing the lock and acknowledging success.
Initialization uses no-clobber atomic publication and never replaces an existing
state path: it hard-links the synchronized temporary file into place, so it
requires a filesystem with hard links. No publication step may expose a partly
written snapshot.

Readers open one snapshot and read it completely; replacement leaves that open
snapshot immutable. No shared read lock is needed. A read concurrent with a
publication may return the complete old or new snapshot. Current-state reloads
reopen the selected path; retaining an old open handle cannot observe replacement.

- Exclude cooperating writers from exact base verification through publication;
  two candidates prepared against one base cannot both replace it.
- Initialize a new empty database without clobbering an existing one.
- Publish a whole checked plan atomically. Failures before publication preserve
  the previous stored state. Flush/synchronize required data before publication
  and synchronize the containing storage after publication.
- Distinguish post-publication synchronization failure from rollback: the new
  state may already be visible, so acknowledgment can be uncertain.
- Reload graph/suite, graph revision, and dismissal data without changing identities.
  Dismiss/restore survives reload and absent current emission. Concurrent dismissal
  edits reread under writer exclusion and preserve other writers' records.
- Validate a supplied token's database/base expectation before preparation and
  preserve the chosen concrete base through publication even when no token was
  supplied. Persist the base identity consistently with the graph/suite state.
- Publish graph changes and any acknowledgment as one unit. Invalid proposals
  and pre-publication failures preserve both. A post-publication failure never
  justifies claiming that one half rolled back.

A failure before publication leaves the previous graph intact. After publication,
failed synchronization or acknowledgment may leave the caller uncertain. Report
that uncertainty together with the outcome the call would have returned,
including the new graph revision and operation-to-entry mapping, and support
inspection of the stored state; do not automatically repeat the changes. Token
minting alone supplies no durable application receipt. Retry recovery is
[deferred](BACKLOG.md#7-deferred-receipts-and-retry-recovery).

## 9. Robustness scenarios to verify

These are behavioral acceptance scenarios for an implementation, not additional
exceptions to the core contract. Collection and receipt-recovery scenarios are in
[BACKLOG §8](BACKLOG.md#8-acceptance-scenarios).

| Scenario | Required outcome |
| --- | --- |
| Equivalent JSON and TOML proposals | Decode to the same shared values, operation order, and references. |
| Nested lists, wrapped records, booleans, or empty collections | Round-trip without changing value kinds or confusing record fields with tags. |
| Wrapped timestamp is imported/exported | Use Jiff Serde behavior and preserve represented instants and nanoseconds; test its accepted edge cases without a separate datetime profile. |
| Plain timestamp-like text or a native TOML datetime appears | Keep plain strings as text; reject unwrapped native datetime payload syntax. |
| An integer is outside JavaScript's exact-number range | Preserve the exact `i64` through browser import, editing, RPC, and export. |
| Either integer input form contains `i64` extrema, including inside lists/records | Decode exactly in JSON and TOML; always export tagged decimal-string envelopes. |
| Integer envelope or shorthand is malformed or outside `i64` | Reject; never round or silently coerce. Plain `"42"` remains text. |
| A browser imports raw JSON/TOML containing bare large integers | Decode in Rust before JavaScript number parsing and return canonical string envelopes. |
| Entry positions, view boundaries, or counts exceed JavaScript's exact range | Preserve decimal strings throughout requests/results and files; reject values outside their typed ranges. |
| Rejected proposal has warnings as well as inconsistencies | Retain complete analysis context for display without exposing commit capability. |
| A minted token is supplied and database identity differs despite matching IDs/count | Reject before mutation. |
| A token is malformed or uses an unknown version | Reject; do not fall back to applying without a token. |
| Two proposals are minted against one database/base | Tokens share that database/base and have distinct proposal identities; minting leaves graph history unchanged. |
| Supported writes change entries/suite, including equal-count divergence | Publish a fresh graph revision atomically and reject old tokens; require deliberate refresh/replanning. |
| A dismissal changes, a no-op is written, or a token is minted | Preserve graph revision; existing plan bases remain valid. |
| No token is supplied and the database changes between standalone check and apply | Check anew against the current state and apply only if valid; leave the source file unchanged. |
| A token is supplied without a previous standalone check | Apply verifies its database/base and performs the full check itself. |
| A standalone check used an earlier evaluation time or analyzer implementation | Apply checks using its selected context and reports the actual result. |
| Initialization targets an existing database | Refuse without clobbering it. |
| Publication fails before replacement | Leave prior state intact. |
| Synchronization fails after publication | Report possible publication; permit state inspection and do not automatically retry. Receipt recovery is deferred. |
| Dismissal is saved, reloaded, restored, or edited by concurrent frontends | Retain exact keys/reasons/notes and preserve each writer's update; never affect acceptance. |
| A reader opens the snapshot during publication | Return one complete old/new snapshot, never mixed graph/revision/dismissal data. |
| The application restarts | Reload graph/suite and dismissals; recompute analysis without a persisted report registry. |
| History mode is requested after evaluation time or implementations change | Show replayed observations under the displayed new context, not an archive of prior runs. |
| Writer advances the base during checking | Reject under writer exclusion before publishing any operation. |
| Analyzer changes, disappears, fails, or receives a new evaluation time | Check anew; changed inputs may change diagnostics, and missing or failed analyzers reject application. |
| Schema migration is valid only at the final view | Reject the first invalid prefix. |
| Payloads of independent new roots are identical | Apply the core duplicate rule; do not invent identity fields. |
| Check is run twice with identical view/context/inputs | Produce equivalent operations, ordering, and findings, accounting for independently allocated provisional IDs. |

See [SUGGESTIONS acceptance scenarios](SUGGESTIONS.md#7-acceptance-scenarios)
for export, acknowledgment verification, edits, empty actions, and restoration.

## 10. Implementation boundaries

One shared proposal representation, codec, checker, and application service
serve the React client, HTTP RPC, CLI RPC, and change-file commands. Format and
transport adapters cannot introduce unchecked writes or different domain rules.
The application owns filesystem envelopes and publication; the graph and analyzer
libraries remain synchronous and independent of HTTP and storage.

The current round includes JSON/TOML change files, proposal export, optional plan
IDs and minting, full check/apply, durable storage, and the React interface.
Document checkout, link extraction, authority reconciliation, and durable receipt
recovery remain [backlogged](BACKLOG.md). Future draft helpers must preserve
exact endpoints, require explicit split selection, and expose intermediate views.
