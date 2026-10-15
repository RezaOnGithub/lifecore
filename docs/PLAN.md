# LifeCore Web Application Plan

## 1. Outcome

Build a React browser client served by a Rust HTTP application. Keep graph
operations, analyzers, and the shared application service synchronous. HTTP and
CLI adapters use the same requests, proposal types, results, and errors.

JSON and TOML change files remain first-class authoring inputs. Keep dedicated
change-file commands alongside `init`, `serve`, and a minimal `rpc` command that
reads one request line from stdin, prints one response line to stdout, and exits.
The web server calls the application service directly, one blocking task per
request.

Read [SPEC.md](SPEC.md) for graph semantics, [INTERFACES.md](INTERFACES.md) for
application boundaries and commands, [FILES.md](FILES.md) for authoring and
publication contracts, [SUGGESTIONS.md](SUGGESTIONS.md) for suggestion export and
acknowledgment, and [AGENTS.md](../AGENTS.md) for contributor guidance.
These documents describe the intended application; unchecked boxes below are
remaining work, not claims of implemented behavior.

## 2. Selected direction

Each line names the document that owns the decision; this list is a map, not a
restatement.

- React with TypeScript in the browser, served together with `POST /rpc` by a
  Rust Axum application, for a local user and one selected database
  ([INTERFACES §1](INTERFACES.md#1-shared-synchronous-application),
  [§5](INTERFACES.md#5-react-client)).
- One synchronous application service, shared by HTTP and CLI, owns checking,
  application, storage, and dismissal updates. The HTTP adapter runs each
  request on a blocking task; there is no worker thread, queue, or server-side
  state, and the file lock serializes writers
  ([INTERFACES §1](INTERFACES.md#1-shared-synchronous-application)).
- Shared Serde requests, results, errors, and proposals, with hand-written
  TypeScript types guarded by Rust-emitted fixtures
  ([INTERFACES §3](INTERFACES.md#3-shared-rpc-envelope),
  [§5](INTERFACES.md#5-react-client)).
- A small Node toolchain in `lifecore-server/ui`, bundled by `build.rs` into the
  server binary, with Playwright tests in Firefox
  ([INTERFACES §5.1](INTERFACES.md#51-frontend-build-and-tests)). The UI is
  delivered in stages that alternate UI and API work (section 4.3).
- Loopback binding, a launch token, and `Host`, `Origin`, and content-type
  checks before dispatch ([INTERFACES §3.2](INTERFACES.md#32-server-access)).
- `rpc` processes exactly one JSON request line; `init` stays outside RPC; the
  dedicated `check`, `apply`, `plan-id`, `export`, and `add-analyzer` commands
  are conveniences over the same service
  ([INTERFACES §2](INTERFACES.md#2-cli-surface)).
- Database selection from `LIFECORE_STATE_FILE_PATH` or `./LifeCoreState.json`
  with no fallback; one JSON snapshot, a stable sibling lock file, atomic
  publication, and a graph revision UUID rotated on graph or suite changes
  ([INTERFACES §4](INTERFACES.md#4-database-selection-and-concurrency),
  [FILES §8.1](FILES.md#8-atomic-publication)).
- JSON and TOML change files deserialize into one `Proposal` over the canonical
  value encoding, with an optional service-minted `plan_id`
  ([FILES §2](FILES.md#2-json-and-toml-change-files)).
- Each suggestion observation pairs with a ready-made proposal whose
  `acknowledges` is verified by replay ([SUGGESTIONS](SUGGESTIONS.md)).
- Analysis outputs are ephemeral and recomputed per request; dismissals persist
  independently
  ([INTERFACES §6](INTERFACES.md#6-analysis-lifetime-and-dismissal-persistence)).
- Every adapter preserves exact references, explicit revision intent,
  every-prefix validation, frozen evaluation inputs, and the unfiltered
  inconsistency gate ([SPEC §7](SPEC.md#7-mandatory-validation),
  [§8](SPEC.md#8-analyzers)).

## 3. Decisions before dependent implementation

None are open. RPC method and result shapes are not frozen up front: the UI
stages in section 4.3 add and reshape methods as the screens need them.

## 4. Implementation sequence

Build a working vertical slice before the remaining layers. Exact encodings in
FILES and INTERFACES are provisional until code exercises them; when the
implementation shows a contract is wrong, change the contract and the code
together rather than working around the document.

### 4.0 Preliminary core work

Core API and encoding changes that the first slice builds on.

- [x] Move revision intent into the core: `Change::AddNode` carries
  `Option<RevisionIntent>`, and `transaction::prepare` validates it at each
  prefix (a predecessor requires an intent, a root forbids one, and `revise`
  requires a current predecessor immediately before the append). Give suggested
  actions the same node/edge shapes as `Change`, so acknowledgment matching is
  plain equality (later one type, `EntryChange`); `prepare_suggestion` keeps
  only its source check.
- [x] Give core types the canonical encoding from FILES, so the snapshot, change
  files, and RPC share one representation without transport copies: `Payload`
  uses native strings and arrays, tagged integer/boolean/timestamp/record
  envelopes, decimal-string integers, and bare `i64` input shorthand.
  `EntryId.position`, `ViewId.end_exclusive`, and other large control integers
  serialize as decimal strings. Rename `Number` to `Integer` and `PreciseTime` to
  `Timestamp` in `Payload`, `PayloadKind`, and `ValueType`, matching the words
  the type schema already uses.
- [x] Return a `Checked` outcome from `transaction::prepare`: an accepted
  `PreparedTransaction`, the only committable form, owning the candidate graph,
  or a `Rejection` keeping the complete `AnalysisOutput` and operation mapping
  without a candidate graph. Structural and analyzer execution failures stay
  errors.
- [x] Test intent, encoding round trips, and rejection contents at several
  boundaries, including before and after the relevant append.

Before the foundation, a second core round consolidated what the UI will freeze
into its fixtures:

- [x] Remove the `format_version` sentinel from proposals and the snapshot. The
  project forbids version detection, and `deny_unknown_fields` already fails
  loudly on a changed shape.
- [x] Encode type declarations as `fields`, `outgoing`, `incoming`, and
  `successors`, each field a native list of presence followed by shape words
  ([FILES §2.6](FILES.md#26-type-declaration-payloads)), so hand-written
  declarations need no envelopes for scalar and list shapes.
- [x] Record an observation once per contiguous run of prefixes, with first and
  last evaluated prefixes, a focus entry instead of a located view, one subject
  set, and no copy of the frozen inputs
  ([SPEC §8.2](SPEC.md#82-finding-identity-and-observations)). Without this, a
  nonsticky warning about every current node produced n(n+1)/2 observations per
  check (section 5).
- [x] Share one diagnostic struct between analyzer messages and observations,
  and lower proposals and suggestions to one `EntryChange` type that cannot add
  analyzers.

### 4.1 First slice: change files, storage, and CLI

Goal: `lifecore init`, then `check` and `apply` a handwritten JSON or TOML file,
look up entries through `rpc`, reload, and see the result. No HTTP, React, or
suggestion export yet.

- [x] Add workspace members `lifecore-app` (shared contracts, JSON/TOML codecs,
  storage, and the synchronous service; no async or HTTP dependencies) and
  `lifecore-cli` (the `lifecore` binary). `lifecore-server` and `serve` belong
  to section 4.2.
- [x] Implement one `Proposal` representation for JSON and TOML, with optional
  `plan_id` and `acknowledges`, over the core encoding from section 4.0. Omit
  absent optional fields in exports so both formats agree.
- [x] Lower proposals to core changes: unique operation names, exact existing
  references, earlier-operation references, and revision intent.
- [x] Implement environment/cwd path resolution once. Reject empty, missing, or
  unreadable selected paths for ordinary operations. Implement no-clobber `init`
  with TypeCheck configured by default and a `--no-typecheck` opt-out.
- [x] Implement the single JSON snapshot and stable sibling lock file specified
  in FILES. Hold writer exclusion from reload and exact base verification through
  same-directory temporary-file publication and required synchronization.
  Preserve pre-publication rollback and distinguish post-publication
  synchronization failure. Start with locking around reload, check, and publication.
- [x] Mint and validate plan IDs, including foreign databases, equal-count
  divergence, and suite-only changes. Persist fresh graph revision UUIDs
  atomically with changed graph/suite state; dismissals, no-ops, and minting
  preserve it.
- [x] Implement `check FILE`, `apply FILE`, `plan-id`, and `export FILE --format`
  with the output, exit-status, and evaluation-time options in
  [INTERFACES](INTERFACES.md#2-cli-surface).
- [x] Implement `add-analyzer NAME [CONFIG]` as a one-operation transaction
  against the current state, checked by full replay under the extended suite.
- [x] Define common request/result/error types and the single-call JSON-RPC-based
  envelope described in INTERFACES, with dispatch shared by HTTP and the one-shot
  CLI; keep `init` outside dispatch. Implement `rpc`: one line in, one compact
  line out, then exit, with no request loop, method arguments, file options,
  notifications, or batches. Invalid input must not mutate the database.
- [x] Add read-only entry listing and lookup methods, returning exact identities
  with type, payload, and currentness. Method names stay provisional; the UI
  stages in section 4.3 reshape them as needed.
- [x] Report rejected proposals with the complete analysis context from section
  4.0, with operation/field locations where available.
- [x] Return provisional operation-to-entry mappings for previews and actual
  mappings after apply. Do not assume independent preparations reuse entry IDs.
- [x] Exercise equivalent JSON/TOML proposals, nested and empty values, timestamp
  round trips, both integer input forms and canonical output, malformed envelopes,
  invalid references, failed checks, revision intent at intermediate prefixes,
  both `init` paths, writer competition, stale tokens, publication failures, and
  `add-analyzer` rejected by an inconsistency already in history.

Exit: `init` -> `apply` -> find the new entries through `rpc` -> `apply` a
revision of one -> reload -> `check` works on real files; `add-analyzer` enables
DagCheck on the result; a stale token and a rejected proposal leave the stored
state unchanged.

### 4.2 Foundation

Goal: a running skeleton on which the UI stages build. `lifecore serve` prints
its URL; opening it in Firefox shows `database.describe` from a React page built
by the frontend toolchain. No feature screens yet.

- [x] Align the core Serde vocabulary with the proposal vocabulary before client
  types are written. Core types and snapshots still use
  `{"Node":{ty_name,body,parent}}`, `{"AddNode":…}`, and `{"Existing":…}` beside
  the proposal's `kind`, `type`, `payload`, `predecessor`, `existing`, and
  `added`; `EntryRecord` could then become the core entry plus `current`.
- [x] Add the `lifecore-server` workspace member (Axum adapter library) and
  `serve`, serving the embedded client and one HTTP RPC endpoint over the shared
  decoder/dispatcher. Resolve the database at startup. Keep initialization and
  filesystem path selection outside remote methods.
- [x] Run each request on a blocking task that calls the synchronous service
  exactly as a CLI invocation does, reading the current snapshot and keeping no
  state between requests. There is no worker thread, queue, or cache; the file
  lock serializes writers. Do not add async traits or new cross-thread
  requirements to the core for HTTP convenience; the service's factory
  constructor only needs to be `Send + Sync`.
- [x] Implement the server access rules of
  [INTERFACES §3.2](INTERFACES.md#32-server-access): loopback binding, a launch
  token printed in the startup URL's fragment, and `Host`, `Origin`, and
  content-type checks before dispatch. Also implement orderly shutdown and
  structured busy/backend errors. A client disconnect does not cancel a running
  task and must not be presented as a transaction rollback.
- [x] Set up the frontend project and build of
  [INTERFACES §5.1](INTERFACES.md#51-frontend-build-and-tests): the Node project
  in `lifecore-server/ui`, `build.rs` bundling into `OUT_DIR`, embedded assets,
  `LIFECORE_UI=skip` for Node-free builds, a hard error with instructions when
  `node_modules` is missing, and `serve --ui-dir` in every build.
- [x] Write the TypeScript contract by hand as described in
  [INTERFACES §5](INTERFACES.md#5-react-client), and guard it with committed
  fixtures emitted by a Rust test, covering integer envelopes, large control
  integers, and timestamps. Keep integer model values as decimal strings; use
  `bigint` only when arithmetic is needed. Keep certificate strings opaque.
- [x] Build the client shell: an RPC client that takes the launch token from the
  URL fragment, keeps it in session storage, and sends it on every call;
  structured error display; and one real call, `database.describe`.
- [x] Add a Playwright smoke test in Firefox that starts `serve` on a scratch
  database, opens the printed URL, and sees the description.
- [x] Add the frontend checks to AGENTS.md "Checks", including
  `LIFECORE_UI=skip` for Rust-only runs.
- [x] Retire the `//A` comment prefix: convert every `//A` comment in the
  repository, including `TODO:` and `HACK:` markers, to plain `//`, and fold
  the AGENTS.md commenting rules into one bullet.

Exit: `lifecore serve` prints a URL that opens in Firefox and shows the database
description; requests without the token, without a JSON content type, from a
foreign origin, or with a rebinding `Host` are rejected before dispatch; the smoke test and all Rust and
frontend checks pass.

### 4.3 Iterative UI stages

Each stage ran UI → API → UI: sketch the screens against existing methods, add
or reshape the methods they turn out to need, then finish the screens. RPC
methods are not frozen. Each API change updates the
[INTERFACES §3.1](INTERFACES.md#31-provisional-first-slice-methods) method
table, the fixtures, and the hand-written `contract.ts` together; compatibility
is not a concern, so a later stage may reshape an earlier method. Each stage
ends with its workflow on real data in the browser, a Playwright test for it in
Firefox, and its UI decisions recorded in
[INTERFACES §5.2](INTERFACES.md#52-client-decisions). The prototype took the
stages in the order Browse, Authoring, Findings with rendering, then a
navigation round, each followed by a review round.

#### 4.3.1 Browse

- [x] Browse by type and currentness in tables; inspect an entry's payload by
  its type, its revision lineage, and its relationships with currentness.
- [x] Add the methods these screens need: `types.list`, `entries.inspect`
  (which replaced the expected `entries.lineage` and `entries.relationships`),
  and an `as_of` boundary on every read.
- [x] Settle how rows are labeled (INTERFACES §5.2).

#### 4.3.2 Authoring

- [x] Build a draft tray of ordered, named operations with JSON/TOML
  import/export, check preview, explicit apply, and actual assigned-ID handling.
  Attach check findings to operations through the report's operation mapping.
  Preserve unsaved drafts on failure. Send raw imported file text to the Rust
  codec so bare large integers are decoded before JavaScript can round them.
  Require deliberate replanning after stale-base rejection.
- [x] Build forms from type declarations for payloads and for type declarations
  themselves, keeping integers as decimal strings and timestamps at full
  precision.
- [x] Assist reconnection after revisions and splits by proposing explicit edge
  operations for the incident edges the user keeps.
- [x] Distinguish rejection, successful publication, and uncertain publication.
  Never automatically replay a write after transport failure; an uncertain
  apply blocks the draft until the user decides whether it was published.

#### 4.3.3 Findings

- [x] Add analysis and dismissal methods: `analysis.run`, `findings.dismiss`,
  and `findings.restore`. Outputs are ephemeral, with view, inputs, and
  implementations; dismissal loading is independent of analyzer availability;
  edits reread under writer exclusion, preserve concurrent records, never
  rotate the graph revision, and reject inconsistencies as targets.
- [x] Present analyzer groups, current/history selection, historical sticky
  labels, evaluation context, dismissal reasons/notes, and restore. Reuse
  backend grouping and validation semantics.
- [x] Show the analyzer suite and add analyzers behind a confirmation that
  shows any rejection; `database.describe` lists the analyzer names the host
  registers.

#### 4.3.4 Views and navigation

- [x] Build views as filter pipelines (seeds and expansion steps, then an edge
  filter) with an "as of" boundary, shown as tables, threads, or a Graphviz
  drawing rendered on demand by `views.render`, which runs `dot -Tsvg` as a
  subprocess with a deadline.
- [x] Settle where saved views live (browser storage, per database) and the
  thread display's details (INTERFACES §5.2).
- [x] Navigate by relationships: relationship columns in type tables, an
  expandable relationship outline in the inspector, pinned and recent
  entries, and Back that restores each screen's state.
- [ ] Compare two boundaries (`views.compare`, over the core's
  `ResolutionComparison`).
- [ ] A refine step that subtracts a selection from a view.

### 4.4 Suggestions and acknowledgment

Follow [SUGGESTIONS.md](SUGGESTIONS.md).

- [ ] Pair each suggestion observation in analysis results with a ready-made
  proposal: a plan ID for the analyzed base, `acknowledges`, and the ordered
  actions. Do not produce them for uncommitted check candidates.
- [ ] Verify `acknowledges` in `check`/`apply` against observations whose range
  begins within the committed base, from the same replay, comparing lowered
  operations exactly and ignoring operation names. Reject unverified acknowledgments with a distinct error.
- [ ] Stage `Acknowledged` with the graph changes in one snapshot, preserving an
  existing reason/note and concurrent records. Preview it in `check`.
- [ ] In React, select an observation's attached proposal, show original and
  matched contexts, and apply with the export's evaluation inputs by default.
  Add a Playwright test in Firefox for the workflow.
- [ ] Add DagCheck tests for cycles sharing a node and for cyclic components that
  grow, split, and shrink back, asserting certificate identity in each case.
  Acknowledgment makes analyzer certificates carry more weight.
- [ ] Cover SUGGESTIONS' acceptance scenarios.

### 4.5 Final verification

Each stage verifies its own workflow; these checks cut across stages.

- [ ] Demonstrate: `init` -> browse -> author/import a proposal -> check -> apply
  -> reload -> inspect revisions and relationships, through both CLI and HTTP.
  After a refresh, CLI and HTTP observe each other's committed writes.
- [ ] Compare the same request through CLI `rpc`, HTTP, and dedicated change-file
  commands. Results agree for fixed base/inputs, allowing transport correlation
  IDs and independently allocated provisional entry IDs to differ.
- [ ] Prove a rejected proposal leaves persisted state unchanged, an invalid
  intermediate prefix cannot pass, and two drafts cannot silently overwrite
  each other's base. Exercise failure before and after publication.
- [ ] Verify `rpc` returns before stdin EOF once a newline-terminated request is
  available, emits one line on success/error, does not process a second line,
  and sends logs only to stderr.
- [ ] Exercise React with `i64` extrema, nanosecond timestamps, nested records,
  missing evaluation time, sticky findings, and dismissed inconsistencies in
  imported display state. Presentation never changes acceptance.
- [ ] Verify that restart retains graph/suite and dismissals but recomputes
  analysis. History selection must not imply an archive of prior evaluation
  times or implementation versions.
- [ ] Keep SPEC, FILES, INTERFACES, SUGGESTIONS, examples, and implementation
  aligned. Run workspace tests, nightly formatting, strict Clippy for changed Rust
  packages, and the frontend checks, including the Firefox workflow tests.

## 5. Checking cost

At `a07483e`, checking cost grew faster than the cube of history length:
`View::is_stale` scanned every visible node, and TypeCheck re-judged every revision
transition in the view at every replayed prefix. Measured ad hoc with a release
build of `lifecore-cli`. Each snapshot holds a `Task` type, tasks, revisions, and
`DependsOn` edges, with TypeCheck configured. The measurements time
`database.describe` through `rpc`, and `check` of a one-node proposal. Before is
`a07483e`; after includes both completed items below. Where compared (describe
and entry listings up to 2,000 entries, check up to 1,000), outputs matched the
baseline apart from the new entry's random origin, though that proposal yields no
observations. On the 500-entry snapshot, a proposal that declares a type, revises
a current Task into it, and appends two more nodes exits 1 on both: the baseline
reports three `BadSuccessor` observations (then named `BadOverride`) at
boundaries 502–504, this branch one at 502, under the same key apart from the provisional entry origin.

| Entries | describe before | describe after | check before | check after |
| ---: | ---: | ---: | ---: | ---: |
| 500 | 0.03 s | 0.004 s | 0.8 s | 0.016 s |
| 1,000 | 0.3 s | 0.009 s | 21 s | 0.06 s |
| 2,000 | 3.1 s | 0.03 s | 370 s | 0.25 s |
| 4,000 | 33 s | 0.11 s | not run | 1.0 s |

Cost is now roughly quadratic in history length.

Output size was a separate problem. A nonsticky warning about current state is
re-emitted at every prefix where it holds, and each emission was a distinct
observation, so a proposal of n tasks missing an encouraged field produced
n(n+1)/2 observations: 20,100 observations and a 36 MB check result for 200
tasks, at `b20673d`. The second core round in section 4.0 records one
observation per contiguous run of prefixes instead: the same 200 tasks give 200
observations and a 0.36 MB result, with the check at 0.05 s and 5 MB of memory.

- [x] Answer staleness in constant time from a derived index of each node's
  first-child position, owned by `LifeCoreList`, rebuilt on load, extended by
  `prepare` only with validated entries, and compared with each view's length
  ([SPEC §9](SPEC.md#9-inspection-and-queries)).
- [x] TypeCheck judges a revision transition only at the prefix whose newest
  entry is the successor; full replay still judges every transition once, with
  unchanged finding keys and acceptance
  ([SPEC §7.2](SPEC.md#72-successor-type-permission)).

Backlogged, not scheduled:

- Incremental `basic_validation`: `prepare` still revalidates the whole graph
  before staging and after every staged entry; only the appended suffix needs it.
- Hash-based duplicate checks: node and edge duplicate detection still scans the
  prefix before each entry.
- Adjacency indexes: `children`, `edges_of`, `descendants`, and
  `fresh_successors` still scan the view.
- TypeCheck type-table or parse caching: every prefix re-parses each current
  declaration and rebuilds its type table.
- Reuse of base-prefix observations: every check replays the committed base
  prefixes again.
- A repeatable benchmark in the repository, replacing these ad hoc measurements.

Caching or incremental checks must reproduce full-prefix results exactly
([SPEC A.5](SPEC.md#a5-failure-handling-and-cost)), keyed by exact base, suite,
implementations, and inputs. The server reloads the snapshot for every request,
so load cost matters there too; a later cache would need a freshness check,
because dismissal edits change the snapshot without rotating the graph revision;
see the backlogged change detection below.
Acknowledgment verification reads observations beginning within the committed
base from the same replay, so an incremental checker must still provide them.

## 6. Backlog

### Markdown document collections

Preserve the [collection contract](BACKLOG.md#1-document-collections) for later:
checkout/reconciliation, document identities, relationship authority, retained
historical link resolution, manifests, and recoverable source-file writeback.
Collection planning and its `plan` workflow are not required for this web round.

The selected format embeds TOML in a YAML frontmatter `lifecore` string and uses
CommonMark body links. It reuses the shared payload encoding. URI destinations
and inherited database context remain as specified in BACKLOG. Collection
writeback recovery depends on durable receipts.

### Further collection format and tooling work

YAML-native metadata, custom-scheme clickability in external renderers, and
installation of desktop URI handlers remain deferred. Preserve unrelated YAML
fields when collection editing is implemented.

### Durable application receipts

Preserve the [recovery contract](BACKLOG.md#7-deferred-receipts-and-retry-recovery):
atomically record proposal identity/content and application results, recognize
identical retries before stale-base rejection, reject conflicting identity reuse,
and recover collection writeback without overwriting newer text. Cover empty
plans, including empty acknowledged suggestions, retries after later appends,
and combined graph/dismissal effects. Tokens alone provide no retry recovery.

### Other deferred work

Persistent analysis-output archives, remote multi-user hosting, multiple storage
backends, general query languages, automatic repair/reconnection, and automatic
split selection are outside this round. Keep improvements driven by concrete
workflows.

- Result limits and paging for list and query methods. The prototype returns
  complete results.
- Browser tests in Chromium. Firefox is the tested browser for now.
- Client navigation aids left out of the prototype: hover previews of linked
  nodes, links from a superseded neighbour to its current successors, and an
  incremental neighbourhood map with stable positions.
- A `views.query` option that returns only the edges incident to the seeds,
  so a type table's relationship columns do not ship the edges among its
  neighbours, and row virtualization beyond the client's 500-row pages.
- A schema helper for the introduce/retire migration paths of
  [SPEC §10.3](SPEC.md#103-gradual-schema-migration), which today are
  authored as ordinary declaration revisions.
- A server cache or cheaper snapshot loading: every request reloads the
  snapshot, which dominates the cost of lazy reads such as the inspector's
  outline on large databases.
- Snapshot change detection, e.g. a snapshot revision rotated on every
  publication, including dismissal edits. For a single user, a page refresh
  re-reads everything; revisit if dismissals grow large or a server cache is
  added, which could otherwise check file metadata.
