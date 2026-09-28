# LifeCore Lite Authoring Files

Status: proposed shared authoring contract for JSON/TOML change files, the React
client, and the synchronous application service. [INTERFACES.md](INTERFACES.md)
defines CLI/HTTP routing; [PLAN.md](PLAN.md) lists remaining implementation work.
Section 8.1 fixes the application-owned snapshot and lock layout; section 2.2
fixes the plan-ID encoding. Analysis outputs remain ephemeral.
[SUGGESTIONS.md](SUGGESTIONS.md) defines suggestion export and the replay-verified
acknowledgment of findings. Exact encodings here are provisional until the
[first slice](PLAN.md#41-first-slice-change-files-storage-and-cli) exercises them;
update this document with the implementation when they change.

Change-file commands, browser authoring, atomic persistence, and bounded history
inspection are in scope. Markdown document collections and durable application
receipts are [backlogged](PLAN.md#6-backlog). Sections 4–7 and 8.2–8.4 retain those
future contracts without making collection planning or retry recovery a
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

A future document-collection planner can produce this same proposal type from
intended document content and relationships. Collections may cover only part of
a database; tasks, concepts, and type declarations need not have document files.

## 2. JSON and TOML change files

### 2.1 Syntax and values

Both formats use UTF-8 and deserialize directly into the same Serde types. TOML
input uses [TOML 1.1 syntax](https://toml.io/en/v1.1.0), which the `toml` crate
parses and which accepts every TOML 1.0 document; JSON uses ordinary JSON syntax.
Arrays preserve operation order. Object/table key order does not affect graph
semantics. Files cannot execute code, interpolate environment variables, or fetch
includes. Unknown envelope versions, control fields, and value tags are errors.
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
bounded control values such as format versions and protocol error codes remain
numbers. This applies consistently to application requests/results and files;
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
| `format_version` | Yes | Proposal and value encoding version, initially `1`. |
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
format_version = 1
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
format_version = 1

[[entries]]
name = "task"
kind = "node"
type = "Task"
payload = { title = "Buy groceries", status = "open" }
```

All forms receive complete validation. Each application has a concrete base
protected through publication. A token does not certify an earlier check, freeze
analyzer versions or evaluation time, or recognize a completed retry. Durable
retry recognition is deferred to section 8.4.

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
context are a separate backlogged authoring concern, described below.

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

## 4. Document collections

This section and sections 5–7 are backlogged collection contracts.

### 4.1 Checkout state and document identity

A collection is a declared set of Markdown files plus workspace metadata. The
metadata records a collection identity, database identity, fixed base boundary,
document identities and paths, base payloads, and relationship authoring
bindings. Its physical layout is implementation-defined and versioned.

Each document has a collection-local `document_id`. This stable authoring handle
survives path changes and revisions, but is not a core entity ID and never
resolves a split to an automatic winner. `revision` records the exact revision
represented at checkout. Neither a filename nor a title establishes identity.

Moving a document does not by itself revise a node. Copying a document with its
identity intact is an error. A deliberate new document gets a new document ID
and no revision. Content equality still obeys the core duplicate-node rule;
document IDs do not make otherwise identical root payloads distinct.

Missing tracked files are unresolved removals, not instructions to delete nodes.
An explicit untrack action only removes collection membership. Archiving, if
desired, is a typed payload revision with application-defined meaning.

### 4.2 Markdown with TOML embedded in YAML frontmatter

Managed documents begin with a YAML frontmatter mapping delimited by unindented
`---` lines. The opening delimiter is the first line; the next unindented closing
delimiter ends the frontmatter. The remaining text is the Markdown body.
Frontmatter is an authoring convention outside CommonMark; the body uses
[CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/).
Any later body extensions must be separately versioned and cannot silently
change edge extraction.

The required top-level YAML key `lifecore` contains a string holding the complete
TOML metadata. Writers emit a YAML [literal block scalar](https://yaml.org/spec/1.2.2/#812-literal-style),
`lifecore: |`, so the TOML stays readable and its line breaks are preserved.
Decode the YAML string first, including its indentation and trailing-newline
rules, then parse that string as TOML. Equivalent YAML string spellings are
accepted when their decoded contents are valid metadata. A missing, duplicate,
or non-string `lifecore` field, invalid YAML, or invalid embedded TOML is an error.

```markdown
---
tags: [design]
lifecore: |
  format_version = 1
  document_id = "design-note"
  revision = { origin = "11111111-1111-4111-8111-111111111111", position = "42" }
  type = "Document"
  body_field = "body"

  [payload]
  title = "Design rationale"

  [edges.implementation-task]
  type = "Supports"
  direction = "incoming"

  [edges.underlying-concept]
  type = "Explains"
  direction = "outgoing"
---
[This task][implementation-task] supports the work described here.

The document explains [this concept][underlying-concept].
Compare [the experiment](./experiment.md).

[implementation-task]: lifecore:entry/81
[underlying-concept]: lifecore:entry/96
```

Inside the decoded TOML, `format_version`, `document_id`, `type`, and `body_field`
are required. `payload` is a table, empty if omitted. For an existing document,
`revision` must agree with checkout metadata. For a new root it is absent.
Split metadata is described in section 7.3.

Other YAML fields, such as `tags` in the example, belong to other tools or the
user. Preserve them during LifeCore writeback; they do not implicitly become
graph payload or control metadata. Unknown control fields inside the embedded
TOML remain errors. Its payload uses the same shared Serde value representation
as JSON/TOML change files, including explicit timestamp and record wrappers.
YAML only carries that TOML text; it does not decode graph values.
YAML-native LifeCore metadata is [backlogged](PLAN.md#further-collection-format-and-tooling-work).

An indented delimiter-looking line inside the YAML scalar belongs to its TOML
text and does not close the frontmatter. TOML multiline values retain their
normal meaning after YAML decoding.

The stored payload is `payload` plus the Markdown body under `body_field`.
Declaring that same field inside `payload` is an error. Control metadata,
including `edges`, is not injected into the payload. The type declaration must
accept the resulting payload.

Relationships that should not appear in prose can be written in change files;
they remain visible as external or unowned connections in collection inspection.
The first format does not require dummy prose links to expose such connections.

Physical line endings normalize to LF. Body text otherwise remains exact,
including whitespace and reference definitions. Reformatting YAML or embedded
TOML without changing the decoded TOML metadata values does not change the graph
payload. Reformatting the Markdown body can change its stored string even when
rendering looks the same. No formatter runs implicitly.

### 4.3 Links and relationship annotations

The body uses standard CommonMark inline links and reference links, with ordinary
reference definitions. Relationship annotations live in the embedded TOML;
interpreting links as proposed graph edges adds application semantics without
adding Markdown link syntax. Other readers can parse the same links without
understanding those annotations. See [CommonMark links](https://spec.commonmark.org/0.31.2/#links).

An unannotated Markdown link to another managed document or a `lifecore:entry`
node proposes an outgoing `References` edge. That label must be permitted by
both endpoint types like any other label.

`[edges.<label>]` in the embedded TOML annotates a Markdown reference label.
It accepts:

| Field | Meaning |
| --- | --- |
| `mode` | `declare` (default), `observe`, or `navigate`. |
| `type` | Relationship label; defaults to `References` for graph modes. |
| `direction` | `outgoing` (default) or `incoming`, relative to this document. |

`declare` expresses a relationship managed at this authoring location. `observe`
asserts that the relationship will exist in the planned final view but does not
manage its creation or removal. `navigate` is only a hyperlink; `type` and
`direction` must be absent for this mode. Observation assertions are authoring
checks at the final boundary, not blocking core analyzers at every prefix.

The example proposes `Task #81 --Supports--> Document`,
`Document --Explains--> Concept #96`, and `Document --References--> Experiment`.
Incoming direction does not require the external node to have a document file.

Labels match using CommonMark's Unicode case folding and whitespace
normalization. Colliding annotation labels or duplicate normalized reference
definitions are errors in managed documents. Type names themselves remain exact
strings. An annotation without a used reference link is an error, preventing a
misspelled label from silently falling back to `References`.

Link extraction uses parsed Markdown with source locations. Code spans, code
blocks, images, and raw HTML do not declare edges. Unused reference definitions
do not declare edges. Repeated occurrences of the same directed typed pair
produce one edge; all occurrences remain available as authoring provenance.
An `observe` occurrence can accompany a declaration without claiming ownership.
Syntax that CommonMark treats as plain text, such as an undefined unannotated
reference label, is not a graph link. A linter may warn about likely unfinished
links, but must not invent relationships from bracket-shaped text.

### 4.4 Destinations

| Destination | Interpretation |
| --- | --- |
| `./other.md` | The revision represented by that managed document in the plan's final state. |
| `./other.md#section` | The same node target, with a navigation fragment. |
| `lifecore:entry/81` | Exact existing entry in this collection's database. |
| `lifecore:alias/new_task` | Node named `new_task` in the accompanying plan. |
| `#section` or a path to this same document | Navigation only; no self-edge. |
| External URL or unmanaged attachment | Ordinary navigation; no inferred node or edge. |

The `lifecore:` destinations fit [generic URI syntax](https://www.rfc-editor.org/rfc/rfc3986.html#section-3)
and the CommonMark link grammar. This does not guarantee that every renderer
makes them clickable: a renderer may filter unfamiliar schemes. Renderer
compatibility and future desktop URI-handler installation are deferred tooling
work, independent of collection parsing and graph-link validation.

URI IDs are UTF-8 percent-encoded as one component and decoded exactly once.
Entry links inherit database identity from their collection or archived view;
cross-database graph edges are unsupported. The viewer may inspect either a node
or edge by entry URI, but only nodes can be endpoints of a declared relationship.

Missing `.md` targets or references to formerly tracked documents are errors
unless explicitly marked `navigate`. An existing unmanaged Markdown target also
requires tracking or explicit navigation intent. Other unmanaged attachments do
not become database nodes merely because they are linked.

Relative paths resolve against the containing document using a collection path
map, not a filesystem scan during each append. File graph targets must stay
within the declared collection root. Traversal outside it, symlink escapes, and
ambiguous path spellings are rejected. Case-sensitive logical paths use `/`;
case collisions on a host filesystem are errors. A move helper may rewrite
links, but the resulting body edits must be shown in the plan.

A fragment is not an edge payload or a separate node. An explicitly annotated
graph link to the containing document is an error rather than an ignored
self-edge. External URLs cannot be made graph endpoints merely by annotation;
an external resource must first be represented by a node.

Exact entry destinations do not follow successors. A stale endpoint blocks a
proposed edge; the checker may offer current descendants but must require an
explicit selection. A `navigate` reference can cite a superseded revision
without creating an impermissible current edge. The viewer must show that the
target is historical.

Alias links permit documents to connect to mechanical nodes created in the same
plan. The entry ordering must still place both endpoints before their edge.
Successful writeback replaces alias destinations with exact entry URIs. The
committed body retains its original text; a retained resolution binding gives
that alias its exact meaning in historical views. Such receipt-driven spelling
changes are recorded as checkout normalization, not mistaken for new user edits.
The baseline therefore retains both the actual committed payload and the exact
post-writeback checkout text. Only recorded writeback substitutions receive this
treatment; arbitrary body edits remain revisions. A subsequent real revision
stores its complete then-current body, including the resolved URI spelling.

## 5. Relationship authority and database agreement

Authoring authority concerns who may interpret removing a declaration as a
request to retire a relationship. It does not determine whether an edge exists:
only core history does that.

Each managed relationship has one authoritative authoring location, independent
of edge direction. It may be an outgoing or incoming declaration in a document.
Repeated declarations of the same typed pair within that document share one
binding. Independent declarations in two documents are a conflict; one must
become `observe` or `navigate`, or authority must be explicitly transferred.

Bindings associate the authoring location with exact edge IDs and record their
successors through reconnection. They are persistent, versioned frontend
metadata. They do not give core edges predecessors, payloads, or replacement
semantics. A matching existing unowned edge requires explicit adoption instead
of silent authority acquisition. A change-file-created edge is initially
unowned unless a collection manifest explicitly binds it.

The planner compares both the checkout baseline and the full affected database
neighborhood. In particular:

- Removing the last authoritative declaration proposes retiring its relationship.
  Removing an observation or navigation link does not.
- Editing a document must account for all current incident edges, including
  edges authored elsewhere and edges with no authoring owner.
- Unaffected external relationships are proposed for explicit preservation.
  Unresolved authority conflicts, stale exact targets, or split destinations
  block the plan; they are not grounds to silently drop connections.
- A change file may alter managed nodes and edges. It must report which bindings
  become outdated. A collection must refresh and reconcile before applying over
  those changes, even when its files were not edited.

At a successful final boundary, every `declare` and `observe` graph link must
match a current edge with the specified direction, type, and resolved endpoints.
Every current edge in the affected neighborhood must be explained in the
preview as document-managed, externally managed, or unowned. The interface must
not claim that a document lists all its connections when it lists only some.

Database publication and collection writeback are separate. The frontend must
display when the graph has committed but document synchronization is incomplete.

## 6. Compiling a collection to a change file

The planner must:

1. Freeze checkout metadata and source bytes, verifying database identity/base.
2. Match documents by identity; detect additions, edits, moves, removals, and
   explicit splits. Compare parsed payloads and relationships with the baseline.
3. Resolve all intended links against the complete proposed collection and any
   accompanying mechanical entries. Consider edited documents together.
4. Determine required node revisions and relationship retirements. Removing an
   edge requires revising an endpoint; prefer the containing authoritative
   document when possible, whether the edge is incoming or outgoing.
5. Account for every affected incident edge. Reuse already current matching edges
   where no append is needed. Expand preservation and desired connections into
   exact edge appends, coalescing repeated link occurrences.
6. Propose a deterministic operation order respecting necessary dependencies.
   Simulate every prefix and verify final collection correspondence.
7. Emit the TOML plan with a minted `plan_id` for that database/base and a frozen
   collection manifest for inspection/application.

Combining mechanical entries and generated document operations creates one
explicitly ordered plan with a single alias namespace. Duplicate operation names
are errors. The planner may propose insertion positions for generated entries,
but must preserve the supplied mechanical order unless the user edits that
order. The combined sequence, rather than separate checks of its inputs, is the
unit of preflight.

An edge-only retirement can require an unchanged-payload successor. A metadata
edit that only adds an edge need not create a node revision. If body text changes,
that payload change itself requires a revision. There must be no gratuitous node
revision solely to satisfy a "save" gesture when neither payload nor required
graph operations changed.

The frozen manifest records collection/base identities, input file digests,
source snapshots, document-to-plan bindings, relationship authority, link
occurrences and resolutions, expected final correspondence, and intended file
writeback. It is a versioned companion artifact, retained with the plan. Its
physical encoding is an implementation choice; its identity and content must be
covered by the check/apply receipt. Apply must not reread mutable source files to
construct different payloads. A missing required manifest is an error.

A generated plan that is subsequently edited must be checked against that
manifest again. Reordering valid entries is permitted; changing payloads or
links requires regenerating or explicitly reconciling the source correspondence.
There is no hidden second source of mutations in the manifest: all core appends
appear in the TOML `entries` array.

## 7. Ordering and graph changes

### 7.1 Editing both ends of a link

Suppose current A links to current B. The collection edits B and removes A's
link to it. A possible plan is:

```toml
format_version = 1
plan_id = "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc"

[[entries]]
name = "b"
kind = "node"
type = "Document"
predecessor = { existing = { origin = "11111111-1111-4111-8111-111111111111", position = "73" } }
intent = "revise"
payload = { title = "B", body = "Updated B.\n" }

[[entries]]
name = "a"
kind = "node"
type = "Document"
predecessor = { existing = { origin = "11111111-1111-4111-8111-111111111111", position = "42" } }
intent = "revise"
payload = { title = "A", body = "A no longer links to B.\n" }
```

After B's revision, old A is still current but its old edge is historical. After
A's revision, the intended document state is reached. No edge from old A to new
B should be inserted as an automatic intermediate repair.

Reversing these operations can produce equivalent current content but a
different history. Analyzers may distinguish them. File traversal order or file
modification time must not be treated as user intent. The proposed order must be
visible and editable; the planner must not silently reorder a checked plan to
make validation succeed.

### 7.2 Retaining links and removing edges

When only B changes, an unchanged file link from A to B requests a new edge to
B's selected successor. This can leave A's payload revision unchanged. The
intermediate view without that edge must still pass validation.

When both endpoints change and their link remains, create the selected endpoint
revisions before creating the replacement edge. Mutual document links can be
constructed by creating both nodes and then both edges, if analyzers permit the
intermediate views. An analyzer demanding immediate relationship completeness
can make the sequence impossible; batching with prefix checks cannot fix that.

Removing an incoming relationship does not inherently require revising the
external source. Revising the containing document makes the old edge historical
and permits recreation of its other desired incident edges. Any chosen endpoint
revision and resulting reconnections must appear explicitly in the plan.

### 7.3 Splits

A split is explicit. New successor documents receive new `document_id` values,
omit `revision`, and specify `predecessor` as an exact existing reference with
`intent = "branch"` in the embedded TOML metadata. These two fields are otherwise
absent from ordinary checked-out documents. The old document's departure from
current collection membership must be acknowledged as part of the split, not
treated as unexplained deletion.

The resulting plan appends one successor per document. Connections must be
assigned to a successor, both successors, or neither. Any unchanged incoming
file link to the old document is unresolved until edited. No alias, path, or
document identity automatically selects one leaf or broadcasts to every leaf.

After success, writeback records each new revision and removes the draft split
fields. Core transition permissions still apply to every successor, including
branches from historical predecessors.

## 8. Robust application and recovery

### 8.1 Atomic publication

These requirements belong to the shared synchronous application service used
by HTTP and CLI; graph/analyzer libraries do not own filesystem publication.

Store one JSON snapshot at the selected state path, defaulting to
`./LifeCoreState.json`. Its envelope contains:

| Field | Stored content |
| --- | --- |
| `format_version` | Snapshot envelope version, initially `1`. |
| `graph_revision` | UUID identifying this graph/suite generation, as specified in section 2.2. |
| `graph` | The complete graph: database identity, ordered entries, and exact ordered analyzer suite. |
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
Write the complete candidate to a temporary file in the same directory, flush
and synchronize it, then atomically replace the published snapshot. Synchronize
the containing directory before releasing the lock and acknowledging success.
Initialization uses no-clobber atomic publication and never replaces an existing
state path. No publication step may expose a partly written snapshot.

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
that uncertainty and support inspection of the stored state; do not automatically
repeat the changes. Token minting alone supplies no durable application receipt.
Retry recovery is deferred to section 8.4.

### 8.2 Source edits and file writeback

Before starting collection application, input files must match the frozen source
digests. A user edit after planning requires regenerating the collection plan.
If files change after application starts, frozen payloads still determine any
committed entries. Writeback must not overwrite those newer edits.

Database commits and filesystem updates are separate operations. Receipts must
support completing writeback after a crash without repeating graph appends.
Use atomic replacement of individual files, track writeback progress, and update
the collection's accepted baseline only when reconciliation completes. Partial
writeback must block ordinary replanning until resolved; it must not look like
a mixture of unrelated user edits and new root documents.

Preserve user formatting where possible. Routine writeback changes revision
metadata inside the YAML-carried TOML string, plan-local URI spellings, and
explicitly planned path changes. Preserve unrelated YAML fields and the decoded
meaning of unchanged TOML values. If a file changed concurrently, present the
frozen, committed, and working versions for reconciliation instead of overwriting
it.

### 8.3 Historical rendering and integrity

Retain the source and resolution information needed to interpret links at each
committed boundary, including intermediate prefixes. A relative path or alias
inside an old body must never be resolved against today's directory tree.
An unchanged A can gain an edge to new B without revising A, so resolution
information cannot be keyed only by A's revision ID; it must account for the
view and actual edge history.

Historical rendering distinguishes textual hyperlinks, current graph edges,
historical edges, and navigation-only citations. After B changes but before a
replacement edge is appended, A's text may still mention B while no matching
current edge exists. The viewer must expose that intermediate state faithfully.

Authoring metadata can explain spelling and authority but cannot override core
history. If metadata is missing, core inspection remains possible; collection
mutation must stop where identity or authority cannot be reconstructed reliably.
Export/backup must include retained authoring records to preserve full collection
and historical-link behavior. Importing history still requires all-prefix core
integrity checks, not merely checking the final documents.

### 8.4 Deferred receipts and retry recovery

This future application-service work is outside the current round. It preserves
the recovery requirements for a later implementation; the current optional token
does not claim to implement them. Plans participating in retry recovery would
carry `plan_id`; omitted tokens carry no retry identity guarantee.

- Persist the frozen plan identity/content, actual base, check context, and
  assigned operation IDs in the same atomic publication as the graph. A
  workspace-only journal written afterward cannot close the crash window.
- Retrying an identical completed plan returns its receipt without appending
  again. Reusing its identity with different content is an error.
- Verify identity/content and look up the receipt before checking the original
  expected base, since a successful application has itself advanced that base.
- Recover uncertain acknowledgment and any backlogged collection writeback
  without repeating graph appends. Receipts are auxiliary persistence metadata,
  not graph entries.

The future design must cover empty plans as well as plans that change the graph.
Replay-verified acknowledgment records nothing about prior use. An empty
suggestion can acknowledge a finding without changing graph revision; restoring
and reapplying it may acknowledge again. A dismissal record is not a receipt.
Future recovery must include the combined graph/dismissal result and avoid
silently reapplying either effect after a later restore.
Receipt retention, encoding, and storage layout remain decisions for that work.

## 9. Robustness scenarios to verify

These are behavioral acceptance scenarios for an implementation, not additional
exceptions to the core contract. Collection scenarios remain backlogged; receipt
recovery scenarios below are explicitly deferred.

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

Deferred collection scenarios:

| Scenario | Required outcome when collections are implemented |
| --- | --- |
| Both A and B change and A removes its link | Plan both revisions without unnecessary transient reconnection. |
| B changes and unchanged A retains a file link | Show explicit reconnection and the intervening historical edge. |
| Exact mechanical target is superseded | Require target selection or navigation-only intent. |
| Incoming link is added from a mechanical node | Create the correctly directed edge after both endpoints exist. |
| Incoming link is removed | Show endpoint revision and all affected reconnections. |
| Two documents declare authority for the same edge | Report conflict; no last-writer-wins rule. |
| Change file modifies a document-managed relationship | Mark bindings outdated and require collection reconciliation. |
| Repeated prose links resolve to one typed pair | Emit one edge and preserve occurrence locations. |
| Broken graph-link destination, ambiguous label, or mistyped annotation | Report source-local error; never silently omit the intended edge. |
| A document is moved, copied, deleted, or split | Distinguish identity-preserving move, duplicate identity, unresolved removal, and explicit branching. |
| File link target escapes the root or has case ambiguity | Reject graph resolution deterministically. |
| User edits a file during apply or writeback | Preserve newer bytes and expose the synchronization conflict. |
| Alias URI writeback is followed by an unchanged collection check | Produce no revision; use the recorded normalized checkout baseline. |
| Old document contains a renamed path or plan-local alias | Resolve from retained view-specific information, never today's files. |
| Authoring metadata is lost or incompatible | Preserve core readability; block unsafe collection reconciliation. |

Deferred collection-format acceptance scenarios:

| Scenario | Required outcome when collections are implemented |
| --- | --- |
| A YAML literal block contains LifeCore TOML metadata | Decode the YAML string, then validate TOML with the shared change-file value encoding; preserve exact reference-link destinations in the body. |
| YAML or TOML presentation changes without changing decoded metadata values | Produce no graph payload change from that reformatting. |
| An indented `---` line occurs inside a TOML multiline value in the YAML scalar | Keep it in the decoded value; only the unindented frontmatter delimiter ends the header. |
| The `lifecore` field is missing, duplicated, a YAML mapping, or invalid TOML text | Reject managed-document metadata; YAML-native LifeCore fields remain backlogged. |
| Revision writeback encounters unrelated YAML fields | Preserve those fields without importing them into the graph payload. |
| A renderer does not make a valid `lifecore:` link clickable | Collection parsing and graph-link validation still work; renderer integration and URI-handler installation remain separate future work. |

Deferred receipt acceptance scenarios:

| Scenario | Required outcome when receipt recovery is implemented |
| --- | --- |
| Crash occurs after commit but before client acknowledgment | Recover the operation receipt; do not append twice. |
| A completed plan is retried after its base has advanced | Verify plan identity/content and return the receipt before stale-base rejection. |
| Plan identity is reused with changed content | Reject the conflicting identity, even if the earlier plan committed. |
| An empty plan is retried | Recognize its completed application through its receipt even if the graph/suite state did not change. |

## 10. Implementation boundaries

One shared proposal representation, codec, checker, and application service
serve the React client, HTTP RPC, CLI RPC, and change-file commands. Format and
transport adapters cannot introduce unchecked writes or different domain rules.
The application owns filesystem envelopes and publication; the graph and analyzer
libraries remain synchronous and independent of HTTP and storage.

The current round includes JSON/TOML change files, proposal export, optional plan
IDs and minting, full check/apply, durable storage, and the React interface.
Document checkout, link extraction, authority reconciliation, and durable receipt
recovery remain backlogged. Collection mutation must eventually handle its
writeback/recovery contract explicitly.

The selected collection format embeds TOML in a YAML frontmatter string and uses
CommonMark body links. YAML-native metadata and external-renderer clickability
or desktop URI-handler installation are further backlog work. Existing URI
context rules remain unchanged. Future draft helpers must preserve exact
endpoints, require explicit split selection, and expose intermediate views.
