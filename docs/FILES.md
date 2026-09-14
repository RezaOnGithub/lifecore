# LifeCore Lite Authoring Files

Status: proposed authoring contract; no implementation is implied.

Current cleanup scope is TOML change files and bounded inspection. Markdown
document collections are backlogged in [PLAN.md](PLAN.md#7-backlog--markdown-document-collections).
The collection design below is retained for later work. Its document checkout,
link extraction, relationship authority, collection manifests, historical link
rendering, and document writeback requirements are not requirements for this
round. TOML plan validation, persistence/retry handling, and core historical
inspection remain in scope.

This document complements [SPEC.md](SPEC.md). The core specification takes
precedence for graph identity, history, typing, and transaction validity.
"Must" describes a requirement of this proposed file interface; "may" describes
an optional convenience. Command names are illustrative.

## 1. Purpose and model

LifeCore has two cooperating plaintext authoring interfaces:

- **TOML change files** describe ordered additions to history. They suit
  mechanical records, scripts, schema changes, and operations whose ordering
  matters.
- **Markdown document collections** describe intended document content and
  relationships. A planner compares them with a known database view and produces
  a TOML change file for inspection, checking, and application.

Documents describe a desired state, not a unique history. A generated change
file makes the proposed history explicit. Both interfaces use the same core
append validator; neither permits temporarily invalid prefixes.

Draft files are mutable. Applying a plan stores complete immutable entries.
Later changes to a source file must not change stored payloads or reinterpret
already committed entries. Paths, document identifiers, and local aliases are
authoring conveniences, not substitutes for exact revision IDs in core edges.

The collection can cover only part of the database. Tasks, concepts, type
declarations, and other nodes need not have document files.

## 2. TOML change files

### 2.1 Syntax and values

Version 1 uses UTF-8 and the [TOML 1.0 syntax](https://toml.io/en/v1.0.0), including
arrays of tables to preserve entry order. Later TOML features are not implicitly
enabled by installing a newer parser.

The initial value profile is `toml-basic-v1`: strings, booleans, signed 64-bit
integers, offset date/time instants, lists, and string-keyed records. Local dates,
local times, floats, null, sets, tuples, and unions are outside this round.
Lists and records may nest. Schema lists are homogeneous; records declare named
fields with presence policies. Timestamp values denote instants, not display
zones. Map key order is irrelevant; list order matters. Equality distinguishes
scalar kinds, and hashes must follow equality. No coercion or Unicode
normalization is implicit. Literal source line endings normalize to LF before
parsing; escaped string content retains its parsed meaning.

Unknown format versions, profiles, and control fields are errors. Payload field
names are governed by the contextual type declaration, including its
additional-fields policy. Files cannot execute code, interpolate environment
variables, or fetch includes during validation.

This profile specifies values, not the schema language. A concrete
implementation must additionally version its schema representation and retain
the interpreters needed for historical validation, as required by SPEC.md.

#### Timestamp conversion contract

Native TOML offset datetimes and `jiff::Timestamp` overlap, but neither can
represent every value of the other. The following authoring restrictions are
proposed for `toml-basic-v1`; the TOML adapter is not implemented yet.

- Require a complete date and time with `Z` or a numeric offset. Local dates,
  local times, and local datetimes cannot supply an instant without additional
  information and are rejected.
- Accept zero to nine fractional-second digits (no decimal point when there
  are zero). Reject longer fractions, even trailing zeros, before a TOML parser
  can discard digits. This is an application restriction on otherwise valid
  TOML, not a claim that TOML limits precision to nanoseconds.
- Reject leap-second notation (`60` in the seconds component) before conversion
  to Jiff. Do not silently clamp it to `59`.
- Require the converted instant to fit Jiff's range and its UTC year to fall
  within `0000..=9999`. Check after applying the input offset, including at year
  boundaries.
- Export accepted instants as native TOML offset datetimes in UTC with `Z`,
  retaining all nanosecond precision. Reject export of a core timestamp outside
  this profile; do not silently substitute a string or truncate the value.

These rules guarantee an exact instant round trip through our adapter for
accepted values, not preservation of the original text. Offset spelling and
fractional-digit count are not stored. For example, `2026-09-14T15:00:00+03:00`
and `2026-09-14T12:00:00Z` become equal timestamps. The RFC 3339 `-00:00`
convention supplies a known UTC instant with an unknown local offset; it is
accepted as that instant, but its unknown-offset metadata is not retained.

The restrictions address concrete representation differences:

- TOML uses RFC 3339's four-digit unsigned year syntax. Jiff also represents
  negative years, which cannot generally be exported as native TOML datetimes.
  In the currently locked Jiff 0.2.33, `Timestamp::MAX` is
  `9999-12-30T22:00:00.999999999Z`; the valid TOML datetime
  `9999-12-31T23:59:59Z` is therefore outside Jiff's range.
- TOML requires only millisecond precision and requires truncation of precision
  beyond an implementation's capability. Our adapter must preserve nanoseconds
  and inspect fractional digits before such truncation. A parsed datetime that
  has already lost digits cannot enforce this contract.
- Jiff represents Unix instants without distinct leap seconds. Its timestamp
  parser accepts second `60` by constraining it to `59`, so relying on that
  parser alone would lose information.

Jiff's default Serde implementation serializes a string and deserializes a
string; it is not a native TOML datetime adapter. The importer/exporter must
explicitly bridge TOML datetime values and `Timestamp`. A quoted datetime in a
payload remains ordinary text. Existing tagged JSON persistence uses Jiff's
string representation under `kind = "precise_time"` and can retain timestamps
outside the TOML profile. The core `PreciseTime(Timestamp)` variant currently
does not enforce the narrower authoring range. Supporting every core timestamp
in change files would require an additional explicit encoding or a decision to
restrict core values; neither is implemented by this proposal.

References: [TOML 1.0 offset datetimes](https://toml.io/en/v1.0.0#offset-date-time),
[RFC 3339 grammar](https://www.rfc-editor.org/rfc/rfc3339.html#section-5.6),
[unknown local offsets](https://www.rfc-editor.org/rfc/rfc3339.html#section-4.3),
and [Jiff Timestamp](https://docs.rs/jiff/0.2.33/jiff/struct.Timestamp.html).

### 2.2 Envelope and ordered entries

Examples assume the named declarations and referenced current nodes already
exist with compatible schemas and permissions; they illustrate file syntax,
not an empty-database bootstrap.

```toml
format_version = 1
value_profile = "toml-basic-v1"
database = "personal"
base = 184
plan_id = "proposal-update-001"

[[entries]]
name = "task_done"
kind = "node"
type = "Task"
predecessor = "@42"
intent = "revise"
payload = { title = "Write proposal", status = "done" }

[[entries]]
name = "explanation"
kind = "node"
type = "Note"

[entries.payload]
title = "Why we changed direction"
body = """
The original approach depended on unavailable equipment.
"""

[[entries]]
name = "explanation_link"
kind = "edge"
type = "Explains"
from = "$explanation"
to = "$task_done"
```

The envelope has these required fields:

| Field | Meaning |
| --- | --- |
| `format_version` | Authoring format version; initially `1`. |
| `value_profile` | Selected stored-value representation. |
| `database` | Persistent database identity, not merely a filename or mutable display name. |
| `base` | Expected starting boundary, expressed as an exclusive entry count. |
| `plan_id` | Nonempty operation identity, unique within this database. |
| `entries` | Ordered array of proposed entries; may be empty. |

The sample database identity is illustrative. A real identity must distinguish
unrelated databases even if their entry counts and local IDs coincide.

Each array element describes exactly one append. TOML table key order does not
affect semantics; `entries` array order does. Comments are never operations.
Entry IDs are assigned on commit and returned in the receipt, not claimed by
the author.

### 2.3 References and names

- `@42` denotes the exact existing entry with database ID `42`. IDs are opaque
  nonempty strings after `@`; the numeric examples do not require numeric IDs.
- `$task_done` denotes a node introduced by an earlier entry in this plan.
- Every entry has a unique `name` matching `[A-Za-z_][A-Za-z0-9_-]*`. Edge names
  identify operations and receipts but cannot serve as node endpoints.
- Existing references must be present at `base`. Aliases cannot be forward
  references. Predecessors and endpoints must resolve to nodes.
- Ordinary payload strings are literal, including strings starting with `@` or
  `$`. Only designated reference fields perform resolution.

There is no implicit lookup by title, root, path, or "latest descendant" in a
change file. Search and selection helpers must materialize exact references.

### 2.4 Node and edge operations

A node entry requires `name`, `kind = "node"`, `type`, and `payload`. It may
also have `predecessor` and `intent`:

- No predecessor creates a root; `intent` must then be absent.
- A predecessor requires `intent = "revise"` or `intent = "branch"`.
- `revise` additionally requires the predecessor to be current immediately
  before this operation. This authoring guard prevents accidental splits.
- `branch` explicitly permits a superseded predecessor. It still requires the
  core transition permission in the preceding view. It is also valid for a
  current predecessor, for example the first operation of an explicit split.

The payload is complete. Omitting a previously present field removes it from
the new payload, subject to schema validation. Patch syntax, inherited defaults,
and "keep all connections" are possible draft-generation helpers, but are not
operations in this version of the executable format. They must expand to
complete nodes and explicit edges before checking.

An edge entry requires `name`, `kind = "edge"`, `type`, `from`, and `to`. It has
no payload or predecessor. Both endpoint permissions and freshness are checked
at its position in the sequence.

Type declarations use ordinary node entries with `type = "Type"`. They receive
no bootstrap or migration exemption. There are no delete, edge-retarget, or
in-place update operations.

Explicit duplicate entries fail the core duplicate rules; application must not
silently skip them. A collection planner may coalesce repeated link occurrences
into one proposed edge before producing the file.

## 3. Planning, checking, and applying

The basic workflows are:

```sh
lc check changes.toml
lc apply changes.toml

lc plan --output changes.toml
lc check changes.toml
lc apply changes.toml
```

`check` does not append history or update the document checkout. It resolves the
specified database and boundary, freezes input values, and simulates the listed
appends in order. It performs the core append checks and resulting-view checks
at every prefix. A successful final graph is not sufficient.

The report must include:

- The base, plan identity, ordered operations, and exact resolved existing IDs.
- Payload differences, type transitions, and connections that become historical,
  are retained through new edges, or are deliberately dropped.
- The first failing prefix, operation name, relevant IDs, and source locations
  where available. Parsing errors can be collected before simulation.
- Type-presence warnings and analyzer diagnostics without treating them as
  implicit repairs.

Analyzer identities/versions, configuration, explicit evaluation inputs such as
time, format/schema interpreters, and plan content are part of the check
context. A check receipt must record this context. An old successful check is
not authorization to ignore changes to it.

`apply` freezes the file, verifies the database identity and expected base, and
repeats a full preflight before appending. The default rejects a changed head;
there is no implicit rebase. A newer evaluation context requires a new check
result to be presented, rather than claiming the earlier result still applies.
During application the selected context remains fixed.

The complete plan commits atomically after every prefix passes. Hold the writer
exclusion and expected-base check through replacement so another writer cannot
interleave with publication. Each committed intermediate boundary remains
inspectable. Editing the plan changes its check result and requires a new plan
identity; there is no partially committed plan suffix to resume.

## 4. Document collections

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

### 4.2 Markdown with TOML frontmatter

Managed documents begin with a TOML block delimited by standalone `+++` lines.
The first following closing delimiter ends the block; the remaining text is the
Markdown body. Delimiter lines cannot occur inside frontmatter multiline values.
The body uses [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/); extensions
must be separately versioned and cannot silently change edge extraction.

```markdown
+++
format_version = 1
document_id = "design-note"
revision = "@42"
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
+++
[This task][implementation-task] supports the work described here.

The document explains [this concept][underlying-concept].
Compare [the experiment](./experiment.md).

[implementation-task]: lifecore:entry/81
[underlying-concept]: lifecore:entry/96
```

`format_version`, `document_id`, `type`, and `body_field` are required. `payload`
is a table, empty if omitted. For an existing document, `revision` must agree
with checkout metadata. For a new root it is absent. Split metadata is described
in section 7.3.

The stored payload is `payload` plus the Markdown body under `body_field`.
Declaring that same field inside `payload` is an error. Control metadata,
including `edges`, is not injected into the payload. Unknown control fields are
errors. The type declaration must accept the resulting payload.

Relationships that should not appear in prose can be written in change files;
they remain visible as external or unowned connections in collection inspection.
The first format does not require dummy prose links to expose such connections.

Physical line endings normalize to LF. Body text otherwise remains exact,
including whitespace and reference definitions. Reformatting TOML without
changing parsed values is a no-op; reformatting the Markdown body can change its
stored string even when rendering looks the same. No formatter runs implicitly.

### 4.3 Links and relationship annotations

An unannotated Markdown link to another managed document or a `lifecore:entry`
node proposes an outgoing `References` edge. That label must be permitted by
both endpoint types like any other label.

`[edges.<label>]` annotates a Markdown reference label. It accepts:

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
normalization. Colliding frontmatter labels or duplicate normalized reference
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
7. Emit the TOML plan and a frozen collection manifest for inspection/application.

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
value_profile = "toml-basic-v1"
database = "personal"
base = 184
plan_id = "revise-b-then-a-001"

[[entries]]
name = "b"
kind = "node"
type = "Document"
predecessor = "@73"
intent = "revise"
payload = { title = "B", body = "Updated B.\n" }

[[entries]]
name = "a"
kind = "node"
type = "Document"
predecessor = "@42"
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
omit `revision`, and specify `predecessor = "@42"` with `intent = "branch"` in
frontmatter. These two fields are otherwise absent from ordinary checked-out
documents. The old document's departure from current collection membership must
be acknowledged as part of the split, not treated as unexplained deletion.

The resulting plan appends one successor per document. Connections must be
assigned to a successor, both successors, or neither. Any unchanged incoming
file link to the old document is unresolved until edited. No alias, path, or
document identity automatically selects one leaf or broadcasts to every leaf.

After success, writeback records each new revision and removes the draft split
fields. Core transition permissions still apply to every successor, including
branches from historical predecessors.

## 8. Robust application and recovery

### 8.1 Receipts and atomic publication

Persist the frozen plan identity, check context, and assigned operation IDs in
the same atomic publication as the graph. A workspace-only journal written after
commit cannot close the crash window. Receipts are auxiliary persistence
metadata, not graph entries.

A failure before publication leaves the previous graph intact. A failure after
publication but before acknowledgment is recovered through the durable receipt.
Retrying an identical completed plan returns its receipt without appending again;
reusing its identity with different content is an error. Verify identity/content
and look up the receipt before checking the original expected base, since a
successful application has itself advanced that base. Safe retries and receipts
are implemented with TOML authoring in Stage 5.

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
metadata, plan-local URI spellings, and explicitly planned path changes. If a
file changed concurrently, present the frozen, committed, and working versions
for reconciliation instead of overwriting it.

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

## 9. Robustness scenarios to verify

These are behavioral acceptance scenarios for an implementation, not additional
exceptions to the core contract.

| Scenario | Required outcome |
| --- | --- |
| Timestamp has more than nine fractional digits or second `60` | Reject before parser truncation or Jiff leap-second clamping can erase the evidence. |
| Offset conversion crosses the supported UTC range, or export receives an out-of-profile core timestamp | Reject explicitly; no string fallback, rounding, or clamping. |
| Nanosecond timestamp is exported and imported with a different offset spelling | Preserve the exact instant and equality; spelling need not survive. |
| Quoted timestamp text or local datetime appears in a payload | Keep quoted text as text; reject the local datetime without assuming a timezone. |
| Database identity differs despite matching IDs/count | Reject before mutation. |
| Head advances between planning and apply | Reject stale base; require refresh/replan. |
| Writer advances the base during checking | Reject under writer exclusion before publishing any operation. |
| Analyzer changes, disappears, fails, or receives a new evaluation time | Invalidate the old check; never silently skip it. |
| Schema migration is valid only at the final view | Reject the first invalid prefix. |
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
| Payloads of independent new roots are identical | Apply the core duplicate rule; do not invent identity fields. |
| Crash occurs after commit but before client acknowledgment | Recover the operation receipt; do not append twice. |
| Plan identity is reused with changed content | Reject the conflicting identity, even if the earlier plan committed. |
| User edits a file during apply or writeback | Preserve newer bytes and expose the synchronization conflict. |
| Alias URI writeback is followed by an unchanged collection check | Produce no revision; use the recorded normalized checkout baseline. |
| Old document contains a renamed path or plan-local alias | Resolve from retained view-specific information, never today's files. |
| Authoring metadata is lost or incompatible | Preserve core readability; block unsafe collection reconciliation. |
| Check is run twice with identical view/context/inputs | Produce the same operations, resolutions, ordering, and diagnostics. |

## 10. Implementation boundaries

The first implementation should share one ordered-plan representation and
validator between handwritten TOML, the collection planner, and any graphical
review interface. CLI, editor, and local web interfaces may inspect the same
plan and bounded views. None should have a separate unchecked write path.

The current implementation round covers TOML parsing and explicit append plans,
check/apply with durable receipts, and bounded inspection. Document checkout,
ordinary and typed/incoming links, and authority reconciliation are backlogged.
When collection work resumes, collection mutation should not ship with silent
fallback behavior for unfinished recovery cases.

This proposal deliberately leaves the storage engine, concrete schema language,
workspace/manifest physical encoding, and UI toolkit open. Those choices must
implement the contracts above. Optional future conveniences include patch-based
draft generators, interactive ordering, and merge assistance. They must not change exact endpoints, invent an automatic
winner after a split, or hide invalid intermediate views.
