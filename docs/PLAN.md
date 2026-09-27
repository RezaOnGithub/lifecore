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
publication contracts, [SUGGESTIONS.md](SUGGESTIONS.md) for authenticated
suggestions and dismissal, and [AGENTS.md](../AGENTS.md) for contributor guidance.
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
  A dedicated ordinary thread owns application state and handles a bounded queue.
  CLI operations call the synchronous service directly without a running server.
- `rpc` accepts no method arguments or change-file options. It processes exactly
  one JSON request line and returns exactly one compact JSON response line.
  Diagnostics go to stderr. Dedicated `check`, `apply`, `plan-id`, and proposal
  export commands remain conveniences over the shared service.
- Keep `init` outside the RPC capability set. Only explicit initialization creates
  a database; ordinary requests require one to exist. Add the TypeCheck
  configuration by default; `init --no-typecheck` opts out. Resolve
  `LIFECORE_STATE_FILE_PATH`, or `./LifeCoreState.json` when unset, relative to the
  process working directory. A bad selected path is an error, never a fallback.
- Deserialize JSON and TOML change files into the same `Proposal`. Use native
  strings and arrays; wrap integers, booleans, timestamps, and records in tagged
  objects/tables. Integer envelopes carry decimal strings; accept bare `i64`
  input shorthand in both formats and always export envelopes. Known large
  control integers use decimal strings without wrappers. Use Jiff's ordinary
  Serde timestamp strings, with no TOML datetime adapter or normalization profile.
- Carry an optional service-minted `plan_id` containing database identity, exact
  graph/suite base, and proposal identity. Browser-generated proposals include
  it. Without a token, select the current stored base for that invocation.
  `apply` always checks anew and protects its concrete base through publication.
- Store one JSON snapshot containing `format_version`, `graph_revision`, `graph`
  (including the ordered suite), `dismissals`, and private `proposal_auth_key`.
  Use a stable sibling `<state-path>.lock` file and atomic whole-snapshot
  replacement. Rotate the revision UUID on graph/suite changes, not dismissal
  edits, no-ops, or minting. Keep the core's exact base comparisons; the application
  owns revision maintenance.
- Use explicit ordinary and authenticated suggestion token kinds. Ordinary
  minting supplies only a base expectation. Export each suggestion from trusted
  service analysis with its ready-made proposal and an HMAC-SHA-256 token binding
  the full finding key, source context, database/base, and canonical content.
  Clients select the attached proposal; no report-ID registry is needed.
- Apply authenticated suggestions through full checking and atomically publish
  graph changes with acknowledgment of the original finding. Preserve existing
  dismissal reasons/notes. Failed checks dismiss nothing. Authentication does
  not establish current applicability, resolution, or one-use/retry semantics.
- Keep analysis outputs ephemeral, with optional in-memory caching. Recompute
  on demand and show the complete evaluation context. History mode means prefixes
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
| RPC method and result shapes | Freeze method names and Serde fixtures before building forms, including observation/proposal associations and preview/application dismissal effects. Preserve FILES references and operation-to-entry mappings. |

## 4. Implementation sequence

### Shared contracts and proposal checking

- [ ] Define common request/result/error types and the single-call JSON-RPC-based
  envelope described in INTERFACES. Share decoding and dispatch between HTTP and
  the one-shot CLI; keep `init` outside dispatch.
- [ ] Implement one Serde payload representation and one proposal representation
  for JSON and TOML. Omit absent optional fields in exports so both formats agree.
- [ ] Serialize integer payloads as tagged decimal-string envelopes. Deserialize
  envelopes and bare `i64` shorthand through the same format-independent Serde
  input type. Validate syntax/range; keep plain strings as text. Encode known
  large control integers as strings and validate their respective ranges.
- [ ] Implement complete node/edge proposals, unique operation names, exact
  existing references, earlier-operation references, and revision/branch intent.
  Export trusted suggestion observations with ready-made proposals of this type.
- [ ] Implement ordinary and authenticated suggestion token codecs from
  SUGGESTIONS. Bind complete claims and canonical typed proposal content, excluding
  `plan_id`, with HMAC-SHA-256. Freeze JCS/MAC fixtures and validate equivalent
  JSON/TOML input, precise integers, timestamp normalization, malformed/duplicate
  fields, changed content, and changed claims. Use a vetted MAC implementation.
- [ ] Mint suggestion tokens only from trusted analysis of committed history;
  preserve source context and never sign invented observations or provisional
  candidate findings. Keep generic plan-ID minting ordinary-only. Export bundles
  as independent proposals with separate identities and shared base expectations.
- [ ] Preserve complete analysis context and advisories when a proposal has
  blocking inconsistencies. Return operation/field locations where available.
  Permit useful candidate inspection only when structure is valid; rejected
  proposals must never yield a committable transaction.
- [ ] Return provisional operation-to-entry mappings for previews and actual
  mappings after apply. Do not assume independent preparations reuse entry IDs.
- [ ] Implement explicit evaluation inputs per request. Default to absent time;
  an explicit UI/CLI choice of current time is captured once by the host and
  reported as the concrete timestamp used throughout replay.
- [ ] Exercise equivalent JSON/TOML proposals, nested and empty values, timestamp
  round trips, both integer input forms and canonical output, malformed envelopes,
  invalid references, failed checks, and revision intent at intermediate prefixes.

### Synchronous application service, storage, and CLI

- [ ] Implement synchronous application operations above the core, with
  format codecs and storage shared by all frontends. Keep storage outside the
  graph/analyzer crates; choose package boundaries that avoid HTTP dependencies
  in shared contracts or synchronous operations.
