# LifeCore Web Application Plan

## 1. Outcome

Build a React browser client served by a Rust HTTP application. Keep graph
operations, analyzers, and the shared application service synchronous. HTTP and
CLI adapters use the same requests, proposal types, results, and errors.

JSON and TOML change files remain first-class authoring inputs. Keep dedicated
change-file commands alongside `init`, `serve`, and a minimal `rpc` command that
reads one request line from stdin, prints one response line to stdout, and exits.
The web server calls the application service directly through a worker thread.

Read [SPEC.md](SPEC.md) for graph semantics, [INTERFACES.md](INTERFACES.md) for
application boundaries and commands, [FILES.md](FILES.md) for authoring and
publication contracts, [SUGGESTIONS.md](SUGGESTIONS.md) for suggestion export and
acknowledgment, and [AGENTS.md](../AGENTS.md) for contributor guidance.
These documents describe the intended application; unchecked boxes below are
remaining work, not claims of implemented behavior.

## 2. Selected direction

- Use React with TypeScript for the browser. Serve its built assets and
  `POST /rpc` from the Rust HTTP application, initially for a local user and one
  selected database. Axum is the planned HTTP adapter.
- Define shared Serde request, result, error, and proposal types. The synchronous
  application service owns checking, application, storage, and dismissal updates.
  Reuse core domain types where suitable; transport types must own their data.
- Keep async execution, channels, and HTTP dependencies in the server adapter.
  A dedicated ordinary thread owns the analyzer factories, takes requests from a
  bounded queue, and handles each like a CLI invocation, keeping no state between
  requests. CLI operations call the synchronous service directly without a
  running server.
- `rpc` accepts no method arguments or change-file options. It processes exactly
  one JSON request line and returns exactly one compact JSON response line.
  Diagnostics go to stderr. Dedicated `check`, `apply`, `plan-id`, and proposal
  export commands remain conveniences over the shared service.
- Keep `init` outside the RPC capability set. Only explicit initialization creates
  a database; ordinary requests require one to exist. Add the TypeCheck
  configuration by default; `init --no-typecheck` opts out. Resolve
  `LIFECORE_STATE_FILE_PATH`, or `./LifeCoreState.json` when unset, relative to the
  process working directory. A bad selected path is an error, never a fallback.
- Change the analyzer suite only through a deliberate `add-analyzer` operation
  (CLI command and RPC method), never through proposals. It appends; there is
  no removal or reconfiguration.
- Deserialize JSON and TOML change files into the same `Proposal`. Use native
  strings and arrays; wrap integers, booleans, timestamps, and records in tagged
  objects/tables. Integer envelopes carry decimal strings; accept bare `i64`
  input shorthand in both formats and always export envelopes. Known large
  control integers use decimal strings without wrappers. Use Jiff's ordinary
  Serde timestamp strings, with no TOML datetime adapter or normalization profile.
- Carry an optional service-minted `plan_id`,
  `lc1:<database-uuid>:<graph-revision-uuid>:<proposal-uuid>`. Browser-generated
  proposals include it. Without a token, select the current stored base for that
  invocation. `apply` always checks anew and protects its concrete base through
  publication.
- Store one JSON snapshot containing `format_version`, `graph_revision`, `graph`
  (including the ordered suite), and `dismissals`. Use a stable sibling
  `<state-path>.lock` file and atomic whole-snapshot replacement. Rotate the
  revision UUID on graph/suite changes, not dismissal edits, no-ops, or minting.
  Keep the core's exact base comparisons; the application owns revision maintenance.
- Pair each exported suggestion observation with a ready-made proposal whose
  optional `acknowledges` field names its finding key. `check`/`apply` accept it
  only if their own replay of the committed base emits that suggestion with
  exactly these actions, then publish the graph changes and an `Acknowledged`
  dismissal together. Nothing is signed or stored per export.
- Keep analysis outputs ephemeral and recompute them per request; in-memory
  caching is deferred. Show the complete evaluation context. History mode means prefixes
  replayed in this run, not saved earlier runs. Persist dismissals independently.
