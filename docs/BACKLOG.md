# Backlogged authoring contracts

Status: contracts for Markdown document collections and durable application
receipts, retained for a later round. [PLAN §6](PLAN.md#6-backlog) lists them as
backlog; nothing here is a prerequisite for the web application. They build on
the shared proposal encoding in [FILES](FILES.md), which takes precedence where
the two overlap, and [SPEC](SPEC.md) governs graph semantics. Compatibility is
not a concern, so these sections will be revised together with their
implementation rather than treated as fixed.

Sections 1–4 describe collections: checkout state, the Markdown format with
embedded TOML metadata, links as relationship annotations, relationship
authority, compiling a collection to a change file, and ordering. Sections 5 and
6 cover source writeback and historical rendering. Section 7 covers durable
receipts and retry recovery. Section 8 lists the acceptance scenarios.

## 1. Document collections

### 1.1 Checkout state and document identity

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

### 1.2 Markdown with TOML embedded in YAML frontmatter

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

Inside the decoded TOML, `document_id`, `type`, and `body_field` are required. `payload` is a table, empty if omitted. For an existing document,
`revision` must agree with checkout metadata. For a new root it is absent.
Split metadata is described in section 4.3.

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

### 1.3 Links and relationship annotations

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

### 1.4 Destinations

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

## 2. Relationship authority and database agreement

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

## 3. Compiling a collection to a change file

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

## 4. Ordering and graph changes

### 4.1 Editing both ends of a link

Suppose current A links to current B. The collection edits B and removes A's
link to it. A possible plan is:

```toml
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

### 4.2 Retaining links and removing edges

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

### 4.3 Splits

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

## 5. Source edits and file writeback

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

## 6. Historical rendering and integrity

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

## 7. Deferred receipts and retry recovery

This future application-service work is outside the current round. It preserves
the recovery requirements for a later implementation; the
[optional token](FILES.md#optional-structured-plan-identity) does not claim to
implement them. Plans participating in retry recovery would
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

## 8. Acceptance scenarios

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
