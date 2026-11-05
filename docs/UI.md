# Browser client decisions

Status: presentation choices of the browser client, recorded so the code can
cite them; none is a server contract. [CLIENT](CLIENT.md) owns the client's
contract with the service.

## 1. Identity, labels, and boundaries

- Row label: a node's `title` field when it is text, else `name` when text,
  else the text field whose name sorts first by code point, the byte order
  `views.render` labels by, else `<ty_name> @<position>`. Blank text does not
  count. An empty type name reads "(unnamed)". Labels are cut at 60
  characters (30 in relationship cells and seed chips, 40 in sidebar places,
  28 in the trail), with the full text as a tooltip.
- `@position` is an entry's on-screen identity, shown beside a field label; the
  origin UUID is never shown.
- Badges call a node current or superseded and an edge current or historical.
  Lists show current entries unless asked for historical or all.
- As-of is one global boundary, set in the top bar up to the visible end or by
  an inspector's "As of here" just after its entry, and not part of Back
  history; another database answering clears it, as does a description of
  this one ending before it, as after a restore. Browsing reads send it as
  `as_of`; `database.describe` (the Database screen) and reconnection read the
  whole history. The top bar shows `showing the first N of M entries, view
  [0, N)`; tables and the inspector show the `view` their response names,
  with the same words as a tooltip.
- Nothing learned at a wider boundary names an entry past the as-of boundary:
  the trail, seed chips, and sidebar places name it by `@position` alone.
- A failed read shows a heading and a sentence for the error kinds a reader
  meets (an entry not in the view, a boundary past the end, refused query
  seeds), else the server's message; the method, code, kind, and
  data wait behind "Details".

## 2. Navigation and places

- Navigation is in memory; a page reload loses Back history and as-of. Back
  pops a stack of steps; reopening the current screen adds none. A trail under
  the top bar names the current step and the six before it, older ones folded
  into "…"; a click returns to that step.
- A step remembers its screen's state: a table's currentness, text,
  relationship toggle, sort, open cells, and rows shown; the inspector's
  expanded outline rows, toggled groups, historical toggles, filters, and raw
  JSON; Explore's form, query, and display choices. Returning restores the main
  pane's scroll offset once the content is tall enough, unless the user
  scrolls, clicks, or types there first.
- The last result of each call is cached (40 calls), so a screen returned to
  renders at once, reads again, and shows "reloading…" until the answer. An
  answer with the same JSON as the cached one keeps the cached object. A
  reload shows the previous answer only if it was read at the same or a
  narrower boundary, so lowering as-of shows "loading…" until the bounded
  answer. The cache and the labels noted from reads hold one database and
  start afresh when another answers. Explore stays mounted under the other
  screens, so its results, drawings, or failure survive a detour; hidden, it
  reads nothing, and on return it reads again at the boundary then selected.
- Places, per database in `localStorage` under `lifecore.places.<database>`,
  are listed in the sidebar below Views: Pinned entries (☆/★ in the inspector
  header), Names (below), Saved views (the Explore form under a name,
  replacing one of that name; opening one restores the form and runs it), and
  Recent (the last 15 inspected entries, most recent first, without repeats).
  An entry keeps the label and type of its last inspection; an edge reads
  `from —Type→ to`.
  Entries past the as-of boundary are dimmed and named by `@position` alone,
  without their type. Another tab's change arrives through the `storage`
  event. Malformed stored items are dropped one by one.
- Names are kept with the places, in name order. Each follows the operation
  name grammar and names one entry; an entry may have several. The tab whose
  Apply succeeds, or whose user decides an uncertain apply's entries are
  present, records them in operation order: a revise passes its
  predecessor's names on, since it is that node's only successor; a branch
  passes none, since its predecessor may have other current descendants; a
  kept edge that still keeps its edge passes that edge's names on and has
  none of its own; then every other operation's name names its entry, taken
  from any entry that held it, which the operation screen warns of. Every
  `@position` is followed by the entry's names, except past the as-of
  boundary. The sidebar's Names open their entry and × forgets one; past the
  boundary a name stays, dimmed, with its position and no type. The inspector
  renames, forgets, and adds names, refusing a name already held, by this
  entry or another. A superseded node with names, read without a boundary,
  offers to move them to one of its current descendants. Follow revisions, in
  the sidebar, reads each named node of the current state and moves the names
  of each superseded one with a single current descendant, listing those with
  several.

## 3. Tables and relationships