- Preserve exact references, explicit revision/branch intent, every-prefix
  validation, frozen evaluation inputs, and the unfiltered inconsistency gate.
  No adapter has an unchecked mutation path.
- Acquire filesystem writer exclusion for mutations. Reads and standalone checks
  open one complete immutable snapshot without a writer lock. Refresh
  current-state requests after external writes. Do not hold a lock while a user
  edits a draft or between standalone check and apply.

## 3. Decisions before dependent implementation

| Decision | Required outcome |
| --- | --- |
| RPC method and result shapes | Freeze method names and Serde fixtures before building React forms (section 4.3), including observation/proposal pairing and preview/application dismissal effects. Preserve FILES references and operation-to-entry mappings. |

## 4. Implementation sequence

Build a working vertical slice before the remaining layers. Exact encodings in
FILES and INTERFACES are provisional until code exercises them; when the
implementation shows a contract is wrong, change the contract and the code
together rather than working around the document.

### 4.0 Preliminary core work

Core API and encoding changes that the first slice builds on.

- [ ] Move revision intent into the core: `Change::AddNode` carries
  `Option<RevisionIntent>`, and `transaction::prepare` validates it at each
  prefix (a predecessor requires an intent, a root forbids one, and `revise`
  requires a current predecessor immediately before the append). Give
  `SuggestedAction` the same node/edge shapes as `Change`, so acknowledgment
  matching is plain equality; `prepare_suggestion` keeps only its source check.
- [ ] Give core types the canonical encoding from FILES, so the snapshot, change
  files, and RPC share one representation without transport copies: `Payload`
  uses native strings and arrays, tagged integer/boolean/timestamp/record
  envelopes, decimal-string integers, and bare `i64` input shorthand.
  `EntryId.position`, `ViewId.end_exclusive`, and other large control integers
  serialize as decimal strings. Rename `Number` to `Integer` and `PreciseTime` to
  `Timestamp` in `Payload`, `PayloadKind`, and `ValueType`, matching the words
  the type schema already uses.
- [ ] Return a checked candidate from `transaction::prepare` that owns its
  complete `AnalysisOutput` and remains inspectable when it has inconsistencies.
  Only a candidate without inconsistencies converts into a committable
  `PreparedTransaction`. Structural and analyzer execution failures stay errors.
- [ ] Test intent, encoding round trips, and rejected-candidate inspection at
  several boundaries, including before and after the relevant append.

### 4.1 First slice: change files, storage, and CLI

Goal: `lifecore init`, then `check` and `apply` a handwritten JSON or TOML file,
look up entries through `rpc`, reload, and see the result. No HTTP, React, or
suggestion export yet.

- [ ] Add workspace members `lifecore-app` (shared contracts, JSON/TOML codecs,
  storage, and the synchronous service; no async or HTTP dependencies),
  `lifecore-server` (Axum adapter library), and `lifecore-cli` (the `lifecore`
  binary, including `serve`).
- [ ] Implement one `Proposal` representation for JSON and TOML, with optional
  `plan_id` and `acknowledges`, over the core encoding from section 4.0. Omit
  absent optional fields in exports so both formats agree.
- [ ] Lower proposals to core changes: unique operation names, exact existing
  references, earlier-operation references, and revision intent.
- [ ] Implement environment/cwd path resolution once. Reject empty, missing, or
  unreadable selected paths for ordinary operations. Implement no-clobber `init`
  with TypeCheck configured by default and a `--no-typecheck` opt-out.
- [ ] Implement the single JSON snapshot and stable sibling lock file specified
  in FILES. Hold writer exclusion from reload and exact base verification through
  same-directory temporary-file publication and required synchronization.
  Preserve pre-publication rollback and distinguish post-publication
  synchronization failure. Start with locking around reload, check, and publication.
- [ ] Mint and validate plan IDs, including foreign databases, equal-count
  divergence, and suite-only changes. Persist fresh graph revision UUIDs
  atomically with changed graph/suite state; dismissals, no-ops, and minting
  preserve it.
