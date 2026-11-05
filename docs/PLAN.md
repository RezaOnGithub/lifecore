# LifeCore Plan

The goal of this round is a React browser client served by a Rust HTTP
application, beside JSON and TOML change files and a small CLI, all over one
synchronous application service ([INTERFACES](INTERFACES.md)). Unchecked boxes
are remaining work, not claims of implemented behavior. Encodings and RPC
shapes are provisional until code exercises them: when the implementation shows
a contract is wrong, change the contract and the code together rather than
working around the document.

## 1. Built

- Core rework: revision intent validated by `transaction::prepare`, one
  canonical value encoding for the snapshot, change files, and RPC, `Checked`
  outcomes that keep a rejection's analysis, one observation per contiguous run
  of prefixes, and `EntryChange` as the only operation shape.
- First slice: `lifecore-app` and the `lifecore` CLI, with `init`, `check`,
  `apply`, `plan-id`, `export`, `add-analyzer`, and one-shot `rpc` over JSON
  and TOML change files, plan IDs, and atomic snapshot publication.
- Foundation: `lifecore-server` behind `lifecore serve`, with loopback access
  checks, the embedded React client, contract fixtures recorded from Rust, and
  Playwright tests in Firefox.
- UI prototype: browsing, Explore queries and drawings, drafts with check and
  apply, findings with dismissal, and appending analyzers
  ([CLIENT §5](CLIENT.md#5-prototype-scope)).
- Checking cost reduced from worse than cubic to roughly quadratic in history
  length ([PERFORMANCE](PERFORMANCE.md)).
- Domain analyzer: Reconnect ([RECONNECT](RECONNECT.md)), registered beside
  TypeCheck and DAG Check and configured after TypeCheck by
  `lifecore init --default-analyzers`
  ([INTERFACES §2](INTERFACES.md#2-cli-surface)).
- Shelved: the planning analyzer, its valuation, `planning.valuation`,
  `lifecore valuation`, and the client's Plan screen, built up to `59c702c`
  and removed after it, until the planning round reworks them
  ([AGENTS.md](../AGENTS.md#planning-is-shelved)).

## 2. Remaining work

The [core round](proposals/core-round/CORE-ROUND.md) comes before
section 2.2. It covers value kinds, typed payload decoding, and optional
indexes, in the order of its section 6. Its steps 3a, the value kinds with
their read-only display in the client, and 3b, their editors, have landed.
The App and the client then take up
the new value kinds, and the planning analyzer is reworked on top of them in
a round of its own, starting from section 5 of the core round.

Each UI stage runs UI → API → UI: sketch the screens against existing methods,
add or reshape the methods they turn out to need, then finish the screens.
Compatibility is not a concern, so a stage may reshape an earlier method; each
API change updates the [RPC](RPC.md) tables, the fixtures, and the hand-written
`contract.ts` together. A stage ends with its workflow on real data in the
browser, a Playwright test for it in Firefox, and its UI decisions recorded in
[UI](UI.md).

### 2.1 Views

- [ ] Compare two boundaries (`views.compare`, over the core's
  `ResolutionComparison`).
- [ ] A refine step that subtracts a selection from a view.

### 2.2 Suggestions and acknowledgment

Follow [SUGGESTIONS](SUGGESTIONS.md).

- [ ] Pair each suggestion observation in analysis results with a ready-made
  proposal: a plan ID for the analyzed base, `acknowledges`, and the ordered
  actions. Do not produce them for uncommitted check candidates.
- [ ] Mark an exported Reconnect edge action `satisfied` when the edge exists
  in the current graph and `inapplicable` when an endpoint is superseded, as
  application results outside the observation
  ([RECONNECT §4](RECONNECT.md#4-sticky)). Specify the marks in SUGGESTIONS,
  which owns export, and link to them from RECONNECT. Then update the
  Reconnect status lines here, in AGENTS.md, and in both RECONNECT documents.
- [ ] Verify `acknowledges` in `check`/`apply` against observations whose range
  begins within the committed base, from the same replay, comparing lowered
  operations exactly and ignoring operation names. Reject unverified
  acknowledgments with a distinct error.
- [ ] Stage `Acknowledged` with the graph changes in one snapshot, preserving an
  existing reason/note and concurrent records. Preview it in `check`.
- [ ] In React, select an observation's attached proposal, show original and
  matched contexts, and apply with the export's evaluation inputs by default.
  Add a Playwright test in Firefox for the workflow.
- [ ] Add DagCheck tests for cycles sharing a node and for cyclic components that
  grow, split, and shrink back, asserting certificate identity in each case.
  Acknowledgment makes analyzer certificates carry more weight.
- [ ] Cover the [acceptance scenarios](SUGGESTIONS.md#7-acceptance-scenarios).

### 2.3 Final verification

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
- [ ] Keep the documents, examples, and implementation aligned. Run workspace
  tests, nightly formatting, strict Clippy for changed Rust packages, and the
  frontend checks, including the Firefox workflow tests.

## 3. Backlog

Persistent analysis-output archives, remote multi-user hosting, multiple storage
backends, general query languages, automatic repair/reconnection, and automatic
split selection are outside this round. Keep improvements driven by concrete
workflows.

- [Markdown document collections](backlog/COLLECTIONS.md): checkout and
  reconciliation of Markdown files with embedded TOML metadata. Collection
  planning and its `plan` workflow are not required for this round.
- [Durable application receipts](backlog/RECEIPTS.md): recognizing completed
  retries and recovering from uncertain publication. Tokens alone provide no
  retry recovery.
- Checking-cost work ([PERFORMANCE §3](PERFORMANCE.md#3-backlog)).
- Result limits and paging for list and query methods. The prototype returns
  complete results.
- Browser tests in Chromium. Firefox is the tested browser for now.
- A Web Lock around the browser's apply. Two tabs that both write the shared
  draft within the storage's cross-tab delay, such as two Apply clicks at
  once, can still overwrite each other's in-flight marker
  (`saveGuarded` in `ui/src/draft/persist.ts`).
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
- One more pass over DagCheck (`lifecore-dag-check`), tightening its public
  API and its JSON configuration (`filter`, `on_cycle`, `range`).
- A concrete metatype for `Type` in place of the built-in one
  ([SPEC §6.1](SPEC.md#61-built-in-metatype)). The built-in metatype fixes the
  declaration payload and gives `Type` no edge permissions, so a user cannot
  annotate declarations with fields of their own or link them to other nodes:
  the same customization problem the shelved planning analyzer addressed for
  its types ([PLANNING §3.4]). This revisits the AGENTS.md invariant that TypeCheck has a
  fixed built-in metatype and no bootstrap node.
- Pooled failure reporting. Analyzer inconsistencies are already collected in
  full, but two paths report only the first failure: `transaction::prepare`
  returns at the first structural or intent failure, before any analyzer
  runs, and the analysis runner aborts on the first analyzer error, discarding
  every observation already recorded, TypeCheck's included
  ([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)). Pool every
  failure a check can find instead.
- An `info` message kind beside `inconsistency`, `warning`, and `suggestion`
  ([ANALYZERS §2](ANALYZERS.md#2-finding-identity-and-observations)), for
  notices that need no action. The shelved planning analyzer's Grace period
  notice ([PLANNING §8.5]) was a sticky
  warning for lack of one. It touches ANALYZERS, [MESSAGES](MESSAGES.md),
  [RPC](RPC.md), the client contract and its fixtures
  ([CLIENT §2](CLIENT.md#2-contract-and-fixtures)), and [UI](UI.md).
- State in [ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization) that an
  analyzer reports failure by returning `Err` and never panics. A panic
  crashes the CLI, or the server request on its blocking task, and the rule
  applies to every analyzer.
- Namespacing in the type system, so that two vocabularies can share a type
  name or an edge label. The shelved planning analyzer owned its names
  outright for lack of it ([PLANNING §13],
  decision 35).

### 3.1 Open questions

These are not scheduled work. Each needs a sketch of its own before it
becomes a backlog item.

- **Protecting the vocabulary an analyzer relies on.** A domain analyzer needs
  a minimum vocabulary: types, fields with their shapes, and edge permissions.
  It may also need constraints a declaration cannot express, such as the type
  at the opposite endpoint of a label. Users may amend declarations freely
  ([SPEC §6](SPEC.md#6-type-declarations)), so each analyzer re-judges them
  against its own requirements, as the shelved planning analyzer did by hand
  ([PLANNING §3.4], decision 36),
  and checks the rest in its own rules
  ([PLANNING §8], "Left to TypeCheck").
  Every new domain analyzer would repeat this. Two directions:
  - **Declared requirements.** At instantiation, an analyzer declares
    everything it needs: types, fields, label permissions, and endpoint
    constraints. TypeCheck then enforces these against the user's
    declarations, so an amendment that breaks one is a type error. This
    grows the declaration language toward what analyzers check by hand
    today. It also creates a channel between analyzers, which are
    independent today: TypeCheck would read the suite's requirements as
    well as the graph, and requirements become part of an analyzer's
    contract, fixed by its version and configuration.
  - **Owned types.** An analyzer supplies declarations that users cannot
    change, as the built-in metatype is fixed today
    ([SPEC §6.1](SPEC.md#61-built-in-metatype)). This is simpler, but it
    loses the user's own fields and labels on those types (PLANNING §3.4)
    unless a separate extension mechanism adds them back.

  Related: namespacing and a concrete metatype (section 3), and the
  core round's split between declaration types and decode types, which makes
  the field half of an analyzer's requirements derivable
  ([CORE-ROUND §3.2](proposals/core-round/CORE-ROUND.md#32-design)).
- **Messages as step changes.** A nonsticky message is emitted again at every
  prefix where its condition holds
  ([ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal)), so an analyzer
  that emits one must recompute its whole current state at every prefix,
  which keeps replay quadratic
  ([PERFORMANCE §3](PERFORMANCE.md#3-backlog)). `sticky` was added as a
  performance lever against that. The alternative is that every message is
  sticky by default: emitted once, at the prefix where its cause appears, so
  analyzers respond to step changes rather than judge the whole signal at
  every prefix. Recovering the messages that hold at a boundary then means
  replaying up to it, which a check and `analysis.run` already do. To settle:
  - Whether any analyzer genuinely needs the whole signal, or every rule can
    be stated as a response to the entry that changes it.
  - How a condition that ends is signalled. Today a nonsticky message leaves
    current reports when it stops being emitted, while non-emission never
    establishes resolution; a step-change model needs an explicit end, or a
    report rule that derives one.
  - What happens to current-state warnings such as TypeCheck's encouraged
    and deprecated fields, to report selection and history mode, and to
    dismissal. The inconsistency gate already selects inconsistencies at any
    boundary, so it is unaffected.

[PLANNING §3.4]: https://github.com/RezaOnGithub/lifecore/blob/59c702c/docs/PLANNING.md#34-amending-the-declarations
[PLANNING §8.5]: https://github.com/RezaOnGithub/lifecore/blob/59c702c/docs/PLANNING.md#85-at-a-seal-sticky-warnings
[PLANNING §13]: https://github.com/RezaOnGithub/lifecore/blob/59c702c/docs/PLANNING.md#13-design-decisions
[PLANNING §8]: https://github.com/RezaOnGithub/lifecore/blob/59c702c/docs/PLANNING.md#8-findings