- Table columns: label, `@position`, current (in a type table of historical or
  all rows, or an Explore table with a superseded row), Explore's via, the
  relationship columns, the declared fields by status (mandatory, encouraged,
  optional, deprecated) and name, then undeclared fields present in the rows,
  marked. The field that labels every row has no column. The `Type` table
  shows a declaration summary instead of field columns, from the server's
  parse of each declaration `types.list` resolves, else from the client's.
  The client words schema errors itself, from their path and kind, for its
  own parse and the server's alike.
- A table renders 500 rows, then "show next 500" and "show all" with the count
  of the rest. Cells render on one line (`[a, b]`, `{owner: me}`, `true`), cut
  at 80 characters with the full text as a tooltip. A duration, date, or
  offset timestamp shows its canonical string, and a variant its tag before
  its body (`committed {cost: PT2H}`). Integers sort by value, timestamps
  and offset timestamps by instant at full precision (one instant at two
  offsets ties), durations by length, everything else, dates included, as
  text; absent cells sort last ascending. A wide table scrolls sideways.
- A declared shape reads as words: `list of text`, `record {owner: text}`,
  `enum day | week`, `variant committed {cost: duration} | resumed {}`.
- A type table sends its text filter as the `text` of its node seed, 300 ms
  after the last keystroke; Explore sends its own on Run. The Findings and
  outline filters run in the client.
- Relationship columns: a type table reads its rows with `views.query`, the
  rows as node seeds and, while the columns are on, one hop both ways over
  edges of the rows' currentness (`all` for historical or all rows), keeping
  only the seeds; with them off, it sends no expansion. Each edge type
  and direction among the rows' incident edges gets a column, outgoing first
  and then by name, headed `→ PartOf` or `← DependsOn`. A cell lists the
  neighbours' labels and sorts by the first. Beyond three, it starts with the
  count by type (`460 Tasks:`) and shows three labels, then "+N more" in place
  up to 20, or above 20 "+N in Explore", a query of the row over that type and
  direction. A "relationships" checkbox, on by default, shows the columns.
  Explore's tables show the same columns over the result's edges. Label, via,
  and relationship cells stay on one line until expanded.
- Inspector relationships follow the header, beside the payload and lineage
  once the pane is 72rem wide: an outline of groups by edge type and direction,
  ordered as the relationship columns, with counts and the neighbours' types
  (`← PartOf 460 · Task`), collapsed and filterable by label above 20 edges. A
  neighbour's ▸ reads that node with `entries.inspect` at the inspected view's
  boundary and nests its groups, to any depth; a node already on the path is
  marked "(on this path)" and does not expand. Historical edges wait behind a
  per-group toggle, except under a superseded node, all of whose edges are
  historical.
- A `Type` node's inspector shows the parsed schema, then the full payload
  table, so `name` and fields outside the grammar stay visible. As in the
  `Type` table, the schema is the server's reading when the boundary's
  `types.list` covers this node (the declaration a name resolves to, a
  malformed one, or any claimant of an ambiguous name), else a client-side
  reading; an unreadable declaration shows its error instead.

## 4. Explore, threads, and drawings

- The client bounds an Explore query to 3 expansion steps of up to 3 hops
  each; the server imposes none. Its node selection takes any type or one
  type, the unnamed one included.
- Each query run is a Back step: Run, a row's "focus" (the node as the only
  seed, node selection off, steps and edge filter kept) or "+ seed", a saved
  view, "+N in Explore", or "Explore from here", which seeds the node alone
  with the default form (one step of 1 hop both ways over current edges of any
  type). Back to a step shows its own query. Editing the form, or rerunning
  the query on show, stays on the step; a form that no longer matches the
  query on show says so until Run. A query started outside Explore keeps the
  last step's display choices, except that a graph shows as a table.
- Seed chips and the folded summary name seeds as `label @position`. Arrows
  read from the node in context: `→ PartOf` for an edge from it, `← PartOf` for
  one to it, in columns, the outline, thread prefixes, and via cells. A via
  cell reads `<hop> via → PartOf from <label>`, the arrow linking to the edge,
  or the edge's `@position` when the edge filter dropped it.
- Thread display: one edge type, or all of them, and one direction at a time;
  with outgoing, an edge's `from` is the parent. All types is the default when
  the result has several; each child row then carries the types joining it to
  its parent, read from the child. The default direction leaves fewer roots,
  outgoing on a tie. A node's first occurrence expands; later ones collapse to
  "also under", and a revisit on the current path is cut as a cycle. Nodes
  occurring more than once get one of 8 stripe colors, then a numeric suffix;
  hovering one highlights them all. Row actions show on hover or focus.