- [ ] Implement `check FILE`, `apply FILE`, `plan-id`, and `export FILE --format`
  with the output, exit-status, and evaluation-time options in
  [INTERFACES](INTERFACES.md#2-cli-surface).
- [ ] Implement `add-analyzer NAME [CONFIG]` as a one-operation transaction
  against the current state, checked by full replay under the extended suite.
- [ ] Define common request/result/error types and the single-call JSON-RPC-based
  envelope described in INTERFACES, with dispatch shared by HTTP and the one-shot
  CLI; keep `init` outside dispatch. Implement `rpc`: one line in, one compact
  line out, then exit, with no request loop, method arguments, file options,
  notifications, or batches. Invalid input must not mutate the database.
- [ ] Add read-only entry listing and lookup methods, returning exact identities
  with type, payload, and currentness. Method names stay provisional until
  section 4.2 freezes them.
- [ ] Report rejected proposals with the complete analysis context from section
  4.0, with operation/field locations where available.
- [ ] Return provisional operation-to-entry mappings for previews and actual
  mappings after apply. Do not assume independent preparations reuse entry IDs.
- [ ] Exercise equivalent JSON/TOML proposals, nested and empty values, timestamp
  round trips, both integer input forms and canonical output, malformed envelopes,
  invalid references, failed checks, revision intent at intermediate prefixes,
  both `init` paths, writer competition, stale tokens, publication failures, and
  `add-analyzer` rejected by an inconsistency already in history.

Exit: `init` -> `apply` -> find the new entries through `rpc` -> `apply` a
revision of one -> reload -> `check` works on real files; `add-analyzer` enables
DagCheck on the result; a stale token and a rejected proposal leave the stored
state unchanged.

### 4.2 Complete RPC contract

- [ ] Add inspection, comparison, analysis, analyzer suite append, and dismissal
  list/update/restore methods. Dismissal edits reread under writer exclusion and
  preserve concurrent records; reject inconsistencies as dismissal targets.
- [ ] Return ephemeral analysis outputs with view, inputs, and implementations.
  Keep dismissal loading independent of analyzer availability.
- [ ] Freeze method names and fixtures (section 3).

### 4.3 Rust HTTP adapter and React client

- [ ] Add `serve`, serving the React assets and one HTTP RPC endpoint over the
  shared decoder/dispatcher. Resolve the database at startup. Keep initialization
  and filesystem path selection outside remote methods.
- [ ] Run synchronous work on a dedicated thread. Construct analyzer factories
  there; send owned requests/results through a bounded queue. Handle each request
  like the corresponding CLI invocation, reading the current snapshot and keeping
  no state between requests. A queued request is not a lock. Do not add async traits or new cross-thread requirements to the
  core for HTTP convenience.
- [ ] Implement local binding and same-origin access controls, queue failure
  handling, orderly shutdown, and structured busy/backend errors. An HTTP
  disconnect must not be presented as a transaction rollback.
- [ ] Generate TypeScript contracts with ts-rs as described in
  [INTERFACES](INTERFACES.md#5-react-client), and type-check serialized Rust
  fixtures against them, including integer envelopes, large control integers,
  and timestamps. Keep integer model values as decimal strings; use `bigint`
  only when arithmetic is needed. Keep certificate strings opaque.
- [ ] Build browsing by type/currentness, entry details, relationships, revision
  navigation, and bounded comparisons using backend resolution results.
- [ ] Build a structured draft editor, JSON/TOML import/export, check preview,
  explicit apply, and actual assigned-ID handling. Preserve unsaved drafts on
  failure. Send raw imported file text to the Rust codec so bare large integers
  are decoded before JavaScript can round them. Require deliberate replanning
  after stale-base rejection.
- [ ] Present analyzer groups, current/history selection, historical sticky
  labels, evaluation context, dismissal reasons/notes, and restore. Reuse backend
  grouping and validation semantics.
- [ ] Distinguish rejection, successful publication, and uncertain acknowledgment.
  Never automatically replay a write after transport failure.

### 4.4 Suggestions and acknowledgment

Follow [SUGGESTIONS.md](SUGGESTIONS.md).

- [ ] Pair each suggestion observation in analysis results with a ready-made
  proposal: a plan ID for the analyzed base, `acknowledges`, and the ordered
  actions. Do not produce them for uncommitted check candidates.
- [ ] Verify `acknowledges` in `check`/`apply` against committed-base observations
  from the same replay, comparing lowered operations exactly and ignoring
  operation names. Reject unverified acknowledgments with a distinct error.
- [ ] Stage `Acknowledged` with the graph changes in one snapshot, preserving an
  existing reason/note and concurrent records. Preview it in `check`.
- [ ] In React, select an observation's attached proposal, show original and
  matched contexts, and apply with the export's evaluation inputs by default.
- [ ] Add DagCheck tests for cycles sharing a node and for cyclic components that
  grow, split, and shrink back, asserting certificate identity in each case.
  Acknowledgment makes analyzer certificates carry more weight.
- [ ] Cover SUGGESTIONS' acceptance scenarios.

### 4.5 End-to-end verification

- [ ] Demonstrate: `init` -> browse -> author/import a proposal -> check -> apply
  -> reload -> inspect revisions and relationships, through both CLI and HTTP.
  CLI and HTTP observe each other's committed writes.
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
  packages, and frontend type/build checks plus focused browser workflow tests.

## 5. Upcoming: checking cost

Scheduled soon; not part of the first slice. Checking cost grows roughly with the
cube of history length. Measured ad hoc on `d36cf22` (release build, TypeCheck
configured, one type declaration plus root nodes, checking one appended node):

| Entries | Load and validate | Check one appended node |
| --- | --- | --- |
| 500 | 24 ms | 93 ms |
| 1,000 | 178 ms | 688 ms |
| 2,000 | 1.6 s | 6.1 s |

Every apply reloads and checks, and the web client checks drafts interactively,
so this needs addressing before real use. Known causes:

- `transaction::prepare` reruns `basic_validation` over the whole graph before
  staging and after every staged entry, though the base was validated on load.
  Only the appended suffix needs checking.
- `View::is_stale` scans all nodes, making `fresh_nodes`, duplicate detection,
  and TypeCheck's per-prefix work quadratic in prefix length.
- The runner replays every prefix, multiplying per-prefix analyzer cost by
  history length.

Candidate direction: validate only the appended suffix; index each node's first
child position so staleness in a view of length `L` is `first_child < L`, which
answers exactly for every prefix without exposing the suffix; then profile
analyzer replay separately. Caching or incremental checks must reproduce
full-prefix results exactly ([SPEC A.5](SPEC.md#a5-failure-handling-and-cost)),
keyed by exact base, suite, implementations, and inputs. The server worker
reloads the snapshot for every request, so load cost matters there too; a later
worker cache would need a freshness check on file metadata (inode, modification
time, size), because dismissal edits change the snapshot without rotating the
graph revision. Acknowledgment
verification reads committed-base observations from the same replay, so an
incremental checker must still provide them. Add a repeatable benchmark with
this work.

## 6. Backlog

### Markdown document collections

Preserve the [collection contract](FILES.md#4-document-collections) for later:
checkout/reconciliation, document identities, relationship authority, retained
historical link resolution, manifests, and recoverable source-file writeback.
Collection planning and its `plan` workflow are not required for this web round.

The selected format embeds TOML in a YAML frontmatter `lifecore` string and uses
CommonMark body links. It reuses the shared payload encoding. URI destinations
and inherited database context remain as specified in FILES. Collection
writeback recovery depends on durable receipts.

### Further collection format and tooling work

YAML-native metadata, custom-scheme clickability in external renderers, and
installation of desktop URI handlers remain deferred. Preserve unrelated YAML
fields when collection editing is implemented.

### Durable application receipts

Preserve the [recovery contract](FILES.md#84-deferred-receipts-and-retry-recovery):
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
