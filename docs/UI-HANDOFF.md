# UI discussion handoff

Status: notes from design discussions, not a contract. Source: Claude Code
session `e7342e3d-577b-40d7-9314-24cb169aadd2`, 2026-09-28 to 2026-09-30. No
frontend code exists yet. The backend's first slice (PLAN §4.0–4.1) and the
accepted checking-cost fixes (PLAN §5) are merged at `b20673d`.

The owning contracts remain [INTERFACES §5](INTERFACES.md#5-react-client)
for the client, [§5.1](INTERFACES.md#51-frontend-build-and-tests) for its build
and tests, [§3.1](INTERFACES.md#31-provisional-first-slice-methods) for the RPC
methods, and [§3.2](INTERFACES.md#32-server-access) for server access. The work
lives in [PLAN §4.2](PLAN.md#42-foundation) (the foundation) and
[§4.3](PLAN.md#43-iterative-ui-stages) (UI stages that alternate UI and API
work). Server access, the stateless server, complete results, the toolchain, and
the staging are already recorded there. The UI decisions below are not: each
stage moves its own into INTERFACES §5, so this file shrinks until it can be
deleted.

## Decided

- **List/table-centric UI.** Tables are the default display.
- **Threaded display for DAG-shaped views.** Show them like a mailing list,
  with repeated nodes color-coded. The details still need thought; see below.
- **Filtering creates the views that matter.** Examples: filter by types, or
  filter by types and also show any node connected to the resulting subgraph.
- **Time is secondary.** No global timeline; "as of" is one parameter of a
  view, defaulting to the current state. Comparing two boundaries is a
  secondary action.
- **Graphviz on demand only.** Because the app is local, it may call
  Graphviz for a tidy graph view, but never as the default view, since
  layout can be expensive.
- **Manual Check button.** No live, debounced checking. Checking is now cheap
  enough for interactive use ([PLAN §5](PLAN.md#5-checking-cost)), but checks stay
  explicit.
- **Utilitarian look.** Mouse-first; keyboard-driven use can come later.
- **Hand-written TypeScript contracts**, guarded by fixtures a Rust test emits
  ([INTERFACES §5](INTERFACES.md#5-react-client) has the details).
- **Stateless server.** Each request runs the service on a blocking task, like
  a non-`serve` CLI invocation. Caching is deferred.
- **RPC methods grow with the frontend.** Note the ones we expect to need;
  add more as work progresses.
- **Server access by printed URL.** `lifecore serve` prints a URL whose fragment
  carries a per-launch token; the client sends it on every call, and the server
  checks `Host` and `Origin` ([INTERFACES §3.2](INTERFACES.md#32-server-access)).
- **Complete results.** No limits or paging in the prototype ("you get what you
  sow"); both are backlogged.
- **Staged delivery.** No big RPC freeze. Each UI stage sketches screens against
  existing methods, adds or reshapes the methods they need, then finishes the
  screens ([PLAN §4.3](PLAN.md#43-iterative-ui-stages)).
- **Node toolchain.** npm, esbuild, `tsc`, Biome, `node --test`, and Playwright
  Test in `lifecore-server/ui`; `build.rs` embeds the bundle, and
  `LIFECORE_UI=skip` embeds a placeholder
  ([INTERFACES §5.1](INTERFACES.md#51-frontend-build-and-tests)). Deno was
  considered, but Playwright proper needs Node.
- **Firefox first.** Browser tests run in Firefox, the browser in daily use and
  the likelier source of compatibility trouble. Chromium tests are backlogged.

## Views as filter pipelines (proposal)

A view is a history boundary (default: current) plus a pipeline the backend
evaluates in one request:

1. **Seed:** select nodes by one or more types, field conditions, text, and
   current/historical. Several seeds can be combined.
2. **Expand**, any number of times: follow edges of chosen types (or any),
   outgoing, incoming, or both, for *n* hops or to a fixpoint. "Any node
   connected to the resulting subgraph" is one hop, both directions, any edge
   type.
3. **Refine:** filter the result again, e.g. exclude a type or subtract
   another selection.
4. **Edges:** keep edges among the resulting nodes, optionally limited by type.

Each result node records why it is included: it was a seed, or it was reached
via edge X from node Y. The table shows that as a "via" column.

The pipeline is structured JSON, not a text query language; general query
languages stay in the backlog. Results are complete; limits and paging are
backlogged. Building blocks that already exist: `inspection::NodeSelection`
(one type, currentness, field present/absent/equal, text), `EdgeSelection`,
`Direction`, relationship traversal, and DagCheck's `SubgraphFilter`
(node and edge type sets).

## Displays of one view (proposal)

- **Table (default).** Rows grouped by type, with columns from the type's
  fields plus currentness, findings, and the "via" provenance.
- **Thread.** The user picks which edge types, in which direction, define
  parent → child. Roots are nodes with no parent inside the view.
  - A node with *k* parents appears *k* times. Only repeated nodes get a
    color stripe, the same on every occurrence; hovering one highlights all.
  - The first occurrence is expanded; later ones collapse to one line saying
    "also under …".
  - A cycle is cut at the first revisit on the current path, with a marker.
  - A node's own revision tree can reuse this display.
- **Graphviz.** A "Render" action for the current view. The service returns
  DOT text; the core already has `to_dot`. The HTTP adapter runs
  `dot -Tsvg` as a subprocess with a timeout and a size cap, so a slow layout
  never ties up the request. A missing Graphviz gives a clear error.

## Other screens (proposed; accepted as broadly reasonable)

- **Entry inspector:** payload rendered by its type declaration; lineage as
  a small revision tree with splits as forks and current leaves highlighted;
  edges per revision, stale ones greyed. "Revise" and "Branch" add a draft
  operation.
- **Draft tray:** a persistent staging area.
  - Ordered, named, reorderable operations; references shown as chips
    (exact entry, or "added: name").
  - After Check, each operation shows whether history up to it is valid;
    findings sit on the operation that introduced them.
  - A toggle shows the draft as TOML/JSON via the backend's own export.
  - A stale base shows a banner with an explicit "replan" action.
  - Drafts persist in browser storage (IndexedDB).
- **Reconnection assistant.** Revising a node makes its edges stale, and the
  rules forbid implicit reconnection. List the node's current incident edges
  with "carry over" checkboxes, each producing an explicit edge operation.
  For splits, assign each edge to one successor, both, or neither.
- **Schema-driven forms:** editors generated from type declarations, with
  presence badges (mandatory, encouraged, deprecated, optional). Integers
  stay decimal strings; timestamps keep full precision, with a date picker
  only as a helper.
- **Findings panel:** backend-provided groups by severity; current/history
  toggle; "historical, applicability unconfirmed" on sticky items; dismiss
  with reason and note, and restore; dismissing an inconsistency is disabled
  with an explanation; an evaluation-context chip. Suggestions preview their
  ready-made proposal (PLAN §4.4).
- **Schema manager and analyzer page:** type declarations as of a boundary;
  a helper for the introduce/retire migration paths; the analyzer suite, with
  `add-analyzer` behind a confirmation that shows any rejection.

## Candidate RPC methods (open list)

Existing, provisional: `entries.list`, `entries.get`, and the others in
[INTERFACES §3.1](INTERFACES.md#31-provisional-first-slice-methods).
Expected additions:

| Method | Purpose |
| --- | --- |
| `views.query` | Filter pipeline → nodes, edges, per-node provenance. |
| `entries.lineage` | A node's revision tree. |
| `entries.relationships` | Incident edges with currentness at a boundary. |
| `types.list` | Type declarations as of a boundary. |
| `views.compare` | Resolution differences between two boundaries. |
| `views.dot` | DOT text for a view. |
| Analysis and dismissal methods | Planned for the Findings stage ([PLAN §4.3.4](PLAN.md#433-findings)). |

Nothing is frozen. Write client types only after the foundation aligns the
core JSON names with the proposal vocabulary
([PLAN §4.2](PLAN.md#42-foundation)).

## Backend behavior the client must handle

These follow from the merged backend; the linked documents own the details.

- **Access.** Without a valid token, `/rpc` fails before dispatch. Show how to
  open the URL printed by `lifecore serve`; a server restart requires the new URL.
- **Check versus apply.** `proposal.check` returns `{accepted, report}` even when
  rejected. `proposal.apply` and `analyzers.add` report rejection as error 2006
  `rejected`, with the complete report in `data.report`
  ([INTERFACES §3.1](INTERFACES.md#31-provisional-first-slice-methods)).
- **Rejected previews have no candidate graph.** A rejection keeps the complete
  analysis and the operation-to-entry mapping, nothing browsable
  ([SPEC §7.4](SPEC.md#74-failure-and-persistence)). The draft tray
  attaches findings to operations through `report.operations`, which maps
  operation names to provisional entry IDs.
- **Stale bases.** A stale plan ID fails with 2005 `stale_base`. Keep the draft
  and offer deliberate replanning; never refresh the token silently.
- **Uncertain publication.** Error 1008 carries the would-be result in
  `data.outcome`. Say the change may have been applied, then re-read state.
- **Busy writers.** Another writer holding the lock yields a busy error after
  about 10 s of retrying.
- **Suggestions.** Until PLAN §4.4, a proposal with `acknowledges` fails with
  `acknowledgment_unsupported`.
- **External writes.** CLI writes do not reach an open page; a refresh
  re-reads state. Automatic change detection is
  [backlogged](PLAN.md#other-deferred-work): for a single user, a refresh solves
  most cases.
- **Evaluation time.** Absent by default. With `now`, the host captures one
  instant and reports it in `analysis.inputs`; show it with every analysis.
- **Findings.** Current reports always include inconsistencies. TypeCheck
  warnings describe current state and are nonsticky, so they leave current
  reports once fixed; a revision (transition) finding appears once, at its own
  prefix ([SPEC §8.3](SPEC.md#83-presentation-and-dismissal)).
- **Identities and numbers.** Key records by `origin` plus `position`;
  positions and integer payloads are decimal strings.
- **JSON vocabulary is not final.** Entry records use the proposal vocabulary
  (`kind`, `type`, `payload`), but analysis output still uses core names
  (`{"Node": …}`, `ty_name`). Write client types only after the PLAN §4.2
  alignment.
- **Type declarations.** They are written by hand as well as by forms: a field
  is a list of presence and shape words
  ([FILES §2.6](FILES.md#26-type-declaration-payloads)), and the schema manager
  generates the same encoding.
- **Cost.** At 2,000 entries, loading takes about 0.03 s and checking one new
  node about 0.25 s; at 4,000, about 1 s. The manual Check button is fine at
  this scale.

## Stack

Decided: React with TypeScript, hand-written contracts, and the Node toolchain above;
the client starts with React and ReactDOM only. Candidates to add when a stage
needs them: TanStack Query for RPC calls, a reducer for draft state persisted to
IndexedDB, Radix primitives for accessible widgets, and CodeMirror 6 for editing
imported TOML/JSON. An interactive graph library (e.g. React Flow with elkjs) is
not needed while Graphviz covers tidy graph views. Vite is not used.

## Open questions

Each is settled in the stage that first needs it.

- **Row labels (Browse):** a convention (a `title` field, falling back to the
  first text field), or a per-type display setting?
- **Saved views (Views):** client-side only (browser storage), or stored in the
  database snapshot like dismissals, so the CLI and other browsers see them?
- **Thread display details (Views):** sibling order (append order or a field);
  collapsing large fan-outs; more repeated nodes than distinguishable colors
  (reuse colors with a number label?); which field labels a row.

## Stage mapping

The former suggested order now lives in PLAN as stages: the foundation
([§4.2](PLAN.md#42-foundation)), then Browse, Authoring (including the draft
tray, schema-driven forms, and the reconnection assistant), Findings, and Views
([§4.3](PLAN.md#43-iterative-ui-stages)), then Suggestions
([§4.4](PLAN.md#44-suggestions-and-acknowledgment)). Authoring precedes Findings
because a check preview needs only a simple findings list; the full findings
panel and dismissals follow. Views, including Graphviz rendering, comes last as
the largest and least settled stage.