- Graph display: nothing is drawn until Render, which calls `views.render` with
  the query on show at its result's boundary. Render sits beside the result's
  node and edge counts, which warn above 200 nodes; the drawing names its view.
  The SVG, from `<svg` on, is inserted as served, with the seeds of the query
  on show outlined thick. A click in a node's shape or on an edge's link opens
  the inspector. DOT fetches the text only while its toggle is on.
- The drawing scrolls in a frame up to 70% of the window high, zoomed by a CSS
  transform from 1% to 400%: fit (at most 100%; below 5% it suggests 100% and
  scrolling), 100%, and ×1.25 or ÷1.25 steps keeping the middle in place. Its
  natural size comes from the SVG's `width` and `height`, else its `viewBox`.
  A new drawing starts fitted but at least at 60%, centered on the first seed
  it shows, once its frame is visible.
- While Explore shows a result, the last drawing of each of 6 query-and-view
  pairs keeps its zoom and scroll, so Back to a drawn query shows it. Another
  query shows the latest drawing, marked as drawn for another query or view.

## 5. Drafts and writes

- Drafts: one per database in `localStorage` under
  `lifecore.draft.<database>`, held under the last database ID that loaded, so
  a failed or reloading description neither empties nor remounts it. No edit
  lands before the first description answers. The proposal, its kept-edge
  provenance, an uncertain-apply record, and an apply-in-flight marker are
  saved synchronously on every change; check results and editor state are not,
  so a restored draft needs a new check. An unreadable stored draft is dropped,
  as is one whose uncertain record holds an applied outcome without its
  report's operations, base, view, and observations.
- Tabs of one database share its draft. Each re-reads the stored record on
  another tab's `storage` event and adopts it, keeping its own selection,
  pick, and check where they still apply. A tab never saves over a record
  carrying an in-flight or uncertain marker it did not hold: the change is
  dropped with a notice and that record adopted, and an Apply whose marker
  could not be saved sends nothing. Another tab's apply in flight freezes the
  draft as an own one does until that tab saves the answer; if that tab went
  away first, a reload restores the draft as uncertain. An adopted uncertain
  record spends the adopting tab's check too. While its own apply is in
  flight, a tab writes its record again over one without markers.
- A plan ID is minted whenever a non-empty draft lacks one: at its first
  operation, before Check if a mint failed, and for operations an Apply left
  behind. Otherwise only an explicit Replan mints one, clearing the check. A
  replacing import takes the file's plan ID or a fresh one; an appending import
  keeps the draft's.
- Operation names default to `<ty_name lowercased>_<n>` with the smallest `n`
  free among the draft's operations and the kept names, characters outside
  the name grammar replaced by `_`. Names identify
  operations in the client, so a rename to a name in use is refused; renames
  update `added` references, kept-edge provenance, and the last check's
  findings. Imported operations get a `_<n>` suffix on a name collision; a
  proposal that acknowledges a finding is not imported.
- Removing an operation unsets every reference to it, a predecessor included:
  the node keeps its intent, and Check waits until it is picked again. A
  notice, kept until dismissed, names the affected operations and offers to
  remove the edges left without an end. Clear, Remove, that offer, and
  Reconnection's "Keep none" take a second click; Remove then goes Back. A
  move that puts a reference before its target posts a notice.
- Picking: an editor's pick leaves a pending role (predecessor, from, or to).
  While it is pending, every node inspector offers "Use as <role>", which sets
  an `existing` reference and returns to the operation; edges cannot be picked.
  Connect starts an edge from the inspected node and picks its `to`. A draft
  with an endpoint or predecessor unpicked is neither sent nor exported.