- [ ] Implement environment/cwd path resolution once. Reject empty, missing, or
  unreadable selected paths for ordinary operations. Implement no-clobber `init`
  with TypeCheck configured by default, `--no-typecheck` opt-out, and
  structural/configured-analyzer validation. Cover both initialization paths.
- [ ] Implement the single JSON snapshot and stable sibling lock file specified
  in FILES. Reload/publish graph, ordered suite, revision UUID, private key, and
  dismissals together. Hold writer exclusion from reload and exact base verification
  through same-directory temporary-file publication and required synchronization.
  Preserve pre-publication rollback and distinguish post-publication
  synchronization failure.
- [ ] Generate and persist the 256-bit authentication key at initialization;
  preserve it on reload/writes and reject missing/malformed keys. Keep private
  storage fields out of public Serde results and logs. Cover restart, full-copy
  behavior, wrong keys, and secret-free RPC/export results.
- [ ] Verify suggestion authentication and base under writer exclusion, perform
  normal full-prefix checks, then stage graph and finding acknowledgment in one
  candidate snapshot. Preview both effects without mutation. Preserve concurrent
  dismissal records and an existing reason/note; reject inconsistencies as
  dismissal targets. An ordinary proposal never acquires automatic dismissal.
- [ ] Return ephemeral analysis outputs with view, inputs, and implementations.
  Recompute after restart; any memory cache must distinguish the exact graph,
  suite, implementations, and inputs. Keep dismissal loading independent of
  analyzer availability or a matching current emission.
- [ ] Start with mutation locking around reload, check, and publication. A later
  optimization may prepare outside the lock only if it rereads and compares the
  exact base under the lock before publication. A queued request is not a lock.
- [ ] Mint and validate optional plan IDs, including foreign databases,
  equal-count divergence through supported writes, and suite-only changes.
  Persist fresh graph revision UUIDs atomically with changed graph/suite state.
  Test that dismissals, no-ops, and minting preserve the revision. Minting neither
  appends nor reserves a base, and supplies no retry receipt.
- [ ] Implement dedicated `check FILE`, `apply FILE`, `plan-id`, and proposal
  export conveniences. Serialize selected ready-made suggestion proposals without
  reminting; preserve tokens across format conversion. Reject edited authenticated
  content and support deliberate conversion to an ordinary draft. JSON/TOML
  decoding feeds the shared service; do not add a second graph command language.
- [ ] Implement `rpc`: one line in, one compact line out, then exit. No request
  loop, method arguments, file options, notifications, or batches in this initial
  request/response profile. Invalid input must not mutate the database.
- [ ] Cover writer competition, stale bases, no-clobber initialization, publication
  failures, dismissal reload/restore, and preservation of concurrent dismissal
  edits. CLI and HTTP must observe each other's committed writes.

### Rust HTTP adapter and React client

- [ ] Add `serve`, serving the React assets and one HTTP RPC endpoint over the
  shared decoder/dispatcher. Resolve the database at startup. Keep initialization
  and filesystem path selection outside remote methods.
- [ ] Run synchronous work on a dedicated thread. Construct analyzer factories
  there; send owned requests/results through a bounded queue. Do not add async
  traits or new cross-thread requirements to the core for HTTP convenience.
- [ ] Implement local binding and same-origin access controls, queue failure
  handling, orderly shutdown, and structured busy/backend errors. An HTTP
  disconnect must not be presented as a transaction rollback.
- [ ] Generate TypeScript contracts from the shared Rust representation and
  verify runtime encoding against fixtures, including integer envelopes, large
  control integers, and timestamps. Keep integer model values as decimal strings;
  use `bigint` only when arithmetic is needed. Keep certificate strings opaque.
- [ ] Build browsing by type/currentness, entry details, relationships, revision
  navigation, and bounded comparisons using backend resolution results.
- [ ] Build a structured draft editor, JSON/TOML import/export, check preview,
  explicit apply, and actual assigned-ID handling. Preserve unsaved drafts on
  failure. Send raw imported file text to the Rust codec so bare large integers
  are decoded before JavaScript can round them. Require deliberate replanning
  after stale-base rejection.
- [ ] Present analyzer groups, current/history selection, historical sticky
  labels, evaluation context, dismissal reasons/notes, restore, and explicit
  selection of an observation's attached proposal. Show automatic acknowledgment
  in check/apply and retain original versus current evaluation context. Reuse
  backend grouping and validation semantics.
- [ ] Distinguish rejection, successful publication, and uncertain acknowledgment.
  Never automatically replay a write after transport failure.

### End-to-end verification

- [ ] Demonstrate: `init` -> browse -> author/import a proposal -> check -> apply
  -> reload -> inspect revisions and relationships, through both CLI and HTTP.
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
- [ ] Exercise tampered suggestion exports, stale sibling proposals, changed
  evaluation context, atomic graph/dismissal failures, and application after cache
  loss. Cover empty suggestions and restore/reapply without assuming a token is
  a receipt. Use SUGGESTIONS' focused acceptance scenarios.
- [ ] Keep SPEC, FILES, INTERFACES, SUGGESTIONS, AGENTS, examples, and implementation
  aligned. Run workspace tests, nightly formatting, strict Clippy for changed Rust
  packages, and frontend type/build checks plus focused browser workflow tests.

## 5. Backlog

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
plans and retries after later appends. Tokens alone provide no retry recovery.
Authenticated suggestion tokens do not record consumption; include empty-action
suggestions and combined graph/dismissal effects in future receipt recovery.

### Other deferred work

Persistent analysis-output archives, remote multi-user hosting, multiple storage
backends, incremental analysis, general query languages, automatic
repair/reconnection, and automatic split selection are outside this round.
Keep improvements driven by concrete workflows.