- Revise and Branch copy the node's payload. A revise is named after the node's
  first kept name that the draft does not use. A revise's predecessor and an
  edge's ends must be current
  ([FILES §2.4](FILES.md#24-node-and-edge-operations)), so Revise, Connect, and
  "Use as" an edge's end or a revise's predecessor (a predecessor picked for an
  operation without an intent makes it a revise) are off for a superseded node
  and, under an as-of boundary, for every node, since a bounded read does not
  say what is current in the state drafts apply to; they also wait while an
  earlier answer stands in for a reload. Branch and a branch's predecessor pick
  stay: a branch may start from any revision, whose payload is the same at every
  boundary. Reconnection lists the predecessor's current edges, read without
  `as_of`, by direction and type, collapsed beyond 10. Revise keeps all of them
  by default, also for a picked predecessor; Branch keeps none. Each kept edge
  becomes an explicit edge operation, `<operation>_keep_<edge position>`, placed
  after the revision ([SPEC §5](SPEC.md#5-current-and-historical-data)). Its
  other end is the revision itself for a self-loop, else the latest earlier
  revise successor of the other node in the draft, else that node. An edge whose
  end the draft supersedes, or whose revision no longer revises the node it was
  kept from, carries a warning. Edges that arrive follow their operation through
  renames; those arriving after it was removed or changed are not added, nor are
  edges already kept.
- Check is manual. Apply is enabled only after an accepted, unspent check of
  identical proposal JSON, plan ID included, with the same evaluation choice.
  It sends the evaluation time that check reported, so `now` applies at the
  instant and offset the check captured; the tray shows which. An explicit
  evaluation time gets a hint naming the RFC 3339 form with a numeric offset
  while what is typed does not look like it, and says why `Z` or `-00:00`
  will be refused; the server parses it. While an apply is in flight the draft
  is frozen: no edit, pick, import, or Clear lands. Drafts always apply to
  the current state; under an as-of boundary the tray says so.
- Kinds without an editor: the form edits text, integers, booleans,
  timestamps, lists of those, and records; the other value kinds and shapes
  (duration, date, offset timestamp, enum, and variant, until their editors
  arrive, and any kind added later) are shown, not edited. Such a field's value
  is a read-only input holding its JSON as the draft has it (canonical once
  stored; time text typed in the raw JSON stays as typed until then),
  labelled `<field> (read-only)`, with a note to edit it in the raw JSON; its
  presence checkbox is disabled, and adding it by name is refused with the
  same advice. A list whose
  element has no editor is read-only as a whole, while a record stays
  editable field by field. In the declaration form, a field whose shape the
  builder does not offer keeps its name, presence, and removal, and shows the
  shape's words read-only the same way; the builder's select offers only the
  editable words. The raw JSON reaches every shape, so nothing is out of
  reach, and a value of another kind than declared still falls back to an
  editable JSON field.
- A successful Apply removes the operations it carried, matched by name and
  content, turns references to them into the assigned IDs, records their
  names, lists those IDs and the report's finding counts until dismissed, and
  reloads every screen.
- Write failures (apply, analyzer add, dismissal edit): envelope errors,
  structured HTTP rejections, and the kinds a method reports before publishing
  change nothing. Those kinds are `invalid_params`, `busy`, `lock_failed`,
  `database_missing`, `database_unreadable`, and `publication_failed` for all
  three; `local_time_unknown` for apply and analyzer add; `rejected` and `analyzer_failure` for apply and analyzer add;
  `malformed_proposal`, `invalid_proposal`, `acknowledgment_unsupported`, and
  `stale_base` for apply; `foreign_database` for apply and dismissal; and
  `cannot_dismiss_inconsistency` for dismissal. Any other failure may follow a
  publication: `publication_uncertain`, `backend_failure`, `internal`, an
  unknown kind, an unstructured HTTP refusal, or a lost or unreadable answer.
  None is retried ([INTERFACES §1](INTERFACES.md#1-shared-synchronous-application)).
- An apply failure that changed nothing keeps the draft and its check unspent:
  `rejected` shows the apply's report in place of the check, `stale_base` keeps
  Apply off until Replan, and `busy` offers a retry.
- Any other apply failure spends the check and stores an uncertain record with
  the draft: plan ID, request JSON, the request's kept-edge provenance as sent,
  the check's base revision and end, error kind and message, and
  `data.outcome` when present. An apply still in flight when the page went
  away restores as uncertain. A banner then stays above the
  others, with Apply and Replan off, until the user decides that the entries
  are present (offered when the outcome, or a look at the positions the request
  would have taken, shows them), which removes the operations the request
  carried as a successful Apply does, recording names by the provenance sent
  rather than the draft's as edited since, or that the draft was verified not
  applied, or clears the draft. A stale base does not show that an earlier apply was
  published: any writer moves the revision.
- The tray is a column: a fixed header with the status, Check, Apply and the
  evaluation it will use, the evaluation choice, the draft-wide buttons, and
  every banner, above a scrolling list of one-line operations with their
  provisional position `@<base + index>`. A revision's kept edges collapse into
  one line under it. An idle tray (an empty draft and nothing to report) folds
  to a strip until an operation arrives or the strip is clicked; a fold or
  unfold by the user holds until the tray becomes idle or stops being idle.
- A check finding belongs to the operation whose entry is the newest in the
  first prefix it was emitted at, matched by position against the report's
  operations and kept by operation index, so renames keep it. Findings from a
  check of other content are dimmed. Each shows its range against the
  candidate's head, with "not emitted at the head" when it ended before;
  nothing reads as resolved. Initialization findings are suite-level;
  provisional subjects link to their operations.
- "Already in history" covers findings first emitted in the committed base. Its
  inconsistencies are always listed, those the base no longer emits included;
  advisories still emitted at the base fold into one line with counts per kind
  until expanded, and those no longer emitted there are only counted.

## 6. Findings and analyzers

- One finding row everywhere: kind, message, analyzer name and version (the
  configuration string as a tooltip), range, the focus, and the subjects other
  than the focus. Ranges read against the boundary of their report: `since N`
  when they run up to it, `N–M` or `at N` when they ended before it, and
  `initialization`. Runs of the stored database mark an ended range "(ended)",
  and reports of a check or a replay "not emitted at the head". A sticky
  advisory that ended before the boundary is labeled "historical;
  applicability unconfirmed" instead. The "sticky" mark and that label each
  have a tooltip saying what they do not claim.
- The Findings screen runs `analysis.run` at the top bar's as-of only on Run,
  which always asks the server again. The last run stays on show with its
  view, evaluation time, and analyzers with versions and configuration strings,
  and says so when the top bar selects another view (a whole-history run of
  `[0, N)` is the view as of N) or a reload has moved on. A failed Run shows
  its failure, and over a previous run of the same parameters says that the
  findings on show are that run's. The page keeps 4 runs and never evicts the
  one on show.
- Groups follow the core's grouping and current or history selection
  ([ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal)). Category,
  "show dismissed", and text (message or focus label) filters apply to
  advisories only; counts per category are taken before them. Groups with one
  analyzer identity, kind, and message cluster into one line: the message, the
  analyzer when the suite has several, the count, the first three distinct
  foci (keys without one are skipped), and "+N more findings" for the keys
  none of them names, which expands to a row per key. A cluster whose keys are
  all dismissed says so. History mode dims nonsticky advisories that ended
  before the boundary. Inconsistencies are never dimmed or filtered out, and
  their clusters offer no "Dismiss all".
- Labels are read at the run's boundary, only for the entries findings name:
  `entries.get` for each below 20, else one `views.query` of `ids` seeds,
  asked once more without every id its `invalid_seeds` refusal names.
- Dismiss takes a reason and an optional note; an inconsistency shows it
  disabled, with the reason. "Dismiss all N" on an advisory cluster takes them
  once and dismisses its undismissed keys with progress and Stop; a key's
  failure stays on its row. Edits go one at a time, and the records each answer
  lists replace the run's without a new run. A form opened on a record closes
  when an answer or a newer run changes that record, so it never sends values
  read before.
- A dismissal failure that changed nothing shows on its row.
  `publication_uncertain` with a `dismissed` or `restored` outcome adopts its
  records and labels the row "written; durability unconfirmed". Any other
  failure puts a banner over the Findings screen and the inspector section ("A
  dismissal edit may have been stored; Run again to see the stored records")
  until an edit answers or a run requested after it arrives; it ends a
  "Dismiss all".
- Suggestions in runs of the stored database list their actions read-only.
  "Add to draft" appends them as ordinary operations with default names,
  `added: n` becoming the name of the n-th earlier action (any other index is
  left unset), and never sets `acknowledges`. It is enabled only for an
  observation emitted at the boundary of a whole-history run of the current
  reload, since the draft applies to the current history, and otherwise says
  why. Afterwards, while any of those operations remain in the draft, it reads
  "added (N operations)" and asks before adding them again.
- The inspector's "Findings about this entry" sits above the relationships of a
  node and below an edge. It is collapsed until opened and then stays open
  across inspections; collapsed, it counts from a run already held for its
  parameters. One run without an evaluation time per as-of selection and
  reload serves every inspection, reusing a Findings run with the same
  parameters; while the section is open, a changed boundary or reload waits
  300 ms before running; a failed run waits for Run again. It names the
  analyzers that ran and lists the current findings whose focus or subjects
  name the entry, without a focus chip that names it again.
- Analyzers: the Database screen lists the suite with each configuration
  string verbatim and appends only names from `registered`, with a
  configuration (default `{}`) sent as typed. "Add…" asks first, naming the
  analyzer, the configuration, its position in the suite as last read, the
  evaluation time, and the consequences of [INTERFACES §2](INTERFACES.md#2-cli-surface): a
  whole-history replay that any inconsistency rejects, no removal, and stale
  plan IDs, so a non-empty draft will need Replan. The confirmed request is
  sent once.
- A successful add shows the new suite and finding counts and reloads every
  screen. `rejected` lists its inconsistencies in the shared finding row and
  counts the rest; `analyzer_failure` names the failing analyzer and the
  boundary when given; `busy` offers Try again. A failure that may follow
  publication says the analyzer may have been added and keeps Add off until
  Refresh re-reads the suite, which then says whether it holds one more copy of
  the analyzer. The form and the last outcome survive a detour.
