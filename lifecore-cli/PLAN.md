# `lifecore-cli` Implementation Plan

This plan implements the accepted interface in `lifecore-cli/SPEC.md` as a
series of cumulative, reviewable stages. Each stage establishes types,
interfaces, and invariants that later stages may rely on. The plan is written
for an implementor and a human reviewer; it deliberately leaves ordinary
Rust-level problem solving to the implementor where the architectural boundary
is already clear.

> The granularity of this plan need not be so high; it is created with the
> reviewer in mind, and shall not absolve the implementor of all design and
> problem-solving responsibility.
>
> — The User

## How to follow this plan

- Complete stages in numerical order.
- Do not begin a stage until the preceding stage's implementation, review, and
  progression gates are checked.
- Check a box only after obtaining the evidence it describes. Boxes record
  completed and verified work, not intent.
- A compiling stage is not necessarily complete. Its focused tests, applicable
  repository checks, manual walkthrough, and human-feedback requirements must
  also be complete.
- The implementor alone checks implementation, technical, verification,
  progression, and exit boxes.
- The reviewer alone checks both reviewer-owned boxes: the summary
  **Human review accepted** box and the detailed **Review gate** box. The
  implementor must never check either box on the reviewer's behalf.
- Review comments, pull-request approval, chat messages, verbal approval, and
  implied acceptance do not constitute a “Go” signal. The reviewer must
  personally edit this file and tick the reviewer-owned boxes.
- If review changes an accepted responsibility or invariant, update this plan
  and reconsider affected later stages before continuing.
- Preserve the ownership and dependency rules below. In particular, do not
  duplicate loading, view construction, reference resolution, analysis,
  transaction, sidecar, or rendering logic inside command handlers.
- Function signatures and type shapes under **Notable members** are interface
  sketches unless explicitly marked normative. Small Rust-level changes are
  allowed when they preserve the stated responsibility, ownership, and
  invariants. Material responsibility changes require a plan update first.
- Functionality assigned to a later stage must remain explicitly unavailable
  or be an explicitly documented internal no-op. A user-facing command must
  never silently report success when its requested effect is unimplemented.
- An unavailable mutation must leave graph and CLI-state bytes unchanged.
- Keep every stage buildable and testable. Do not land a knowingly broken
  intermediate stage on the assumption that the following stage will repair it.
- Add tests only under a module matching `*::test::test2`, as required by
  `AGENTS.md`.
- Do not add Rust doc comments or doctests unless the user explicitly requests
  them. Follow the agent-comment, logging, formatting, and dirty-tree
  conventions in `AGENTS.md` and `NOTES.md`.
- Prefer focused checks while the working tree contains unrelated changes.
  Expand to workspace-wide checks when a stage changes shared core behavior.
- Treat the RFCs under “Suggested extensions” in `SPEC.md` as outside this
  implementation plan unless the plan is explicitly amended.

## Progression rule

Each stage uses a three-step handshake:

1. The implementor completes the work and checks the implementation and
   verification boxes, including the summary **Stage implementation complete**
   box and detailed **Implementation gate**.
2. The reviewer reviews the stage, personally edits this file, and checks both
   reviewer-owned boxes: **Human review accepted** and **Review gate**.
3. Only after detecting those checked boxes in this file does the implementor
   check the **Progression gate** and summary **Exit gate**, then begin the next
   stage.

The reviewer-owned boxes are the sole acceptance signal. Favorable or implicit
approval elsewhere is insufficient. All summary and detailed gates must be
checked by their designated owner before proceeding.

## Review fixture generators

A stage may list multiple named fixture generators or recipes. Add a generator
during implementation when it produces a distinct artifact useful for review.
When the base CLI can reproduce the artifact, record that CLI recipe directly;
use a separate `lifecore-cli-fixture` scenario only when normal workflows cannot
construct suitable review state.

Keep each generator narrow, self-contained, and focused on a representative
happy path. Generate artifacts by exercising base `lifecore`, the base CLI, and
behavior implemented in the current or accepted prior stages. A generator must
not use functionality assigned to a later stage or substitute a preconstructed
JSON template for the construction being reviewed.

A generator recipe is review evidence, not a reusable production abstraction or
a general fixture framework. Preserve the recipe in this plan; a generated copy
need not be committed when the base CLI or a recorded generator can recreate it
at review time. Ask before removing an artifact only when no such recipe can
recreate it. After a stage is accepted, leave its recipes unchanged as a
time-capsule and add separately named generators for later needs. Revise an
accepted recipe only to correct a defect agreed with the reviewer.

## Overarching architecture

The CLI is an application layer over `lifecore` and `lifecore-typecheck`. It
must not introduce a parallel graph model, query evaluator, type checker, or
analysis lifecycle.

```text
clap syntax
    |
command adapters
    |
application services
    |
lifecore / lifecore-typecheck
    |
canonical state storage

structured command results
    |
shared table / lines / JSON rendering
```

The intended high-level source layout is:

```text
src/
    main.rs
    bin/
        lifecore-cli-fixture.rs
    lib.rs
    cli.rs
    app.rs
    error.rs
    state.rs
    analysis.rs
    effective.rs
    transaction.rs
    locator.rs
    reference.rs
    view.rs
    payload_input.rs
    query.rs
    type_edit.rs
    messages.rs
    output.rs
    sidecar.rs
    graphviz.rs
    interactive.rs
    interactive_analyzer.rs
    commands/
        mod.rs
        init.rs
        entry.rs
        add.rs
        connect.rs
        query.rs
        alias.rs
        type_command.rs
        sidecar.rs
        analyzer.rs
        check.rs
        message.rs
        graphviz.rs
        interactive.rs
    test/
        mod.rs
        test2.rs
```

The implementor may use submodules when a listed module becomes too large. Do
not create empty directories or speculative abstractions merely to reproduce
this tree exactly. The fixture binary is introduced only when Stage 3 reaches
its review-fixture task.

The following ownership rules are plan-level invariants:

- Only `state` reads or persists canonical CLI/database state.
- Only `transaction` applies graph or analyzer-suite mutations.
- Only `effective` prepares an attached sidecar for ordinary invocation.
- Only `analysis` constructs and runs the configured analyzer suite.
- Only `view` constructs bounded contexts from verified user-selected
  identities.
- Only `locator` and `reference` interpret entry locators, aliases, and `HEAD`.
- Query predicate semantics remain in core `NodeQuery`; the CLI only parses,
  constructs, selects, explains, and renders.
- Type correctness remains in TypeCheck; CLI type editing only constructs and
  patches stored declarations.
- Ordinary commands return structured results and do not print directly.
- Interactive mode consumes the same application services as batch commands.
- GraphViz consumes immutable verified graph/view input and cannot save graph
  or CLI state; later effective graphs must enter through the same read
  boundary.
- A command adapter may coordinate services but may not reproduce their
  lifecycle logic.

## Stage map

| Stage | Reviewable outcome | Architectural boundary established |
| ----- | ------------------ | ---------------------------------- |
| 1 | Complete help/grammar and explicit unavailable behavior | Executable, parsing, dispatch, and errors |
| 2 | Initialize canonical state and inspect bootstrap diagnostics | Storage and shared analysis |
| 3 | Generate review history and inspect/query it in bounded views | Identity, references, views, and read output |
| 4 | Export deterministic DOT or SVG from the review graph | Read-only visualization |
| 5 | Apply bootstrap Suggestions safely and manage messages | Shared mutation transaction |
| 6 | Add/connect graph data and configure analyzers | Ordinary mutation adapters and shared payload input |
| 7 | Bind exact aliases and consume guarded query selections | Durable conveniences over verified identity |
| 8 | Inspect, create, and patch stored type declarations | Type-edit convenience without semantic duplication |
| 9 | Attach, browse, diagnose, dump, and commit sidecars | Effective-graph preparation |
| 10 | Browse and review selected operations in a TTY | Interactive client over shared services |
| 11 | Demonstrate complete cross-feature correctness | Acceptance and release-readiness evidence |

## Stage 1 — Executable shell and complete command grammar

- [x] **Stage implementation complete (implementor only)**
- [X] **Human review accepted (reviewer only)**
- [x] **Exit gate (implementor only): Stage 2 may begin**

### Description

Create a testable executable shell and expose the complete accepted command
grammar through clap. The only fully operational behaviors at this stage are
help, version information, argument validation, format selection, and clear
reporting that later functionality is unavailable.

This stage lets the reviewer assess command names, hierarchy, help text, mutual
exclusions, and option placement before persistence or graph behavior makes
changes expensive.

### Included scope

- A minimal `main.rs` that initializes logging, calls the library entry point,
  and maps its result to a process exit code.
- A reusable library entry point for in-process CLI tests.
- The full top-level and nested clap grammar from `SPEC.md`, including the
  `interactive` subcommand.
- Central application-error and command-outcome types.
- Initial `auto`, `table`, `lines`, and `json` format parsing.
- Explicit unavailable outcomes for every command that is not yet implemented.
- Dependency declarations needed by the executable shell.
- Logging configured through `env_logger`, defaulting to `trace` and writing to
  stderr.

### Deferred behavior

- State-file resolution, loading, initialization, or saving.
- Analyzer construction and graph access.
- All command effects and listings other than help/version.
- Final table, line, and JSON schemas.
- Interactive terminal behavior.

Every parsed but unimplemented command exits unsuccessfully, names the
unavailable operation, and performs no filesystem mutation.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `Cli` | Complete root clap grammar and subcommand selection | `cli`; one invocation |
| `Command` | Typed top-level command dispatch | `cli`; produced by parsing |
| `OutputFormat` | Accepted output-mode vocabulary | `cli` or `output`; one invocation |
| `AppError` | Stable application-level failure categories and error chaining | `error`; returned across application boundaries |
| `CommandOutcome` | Presentation-neutral result of one command | `app` or `output`; consumed before invocation ends |
| `Io` | Injectable standard streams and TTY capability, if needed | library entry point; one invocation |

### Invariants

- `main.rs` contains no command-specific behavior.
- clap parsing performs no state or graph access.
- A command handler does not print directly.
- `interactive --format json` is rejected.
- Invalid argument combinations fail before any command service is called.
- Every unavailable mutation has a nonzero outcome and no filesystem effects.
- Help text describes accepted semantics rather than current implementation
  shortcuts.
- Logging never contaminates structured stdout.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `run_from(...) -> Result<CommandOutcome, AppError>` | Execute one invocation without spawning a child process |
| `dispatch(command, app) -> Result<CommandOutcome, AppError>` | Route one parsed command to its isolated adapter |
| `AppError::Unavailable { operation }` | Distinguish intentionally deferred behavior from an internal failure |
| `render_error(...)` | Produce consistent human and JSON-compatible failure output |

### Resulting architecture

```text
main -> run_from -> clap parse -> command adapter -> unavailable outcome
```

Later stages replace unavailable adapters incrementally. They may not move
business logic back into `main.rs` or clap types.

### Implementation checklist

- [x] Add the library entry point, central error type, and thin executable.
- [x] Define the complete root and nested clap command tree from `SPEC.md`.
- [x] Split top-level command adapters into `commands`.
- [x] Add explicit unavailable handlers for deferred commands.
- [x] Implement output format and interactive-subcommand option validation.
- [x] Add test stream/TTY injection or an equivalent test seam.
- [x] Add focused grammar, help, conflict, and unavailable-command tests under
      `test::test2`.

### Verification

#### Automated checks

- [x] Run `cargo test -p lifecore-cli`.
- [x] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [x] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.

#### Manual walkthrough

- [x] Inspect `lifecore --help` and help at every command level.
- [x] Confirm representative invalid flag combinations fail clearly.
- [x] Invoke a representative deferred read command and mutation command.
- [x] Confirm neither invocation creates `LifeCoreState.json`.

### Review questions

- Is the command hierarchy discoverable without referring back to `SPEC.md`?
- Are dry-run, `--commit`, selection-action, and `--expect-head` options placed
  where a user would expect them?
- Are unavailable operations unmistakable rather than appearing to succeed?

### Stage gates

- [x] **Implementation gate (implementor only):** the full grammar, unavailable behavior, focused
      tests, checks, and walkthrough are complete.
- [X] **Review gate (reviewer only):** command-surface feedback is incorporated or explicitly
      deferred by agreement.
- [x] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 1 item blocks state work.

## Stage 2 — Canonical state, initialization, and diagnostic inspection

- [x] **Stage implementation complete (implementor only)**
- [X] **Human review accepted (reviewer only)**
- [x] **Exit gate (implementor only): Stage 3 may begin**

### Description

Implement the canonical state envelope, safe storage boundary, analyzer
registry, and `init` workflow. Add read-only `check` and `message show` so a
newly initialized empty graph exposes the TypeCheck bootstrap Suggestion with a
stable message ID.

The built-in `Interactive` analyzer is registered under its final name but may
be an explicitly documented internal no-op until ordinary parent/edge workflows
make its behavior reviewable.

### Included scope

- Authoritative state-path resolution and version-1 state envelope.
- Immutable UUIDv7, fingerprint/message-format version fields, empty alias and
  dismissal registries, optional sidecar path, and reserved metadata.
- Strict canonical load checks, including basic validation and persisted
  `Mock` rejection.
- No-clobber atomic `init`.
- Built-in registration of `Type_Check` and the placeholder `Interactive`
  analyzer.
- Complete analyzer-suite construction and execution over valid prefixes.
- Canonical message encoding, deduplication, deterministic sorting, display
  handles, and the read-only message universe.
- `check` default display and `message show`.
- Initial structured transaction/listing output sufficient for `init`, `check`,
  and `message show` in every output mode.

### Deferred behavior

- Message dismissal, restoration, and suggestion application.
- Entry locators in diagnostic locations; empty-state/context diagnostics are
  sufficient for the bootstrap workflow in this stage.
- Attached-sidecar preparation; the metadata field exists but cannot yet be
  created through the CLI.
- Graph mutations after initialization.
- Real connection-recreation behavior in the `Interactive` analyzer.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `PersistedState` | Exactly two top-level values: graph and CLI metadata | `state`; serialized canonical record |
| `CliStateV1` | Versioned UUID, format versions, aliases, dismissals, sidecar path, and reserved data | `state`; persisted |
| `StoredEntryLocatorV1` | Serialized index and full digest shape reserved for aliases | `state`; persisted but initially empty |
| `StatePath` | Authoritatively resolved state-file location | `state`; one invocation |
| `StateStore` | Canonical load, no-clobber creation, and later atomic replacement | `state`; one invocation |
| `AnalyzerRegistry` | Registered analyzer factories known to the binary | `analysis`; one invocation |
| `AnalysisReport` | Raw configured-suite output and execution metadata | `analysis`; one analysis run |
| `MessageId` | Full versioned SHA3-512 diagnostic identity | `messages`; reproducible value |
| `MessageRecord` | Analyzer message plus ID, severity, location, relevance, and visibility facts | `messages`; one universe |
| `MessageUniverse` | Deduplicated, sorted output before presentation filters | `messages`; one analysis run |

### Invariants

- A successfully loaded state has supported versions, a valid UUID, a basically
  valid graph, and no `Mock`.
- `LIFECORE_STATE_FILE_PATH`, when present, is authoritative; failure never
  falls back to the default file.
- `init` is the only operation that creates a state file and never replaces an
  existing path.
- Atomic writes use the destination directory and no implicit file locking is
  claimed.
- The state UUID does not change during ordinary load/render operations.
- Analyzer factories are registered separately from persisted name/config
  pairs.
- A configured analyzer without a registered factory is an execution error.
- The message universe is deduplicated before prefix resolution or filtering.
- Message identity sorts relevance but preserves SuggestedAction order.
- `check` always retains Inconsistencies.
- Read-only commands do not change state-file bytes.
- `auto` uses table presentation only when the relevant output is a TTY.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `StatePath::resolve(environment, cwd)` | Apply authoritative environment/default path rules |
| `StateStore::initialize(state)` | Atomically create version-1 state without clobbering |
| `StateStore::load()` | Deserialize and validate canonical state |
| `AnalyzerRegistry::built_in()` | Register TypeCheck and Interactive factories |
| `run_analysis(graph, registry)` | Execute initialization and every valid historical prefix |
| `MessageUniverse::from_analysis(...)` | Canonicalize, identify, deduplicate, and sort diagnostics |
| `MessageUniverse::resolve(prefix)` | Resolve one prefix before presentation filtering |

### Resulting architecture

```text
init  -> StateStore::initialize
check -> StateStore::load -> analysis -> MessageUniverse -> output
show  -> StateStore::load -> analysis -> MessageUniverse::resolve -> output
```

No later command may deserialize state directly or construct a private analyzer
runner.

### Implementation checklist

- [x] Add required path dependencies and serialization, hashing, UUID, logging,
      and output dependencies to `lifecore-cli`.
- [x] Implement the versioned state model and strict state-path resolution.
- [x] Implement no-clobber initialization and canonical-load validation.
- [x] Build and register TypeCheck and placeholder Interactive factories.
- [x] Implement analysis execution and stable message identities.
- [x] Implement `init`, default `check`, and `message show`.
- [x] Implement deterministic human and JSON output for these workflows.
- [x] Add tests for path authority, versions, UUID preservation, no-clobber,
      persisted `Mock`, missing factories, message stability, deduplication, and
      prefix ambiguity.

### Verification

#### Automated checks

- [x] Run `cargo test -p lifecore-cli`.
- [x] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [x] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [x] Run `cargo test --workspace` because this stage first integrates core and
      TypeCheck.

#### Manual walkthrough

- [x] Initialize a temporary state and inspect its exact two-part JSON envelope.
- [x] Confirm a second `init` refuses to replace it.
- [x] Run `check` and observe the bootstrap Suggestion and stable short handle.
- [x] Run `message show` with a valid prefix and an invalid prefix.
- [x] Compare state-file bytes before and after all read-only commands.

### Review fixture generators

- [x] **`stage2-bootstrap-state` (base CLI):** set
      `LIFECORE_STATE_FILE_PATH` to a new temporary `state.json`, run
      `cargo run -p lifecore-cli -- init`, then retain that generated state for
      envelope inspection and the `check`/`message show` walkthrough.
- [x] Confirm the generator uses only Stage 2 base-CLI behavior and can
      reproduce the review artifact without a committed JSON template or a
      dedicated fixture binary.

### Review follow-ups

- [x] Replace the `hex` dependency with a small local lowercase-hex encoder for
      fixed-size message digests, preserving the lowercase full and display ID
      representations.
- [x] Add focused coverage for exact lowercase encoding, including leading-zero
      bytes.
- [x] Remove `hex` from `lifecore-cli/Cargo.toml`, update `Cargo.lock`, and rerun
      the Stage 2 automated checks.
- [x] Replace the hand-written message-ID encoding primitives with a compact,
      explicitly versioned serialization DTO encoded by `postcard`; redefine
      the unaccepted version-1 wire format, preserve relevance sorting and
      action order, and lock the wire bytes and resulting digest with golden
      tests.
- [x] Remove analyzer-layer entry-count derivation and the validation or
      presentation logic built around it; analyzers and their consumers must
      use the bounded contexts and pointed identities already carried by
      diagnostics instead of trying to infer a graph-wide upper bound.
- [x] Move the placeholder `InteractiveAnalyzer`, its message/output types, and
      its factory into a dedicated module so each analyzer implementation owns
      its private scaffolding and future behavior.
- [x] Use `AnalyzerFactory::analyzer_name()` as the single authority for
      persisted analyzer names, removing duplicated name constants that can
      drift from their registered factories.
- [x] Add focused tests for analyzer-name registration/configuration and for
      diagnostic normalization without a separately derived entry count.

### Review questions

- Is initialization output clear about the non-fatal bootstrap Suggestion?
- Are message handles, severities, locations, and analyzer attribution readable?
- Do state load/version errors explain the broken invariant without suggesting
  an unsafe automatic rewrite?

### Stage gates

- [x] **Implementation gate (implementor only):** canonical state, analysis, message inspection,
      tests, checks, and walkthrough are complete.
- [X] **Review gate (reviewer only):** initialization and diagnostic feedback is incorporated or
      explicitly deferred by agreement.
- [x] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 2 item blocks identity and view
      work.

## Stage 3 — Entry identity, bounded views, inspection, and query

- [x] **Stage implementation complete (implementor only)**
- [X] **Human review accepted (reviewer only)**
- [x] **Exit gate (implementor only): Stage 4 may begin**

### Description

Establish the safe user-identity boundary before enabling graph mutations.
Implement entry locators, node references, bounded views, entry inspection, and
the complete initial node-query grammar. Since accepted prior stages produce
only an empty graph, add a narrow `lifecore-cli-fixture stage3` scenario that
constructs enough valid history for a reviewer to exercise these read
operations while ordinary CLI mutation remains unavailable.

### Included scope

- Versioned entry fingerprints and strict locator parsing.
- Safe latest and `--at` view construction.
- Alias lexical validation and resolver scaffolding; no persisted alias
  mutations yet.
- `HEAD` resolution in accepted node-reference positions, even though mutation
  consumers remain unavailable.
- `entry show`.
- `query nodes all|any|none`, every specified initial predicate, deterministic
  listing, `--select` without a mutation action, `entry inspect`, and
  `--explain`.
- View-aware locator, freshness, circumstance, field, endpoint, and alias
  display.
- Complete listing envelopes for table, lines, and JSON.
- A separate `lifecore-cli-fixture stage3` binary scenario that creates and
  reports a suitable canonical review state.

### Deferred behavior

- Query selection actions that mutate graph or CLI state.
- Alias set/remove.
- `add`, `connect`, and `--expect-head`.
- Effective sidecar graphs; the effective graph equals canonical state.
- Convenient population of a graph through the ordinary CLI.
- Fixture scenarios for later stages.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `EntryFingerprint` | Full versioned digest for one state/index/kind summary | `locator`; reproducible value |
| `EntryLocator` | Parsed index plus checked fingerprint prefix | `locator`; user input or display |
| `DurableNodeLocator` | Index plus full digest stored in CLI metadata | `locator`; persisted through aliases later |
| `NodeReference` | Lexically distinct `HEAD`, alias, or entry locator | `reference`; parsed input |
| `ViewSpec` | Latest or through-and-including one verified entry | `view`; command request |
| `GraphView` | Bounded `Context` plus display boundary and graph access | `view`; one command |
| `ParsedNodeQuery` | Core query plus original predicate presentation | `query`; one command |
| `QuerySelection` | Locator-backed selected node and its query/view evidence | `query`; one command |

### Invariants

- Locator parsing accepts only the specified decimal-index and lowercase-hex
  grammar and detects overflow.
- Resolution checks index, fingerprint, expected kind, and view membership in
  that order.
- An input containing `:` never falls back to alias lookup.
- Aliases and locators are lexically disjoint; `HEAD` is reserved exactly in
  uppercase.
- `HEAD` resolves to the highest-index visible node, never the latest edge.
- Stable identity positions reject `HEAD` and aliases where the specification
  requires a locator.
- Contexts are constructed only from verified indices through `view`.
- Query results sort by ascending entry index.
- Query combinator empty identities come from core behavior, not CLI special
  casing.
- Missing, wrong-kind, and out-of-range field paths are non-matches.
- `--select` rechecks membership in the same invocation.
- `--explain` can explain a non-member and does not select it.
- The Stage 3 fixture is a small, self-contained construction using base core,
  Stage 2, and Stage 3 functionality rather than a preconstructed state or
  Stage 5 transaction behavior.
- The generated state is valid and rich enough to review historical views,
  freshness, locators, `HEAD`, selection, and query explanations.
- The fixture reports the state and useful locators it actually produces.
- Once Stage 3 is accepted, its fixture recipe remains unchanged except for an
  agreed correction.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `EntryFingerprint::for_entry(state_id, version, entry)` | Produce the full stable checksum |
| `EntryLocator::parse(text)` | Enforce strict locator grammar without graph access |
| `resolve_entry(graph, state_meta, locator, expectation, view)` | Perform ordered verification and return a safe pointed entry |
| `resolve_node_reference(...)` | Resolve locator, alias, or `HEAD` with position-specific policy |
| `GraphView::latest(graph)` | Construct the current bounded view |
| `GraphView::through(locator)` | Construct the smallest view including a verified entry |
| `parse_predicate(text)` | Adapt textual grammar to `NodeQueryPredicate` |
| `execute_query(graph, view, expression)` | Delegate evaluation and return deterministic results |
| `select_query_result(...)` | Re-evaluate membership and preserve selection evidence |
| `lifecore-cli-fixture stage3` | Build a small valid history using functionality available through Stage 3 and report its path and useful locators |

### Resulting architecture

```text
user identity -> locator/reference -> view -> core graph/query
                                      |
                                      +-> structured inspection output
```

Later commands must consume `GraphView`, `EntryLocator`, or resolved
`NodeReference` values rather than indexing the graph from raw text.

### Implementation checklist

- [x] Implement fingerprint encoding, display prefixes, and locator parsing.
- [x] Implement verified entry/node resolution and view construction.
- [x] Implement node-reference grammar and `HEAD` resolution.
- [x] Implement detailed entry inspection.
- [x] Implement the complete specified node-query predicate grammar.
- [x] Implement list, select/inspect, and explanation paths.
- [x] Extend shared output DTOs and renderers for entries, views, and queries.
- [x] Implement the self-contained Stage 3 fixture scenario without using
      later-stage services or a state template.
- [x] Have the fixture report enough generated state and locator information for
      the manual walkthrough.
- [x] Add focused tests for locator check order, lexical disjointness,
      historical freshness, empty combinators, predicate parsing, panic-free
      paths, deterministic ordering, selection rejection, and non-member
      explanation.

### Review fixture

- [x] Run `lifecore-cli-fixture stage3` and inspect both its construction recipe
      and its reported state/locators.
- [x] Use the generated state for the Stage 3 manual walkthrough.
- [x] Confirm the recipe uses only base, Stage 2, and Stage 3 functionality.

### Verification

#### Automated checks

- [x] Run `cargo test -p lifecore-cli`.
- [x] Run focused core query tests when CLI work exposes a core prerequisite.
- [x] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [x] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [x] Run `cargo test --workspace` if any core query/context API changes.

#### Manual walkthrough

- [x] Generate a fresh review state with `lifecore-cli-fixture stage3`.
- [x] Inspect the reported node and edge locators through the ordinary CLI.
- [x] Show the same entry in latest and historical views.
- [x] Exercise all/any/none queries, including an empty predicate list.
- [x] Select a member, reject a non-member, and explain a non-member.
- [x] Confirm `HEAD` identifies the latest visible node even when the latest
      fixture entry is an edge.
- [x] Try a correct index with a wrong digest and confirm no alias fallback.
- [x] Compare state bytes before and after all operations.

### Review questions

- Are locators readable and easy to copy without being mistaken for row IDs?
- Does every listing communicate its historical view clearly enough?
- Are selection rejection and predicate explanations useful for correcting a
  query?

### Stage gates

- [x] **Implementation gate (implementor only):** identity, view, inspection, query, Stage 3
      fixture, tests, checks, and walkthrough are complete.
- [X] **Review gate (reviewer only):** locator/query feedback is incorporated or explicitly
      deferred by agreement.
- [x] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 3 item blocks visualization
      work.

## Stage 4 — Read-only GraphViz generation and export

- [x] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): Stage 5 may begin**

### Description

Implement GraphViz immediately after the read and identity boundary so it can
help reviewers validate every later graph feature. Keep deterministic DOT
generation separate from output-file handling and the external `dot` process.
Use the Stage 3 fixture to review canonical latest and historical views.

The initial interface must consume immutable graph/view input without taking
ownership of canonical-state loading. It must also leave room for the later
sidecar stage to supply an effective graph through the same read boundary;
GraphViz must not depend on a not-yet-implemented `EffectiveGraph` type or bake
canonical-only preparation into the renderer.

### Included scope

- `graphviz TARGET` with `--at`, `--full`, `--fields`, and `--dot`.
- Pure deterministic DOT generation from canonical `GraphView` input.
- Fresh-only default and full family/parent visualization.
- Escaped `mrecord` labels with locators, stored type names, and optional fields.
- Direct DOT-file write or streamed `dot -Tsvg` conversion.
- Clear external-process and target-write errors.

### Deferred behavior

- Alias rendering, until the alias stage.
- Attached-sidecar/effective-graph rendering, until the sidecar stage.
- Styling or layout options beyond `SPEC.md`.
- Circumstance-provenance display RFC.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `GraphvizOptions` | Selected view, full/fresh, fields, and output kind | `graphviz`; one command |
| `DotDocument` | Generated complete DOT text | `graphviz`; one render |
| `GraphvizExporter` | Direct write or external SVG conversion | `graphviz`; one command |
| `GraphvizTarget` | Validated requested artifact path and DOT/SVG kind | command/graphviz; one command |

### Invariants

- DOT generation receives immutable graph data and a verified Stage 3 view.
- Graph preparation remains outside the renderer so canonical and later
  effective graphs can share the interface.
- GraphViz does not depend on transaction, alias, type-edit, or sidecar services.
- Alias decoration is absent in this stage rather than anticipated through
  unimplemented alias behavior.
- Default output includes only fresh nodes and edges in the selected view.
- Full output includes all visible nodes/edges, family clusters, and dashed
  parent relationships.
- Graph identifiers and labels are escaped safely and output ordering is
  deterministic.
- `--dot` never invokes the external `dot` executable.
- SVG mode streams DOT over standard input without creating a temporary DOT
  file.
- Artifact success or failure never changes canonical state.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `generate(graph, view, options) -> Result<DotDocument, AppError>` | Construct deterministic DOT from immutable graph/view input |
| `GraphvizExporter::write_dot(target, document)` | Write the requested DOT artifact |
| `GraphvizExporter::write_svg(target, document)` | Stream DOT to `dot -Tsvg` and write SVG |

### Resulting architecture

```text
StateStore -> GraphView -> pure DOT generation -> DOT file
                                          `----> dot stdin -> SVG file
```

Later graph sources may feed the same immutable view boundary; they may not
move loading, sidecar preparation, or persistence into `graphviz`.

### Implementation checklist

- [x] Define the immutable graph/view input contract without depending on the
      later `EffectiveGraph` type.
- [x] Implement deterministic default/full/fields DOT generation and escaping.
- [x] Implement direct DOT and streamed SVG export.
- [x] Wire the command adapter only to canonical state and Stage 3 view services.
- [x] Add focused tests for historical views, fresh/full behavior, escaping,
      deterministic bytes, missing/failing `dot`, and unchanged state.

### Verification

#### Automated checks

- [x] Run `cargo test -p lifecore-cli`.
- [x] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [x] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.

#### Manual walkthrough

- [x] Generate default and full DOT from the Stage 3 fixture in latest and
      historical views.
- [x] Inspect field values containing GraphViz-special characters.
- [x] Generate SVG when `dot` is available and inspect the artifact.
- [x] Exercise missing/failing `dot` and unwritable-target errors.
- [x] Compare canonical state bytes before and after every case.

### Review questions

- Does visualization make freshness, families, parentage, and edges easier to
  validate before mutation work begins?
- Does the renderer interface stay independent of canonical/effective graph
  preparation?
- Are locators legible without making node labels unwieldy?

### Stage gates

- [x] **Implementation gate (implementor only):** canonical DOT/SVG generation,
      tests, checks, and walkthrough are complete.
- [ ] **Review gate (reviewer only):** visualization and interface feedback is
      incorporated or explicitly deferred by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 4 item blocks transaction
      work.

## Stage 5 — Shared transaction engine and complete message workflow

- [ ] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): Stage 6 may begin**

### Description

Implement the single mutation lifecycle before exposing ordinary append
commands. Use it first for Suggestion application, allowing a reviewer to apply
TypeCheck's bootstrap Suggestion as a dry run or commit. Complete dismissal and
restoration at the same time so all message-state axes are reviewable together.

This stage establishes the most important safety boundary in the CLI: every
future graph or analyzer-suite mutation must be expressed as a plan and executed
through the same staging, analysis, application, expectation, and save path.

### Included scope

- Declarative graph and analysis-suite change plans.
- Loaded -> staged -> analyzed -> dry-run/applied -> saved transaction states.
- Complete-candidate rollback on append, basic-validation, analysis, apply,
  expectation, or save failure.
- Optional `--expect-head LOCATOR|EMPTY` on committing graph changes, including
  reread immediately before save.
- `message dismiss`, `message restore`, and `message apply`.
- Full message presentation filters and `check --dismissal-ids`.
- Suggestion application through ordered `SuggestedAction` plans.
- Automatic post-commit dismissal unless `--keep-message`.
- Transaction output containing state, entries, messages, and view.

### Deferred behavior

- Generic `add`, `connect`, and analyzer configuration commands.
- Query-attested mutation actions.
- Sidecar multi-entry planning.
- Actual connection-recreation suggestions from `Interactive`; it remains an
  internal no-op analyzer.
- Interactive review UI.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `PlannedChange` | Declarative node, edge, suite, or composite candidate operation | `transaction`; one request |
| `TransactionRequest` | Plan, commit intent, expectation, and related CLI-state edits | `transaction`; one command |
| `TransactionState` | Dry run, committed, or aborted lifecycle outcome | `transaction` and `output`; one execution |
| `TransactionResult` | Predicted/committed entries, diagnostics, view, and state | `transaction`; returned to command |
| `ExpectedHead` | Stable locator or explicit empty canonical graph | `transaction`; optional guard |
| `DismissalRecord` | Full message digest and identity-format version | `state`/`messages`; persisted |
| `MessageFilter` | Dismissed/resolved presentation choices | `messages`; one command |

### Invariants

- `transaction` is the only module that calls core provisional mutation and
  `apply` for a user-requested graph change.
- Every provisional append passes basic validation immediately.
- Any failure discards the whole candidate clone.
- Analysis always runs the complete configured suite over the complete staged
  candidate.
- Inconsistencies block apply and save; Warnings and Suggestions do not.
- Dry runs never alter graph, aliases, dismissals, or state-file bytes.
- Persisted graph state never contains `Mock`.
- `--expect-head` requires `--commit`, rejects dynamic `HEAD`, and compares the
  latest canonical entry including edges.
- The expectation check rereads canonical state immediately before saving.
- Inconsistencies cannot be dismissed or filtered from message rendering.
- Message prefixes resolve against the required unfiltered universe.
- Suggestion action order is preserved during staging.
- Applying a Suggestion dismisses it only after successful commit and save.
- CLI-state-only dismissal changes never apply a graph suffix.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `TransactionService::execute(state, request)` | Own the complete candidate lifecycle |
| `stage_change(candidate, plan)` | Append or configure one typed plan without saving |
| `analyze_candidate(candidate)` | Run the shared analyzer registry and enforce inconsistency gating |
| `verify_expected_head(store, expectation)` | Reread and compare canonical head immediately before save |
| `MessageUniverse::visible(filter, dismissals, latest_view)` | Apply presentation state after identity resolution |
| `plan_suggestion(message)` | Convert ordered SuggestedActions into one atomic transaction plan |
| `StateStore::save(candidate)` | Atomically replace canonical state after all gates |

### Resulting architecture

```text
command -> PlannedChange -> transaction
                         -> stage clone
                         -> basic validation
                         -> shared analysis
                         -> dry run OR apply
                         -> expect-head reread
                         -> StateStore::save
```

No later mutation command may call `add_mock`, `connect_mock`, `apply`,
`add_analyzer`, or `StateStore::save` directly.

### Implementation checklist

- [ ] Define plans, transaction requests/states/results, and expectation types.
- [ ] Implement staging, analysis gating, dry-run, apply, reread, and atomic
      save transitions.
- [ ] Implement full message visibility, history-resolution, and registry-only
      listing behavior.
- [ ] Implement dismiss/restore as CLI-state-only saves.
- [ ] Implement suggestion planning and atomic apply.
- [ ] Implement automatic dismissal and `--keep-message`.
- [ ] Extend human and JSON transaction output.
- [ ] Add focused tests for each rollback point, Mock exclusion, dry-run byte
      identity, expectation mismatch, edge heads, dismissal persistence,
      inconsistency rejection, prefix universes, orphan restoration, action
      ordering, and post-commit dismissal.

### Verification

#### Automated checks

- [ ] Run `cargo test -p lifecore-cli`.
- [ ] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [ ] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [ ] Run `cargo test --workspace`.

#### Manual walkthrough

- [ ] Dry-run the bootstrap Suggestion and confirm state bytes are unchanged.
- [ ] Commit it with `--expect-head EMPTY` and inspect its locator.
- [ ] Repeat with a mismatched expectation and confirm nothing is saved.
- [ ] Dismiss and restore a Warning or Suggestion, including an orphaned full
      dismissal ID fixture.
- [ ] Try to dismiss an Inconsistency and confirm it remains visible.

### Review questions

- Does transaction output distinguish planned, dry-run, committed, and aborted
  work clearly?
- Does `--expect-head` provide useful actual-versus-supplied feedback?
- Are dismissal, history resolution, and emission presented as distinct states?

### Stage gates

- [ ] **Implementation gate (implementor only):** transaction and message workflows, rollback
      evidence, tests, checks, and walkthrough are complete.
- [ ] **Review gate (reviewer only):** transaction/message feedback is incorporated or
      explicitly deferred by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 5 item blocks ordinary mutation
      commands.

## Stage 6 — Ordinary appends, analyzer configuration, and shared payload input

- [ ] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): Stage 7 may begin**

### Description

Enable normal graph growth through `add` and `connect`, and enable safe
analysis-suite extension through `analyzer add`. Introduce the recursive TOML
payload converter that later sidecars will reuse. Replace the placeholder
`Interactive` analyzer with its specified connection-recreation behavior now
that parent/edge workflows exist to demonstrate it.

### Included scope

- `add TYPE` with repeatable fields, optional parent, dry-run/commit, and
  expectation guard.
- `connect` with verified node-reference endpoints.
- Shared recursive TOML-to-`Payload` conversion.
- `analyzer list` and analysis-gated `analyzer add`.
- Built-in `Interactive` analyzer suggestions for eligible inherited
  connections.
- Complete transaction output for node, edge, and suite candidates.
- Locator output for every predicted and committed entry.

### Deferred behavior

- Persisted alias mutation and query-attested mutation.
- Type convenience commands.
- Sidecar file parsing, though it will later call `payload_input`.
- Generic add/connect in interactive mode.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `FieldAssignment` | One validated, uniquely named CLI body field | `payload_input`; one add request |
| `PayloadInput` | Recursive supported TOML payload representation/conversion | `payload_input`; parse-local |
| `AddRequest` | Type, body, optional verified parent, and transaction options | `commands::add`; one command |
| `ConnectRequest` | Verified from/type/to plus transaction options | `commands::connect`; one command |
| `AnalyzerChange` | Candidate name/config addition to a cloned suite | `transaction`/analyzer command; one plan |
| `InteractiveAnalyzer` | Built-in analyzer over child-node-ending windows | `interactive_analyzer`; analyzer instance |

### Invariants

- `add` and `connect` resolve all references in the current effective view
  before constructing a plan.
- TypeCheck, not the command adapter, decides semantic type compatibility,
  override permission, and edge endpoint permission.
- Repeating one field name in an `add` command is an error.
- The payload converter accepts only strings, signed integers, `sequence`, and
  `ordered_set`.
- Ordered-set duplicates and unknown/ambiguous constructors are errors.
- Bare arrays, booleans, floats, arbitrary tables, and TOML time values are
  rejected deliberately.
- `analyzer add` analyzes the complete candidate suite before save.
- Missing analyzer factories never persist configuration.
- Analyzer listing preserves stored order.
- Interactive connection suggestions suppress stale opposite endpoints and
  already recreated equivalent fresh connections.
- All mutation commands use `TransactionService`.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `parse_field_assignment(text)` | Parse exactly one TOML root key and supported payload |
| `payload_from_toml(value)` | Recursively convert the shared payload grammar |
| `AddRequest::into_plan(...)` | Produce one validated node append plan |
| `ConnectRequest::into_plan(...)` | Produce one validated edge append plan |
| `plan_analyzer_add(current_suite, name, config)` | Clone and extend configuration for transaction analysis |
| `InteractiveAnalyzer::analyze_inner(...)` | Detect eligible parent connections and emit ordered actions |

### Implementation checklist

- [ ] Implement the shared TOML field/payload parser and focused error types.
- [ ] Implement `add` through verified references and transaction plans.
- [ ] Implement `connect` through verified references and transaction plans.
- [ ] Implement analyzer listing and candidate-suite addition.
- [ ] Implement the actual built-in Interactive analyzer and factory.
- [ ] Add trace/debug logs around staging, analysis, apply, expectation, and
      save transitions without leaking payload data unnecessarily.
- [ ] Add focused tests for payload recursion/rejection, node/edge rollback,
      `HEAD`, parent and endpoint views, missing analyzer factories, suite
      ordering, and every Interactive suppression rule.

### Verification

#### Automated checks

- [ ] Run `cargo test -p lifecore-cli`.
- [ ] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [ ] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [ ] Run `cargo test --workspace`.

#### Manual walkthrough

- [ ] Dry-run and commit a nested-payload node.
- [ ] Add a child with `HEAD` and verify the resolved locator is printed.
- [ ] Connect two explicit node locators and confirm latest-entry expectation
      treats the resulting edge as head.
- [ ] Attempt invalid/duplicate TOML fields and confirm no append occurs.
- [ ] Attempt to add an unknown analyzer and confirm its configuration is not
      persisted.
- [ ] Produce and inspect an Interactive connection Suggestion.

### Review questions

- Is recursive payload syntax usable from an ordinary shell?
- Does mutation output show resolved parents/endpoints and predicted locators
  clearly enough to review before commit?
- Are Interactive suggestions specific enough to apply confidently?

### Stage gates

- [ ] **Implementation gate (implementor only):** ordinary appends, payload input, analyzer
      configuration, Interactive behavior, tests, checks, and walkthrough are
      complete.
- [ ] **Review gate (reviewer only):** append/analyzer feedback is incorporated or explicitly
      deferred by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 6 item blocks alias and guarded
      selection workflows.

## Stage 7 — Durable aliases and query-attested actions

- [ ] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): Stage 8 may begin**

### Description

Add persisted node aliases and complete the initial query-selection action
model. Aliases remain exact, rebindable conveniences; query-attested actions
provide a same-invocation membership guard rather than persisting result sets or
trusting row numbers.

### Included scope

- `alias list`, `alias set`, and `alias remove`.
- Alias resolution in ordinary node-reference positions.
- Historical alias status: current, stale, or outside-view.
- Query selection actions `entry inspect`, `alias set`, and `add-child`.
- Revalidation of query membership before an action consumes a selection.
- Immediate CLI-state-only persistence for alias changes.
- Alias decoration in GraphViz labels through the existing read-only renderer
  interface.

### Deferred behavior

- Query-attested `type override`, which belongs with the complete type editor.
- Temporary sidecar aliases.
- Saved named queries.
- Any automatic alias advancement to a fresh successor.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `AliasName` | Validated case-sensitive C-style identifier excluding `HEAD` | `reference`; input and persisted key |
| `AliasRegistry` | Name-to-full-durable-node-locator bindings | `state`/`reference`; persisted |
| `AliasStatus` | Current, stale, or outside selected view | `reference`/output; computed |
| `SelectionAction` | Typed action permitted after query membership attestation | `query`/commands; one invocation |

### Invariants

- Aliases store full durable node locators, never raw indices or implicit views.
- `HEAD` cannot be stored, shadowed, rebound, removed, or displayed as an
  alias.
- Alias set requires an explicit entry locator, not another alias or `HEAD`.
- An alias resolving to an edge or invalid fingerprint fails.
- An input containing `:` never falls back to alias lookup.
- Rebinding is explicit and does not track successors.
- Alias list sorts by name and always includes the locator.
- Historical listing retains aliases whose targets are outside the view.
- Alias changes use the CLI-state-only save path and cannot apply a graph suffix.
- Query-attested actions preserve the selected locator and recheck membership
  in the same invocation.
- `add-child` fixes the selected node as parent and cannot accept another parent.
- GraphViz displays visible aliases without taking ownership of alias resolution.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `AliasName::parse(text)` | Enforce lexical grammar and reserved-name rule |
| `AliasRegistry::bind(name, durable_locator)` | Create or deliberately replace one exact binding |
| `resolve_alias(name, graph, view)` | Reverify stored fingerprint, node kind, and membership |
| `alias_status(target, view)` | Report current/stale/outside-view without rebinding |
| `execute_selection_action(selection, action)` | Recheck query evidence and call the appropriate service |

### Implementation checklist

- [ ] Implement validated alias names and persisted registry operations.
- [ ] Integrate aliases into node-reference resolution.
- [ ] Implement alias listing across latest and historical views.
- [ ] Implement CLI-state-only set/remove saves and query-attested binding.
- [ ] Implement selected entry inspection and `add-child`.
- [ ] Add resolved alias decoration to GraphViz without moving alias lookup into
      the renderer.
- [ ] Add focused tests for reserved names, bad-locator non-fallback, edge
      rejection, rebind behavior, outside-view status, deterministic listing,
      byte-safe failures, membership changes, and selected-parent enforcement.

### Verification

#### Automated checks

- [ ] Run `cargo test -p lifecore-cli`.
- [ ] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [ ] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.

#### Manual walkthrough

- [ ] Bind two aliases to one node and use each in an ordinary node-reference
      position.
- [ ] Supersede a target and confirm the alias remains on the stale entry.
- [ ] List aliases in a historical view containing and excluding their targets.
- [ ] Bind through a query-attested selection and reject a non-member.
- [ ] Add a selected child and inspect the fixed parent locator.
- [ ] Render the graph and confirm visible aliases decorate the expected nodes.

### Review questions

- Is the exact, non-moving nature of aliases clear in listings and errors?
- Does query-attested action syntax feel like one coherent workflow?
- Is `outside-view` distinct enough from invalid or stale?

### Stage gates

- [ ] **Implementation gate (implementor only):** aliases and selection actions, tests, checks, and
      walkthrough are complete.
- [ ] **Review gate (reviewer only):** alias/selection feedback is incorporated or explicitly
      deferred by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 7 item blocks type editing.

## Stage 8 — Stored type inspection, creation, and patch-based override

- [ ] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): Stage 9 may begin**

### Description

Implement `type` as a convenience layer over ordinary stored `Type` nodes.
Reviewers can inspect copy-pasteable declarations, create new declarations, and
patch a declaration resolved by name and historical view or attested query.
Every candidate remains subject to the normal transaction and TypeCheck gates.

### Included scope

- `type list` and `type show`.
- `type create` using core `FieldDecl`, `ValueType`, and `FieldStatus` parsers.
- Direct `type override NAME` resolved by active declaration name and view.
- Query-attested `type override`.
- Ordered patch operations for fields and permissions.
- `--as`, conflict detection, and no-op patch rejection.
- Dry-run, commit, expectation, and structured output through shared services.

### Deferred behavior

- Type-impact and health-summary RFCs.
- Time-payload authoring RFC.
- Any CLI-side attempt to predict or bypass TypeCheck semantics.
- Type editing of unavailable/inconsistent sidecar views until sidecars are
  implemented.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `TypeCreateRequest` | New declaration assembled from ordered CLI options | `type_edit`; one command |
| `TypePatch` | Validated ordered field/permission changes and optional rename | `type_edit`; one command |
| `ResolvedTypeDeclaration` | Well-formed stored `Type` node plus selected view | `type_edit`; one resolution |
| `TypeListing` | Deterministically ordered active declaration presentation | `type_edit`/output; one view |

### Invariants

- Type commands append ordinary nodes whose stored type name is exactly
  `Type::TYPE_NAME`.
- Stored declarations are decoded with `Type::try_from_body`.
- Direct override resolves a unique active, well-formed declaration by name in
  the selected view.
- A historical source need only be fresh in that historical view.
- The resolved source node is always the appended child's parent.
- The declaration name is preserved unless `--as` is supplied.
- Conflicting patch operations on one logical item are rejected.
- Replacing a field retains its display position; additions retain option order.
- A patch producing an identical declaration is rejected.
- TypeCheck owns duplicate-name, existing-node impact, edge-permission, and
  override-permission decisions.
- Type listings sort by declaration name and entry index.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `resolve_active_type(graph, view, name)` | Find one fresh, well-formed stored declaration |
| `TypeCreateRequest::build()` | Construct a stored Type without duplicating core parsers |
| `TypePatch::validate()` | Detect conflicts and invalid/no-op combinations |
| `TypePatch::apply(source)` | Preserve order while producing the child declaration |
| `plan_type_append(type, parent, transaction_options)` | Serialize through `NodeBody` and use the shared transaction |
| `render_type_declaration(type)` | Produce deterministic copy-pasteable command syntax |

### Implementation checklist

- [ ] Implement active type resolution and deterministic listing.
- [ ] Implement locator-based type inspection and copy-pasteable rendering.
- [ ] Implement creation through core type parsers and serialization.
- [ ] Implement ordered patch representation, conflict checking, and application.
- [ ] Implement direct and query-attested overrides.
- [ ] Route every append through `TransactionService`.
- [ ] Add focused tests for historical/name resolution, malformed declarations,
      name preservation/rename, parent identity, conflict/no-op detection,
      insertion order, TypeCheck rejection, and query membership.

### Verification

#### Automated checks

- [ ] Run `cargo test -p lifecore-cli`.
- [ ] Run focused `lifecore` and `lifecore-typecheck` tests if parser or typed
      serialization prerequisites change.
- [ ] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [ ] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [ ] Run `cargo test --workspace`.

#### Manual walkthrough

- [ ] Create a type with literal, sequence, tuple, ordered-set, and OneOf fields.
- [ ] Show it and verify the output is copy-pasteable.
- [ ] Patch a field and permission while preserving prior ordering.
- [ ] Branch from a historical declaration with `--as`.
- [ ] Attempt a no-op/conflicting patch and a TypeCheck-invalid mandatory-field
      change; confirm nothing is saved.
- [ ] Perform the same override through a query-attested selection.

### Review questions

- Is direct name-and-view resolution understandable without hiding historical
  branching?
- Is declaration output suitable for editing and reuse?
- Are patch conflicts and TypeCheck impact errors attributed to the correct
  layer?

### Stage gates

- [ ] **Implementation gate (implementor only):** type inspection/editing, tests, checks, and
      walkthrough are complete.
- [ ] **Review gate (reviewer only):** type-workflow feedback is incorporated or explicitly
      deferred by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 8 item blocks sidecar work.

## Stage 9 — Attached sidecars and effective-graph preparation

- [ ] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): Stage 10 may begin**

### Description

Implement sidecars as external, ordered, provisional addenda and expand
invocation preparation from canonical state to the complete effective-graph
state machine. All existing read-only operations consume an effective graph,
while mutations remain prohibited during attachment except sidecar commit and
detach.

This stage must preserve detach as the unconditional recovery path: it cannot
read or parse the attached file.

### Included scope

- `sidecar attach`, `detach`, `dump`, and `commit`.
- Sidecar TOML schema and reuse of the ordinary payload converter.
- Ordered entry/node/edge staging and complete-candidate rollback.
- Persisted and temporary alias resolution with precise per-entry `HEAD`
  capture.
- Effective states `EffectiveReady`, `SidecarDiagnosable`, and
  `SidecarUnavailable`.
- Effective-graph use by query, entry inspection, check/messages, type
  inspection, alias/analyzer listing, and GraphViz.
- Inconsistent-sidecar diagnosis through its final entry.
- Commit application, temporary-alias persistence, attachment clearing, and
  expectation guard.

### Deferred behavior

- Interactive read-only browsing of an attached ready sidecar until Stage 10.
- Any sidecar source rewriting or deletion.
- Replacement attachment without detach.
- User-authored time payloads.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `SidecarDocument` | Ordered deserialized `[[entry]]` source | `sidecar`; one preparation |
| `SidecarEntry` | One node plus ordered outgoing/back edges and optional alias | `sidecar`; source order |
| `SidecarPlan` | Fully resolved ordered append plan and temporary aliases | `sidecar`; one invocation |
| `TemporaryAliases` | Non-persisted names introduced during preparation | `sidecar`/`reference`; until commit |
| `EffectiveState` | Canonical-ready, sidecar-ready, diagnosable, or unavailable state | `effective`; one invocation |
| `EffectiveGraph` | Graph/view plus canonical boundary and provisional suffix metadata | `effective`; one invocation |
| `SidecarDumpItem` | Predicted ordered entry with locator and alias facts | `sidecar`/output; one dump |

### Invariants

- `attach` verifies only that the path exists, stores an absolute path, and does
  not parse the file.
- `detach` edits only attachment metadata and never reads the sidecar.
- Attach followed by detach with no other saved change restores identical
  state-file bytes.
- Sidecar entries stage strictly in source order: node, `connect_to`, then
  `connect_back_from`.
- Each entry captures `HEAD` before its node append and retains that capture for
  its parent and connections.
- Non-`HEAD` connection references resolve after the optional temporary alias is
  installed, permitting explicit self-links.
- Temporary aliases cannot shadow persisted or earlier temporary aliases.
- A parse or basic-validation/factory failure produces
  `SidecarUnavailable`; only detach is allowed.
- An emitted Inconsistency produces `SidecarDiagnosable`; `check` and detach are
  allowed but browsing is not.
- A valid sidecar produces `EffectiveReady`; read-only commands see the
  provisional suffix and never save it.
- While attached, singular graph/suite mutations, alias changes, and dismissal
  changes are rejected.
- `sidecar commit` rebuilds, validates, analyzes, applies, saves aliases with
  graph state, and clears attachment atomically.
- Predicted locators remain valid after commit because circumstance is excluded
  from fingerprints.
- The source TOML file is never modified or deleted.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `attach_path(state, path)` | Resolve and persist metadata without parsing |
| `detach(state)` | Remove metadata without touching the external file |
| `parse_sidecar(bytes)` | Decode ordered entries without graph mutation |
| `build_sidecar_plan(canonical, document, references)` | Resolve ordered nodes, edges, aliases, and per-entry HEAD |
| `prepare_effective(state, registry)` | Produce ready, diagnosable, or unavailable invocation state |
| `SidecarPlan::into_transaction()` | Reuse shared staging/apply/save behavior for commit |
| `dump_suffix(effective)` | Return only predicted sidecar entries in append order |

### Resulting architecture

```text
StateStore::load -> effective preparation
                   |-- canonical ----------------------> EffectiveReady
                   |-- invalid/factory failure --------> SidecarUnavailable
                   |-- analysis inconsistency ---------> SidecarDiagnosable
                   `-- valid provisional suffix -------> EffectiveReady

EffectiveReady -> existing read services
EffectiveReady -> sidecar commit -> transaction -> save
any sidecar state -> detach -> CLI-state-only save
```

Commands may not independently read an attached sidecar or append its entries.

### Implementation checklist

- [ ] Implement sidecar schema, parser, ordered planner, and temporary aliases.
- [ ] Implement attach/detach metadata operations with byte-restoration tests.
- [ ] Implement effective-state preparation and command-capability gating.
- [ ] Adapt all existing read-only commands to `EffectiveGraph`.
- [ ] Implement diagnosable check behavior through the final staged entry.
- [ ] Implement dump and commit through shared locator/transaction services.
- [ ] Feed ready effective views through the existing GraphViz read boundary.
- [ ] Persist temporary aliases only with successful commit.
- [ ] Add focused tests for recursive payload round trips, ordering, reference
      timing, self-links, shadowing, all effective states, prohibited mutations,
      recovery detach, predicted locator stability, effective-view
      visualization, and atomic commit.

### Verification

#### Automated checks

- [ ] Run `cargo test -p lifecore-cli`.
- [ ] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [ ] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [ ] Run `cargo test --workspace`.

#### Manual walkthrough

- [ ] Attach a valid multi-entry sidecar and browse its effective nodes, edges,
      aliases, types, queries, messages, and GraphViz output.
- [ ] Dump the suffix, commit it, and verify every predicted locator.
- [ ] Confirm singular mutation and dismissal operations are refused while
      attached.
- [ ] Attach then detach without another change and compare state bytes.
- [ ] Detach deleted, malformed, basically invalid, analyzer-unavailable, and
      analysis-inconsistent sidecars.
- [ ] Run `check` on an inconsistent but basically valid sidecar and confirm
      diagnostics include its final entry.

### Review questions

- Are canonical versus provisional entries unmistakable in every read view?
- Does sidecar failure guidance make detach easy to discover?
- Are per-entry `HEAD` and temporary alias rules predictable from dump output?

### Stage gates

- [ ] **Implementation gate (implementor only):** sidecar/effective-state behavior, recovery,
      tests, checks, and walkthrough are complete.
- [ ] **Review gate (reviewer only):** sidecar feedback is incorporated or explicitly deferred
      by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 9 item blocks interactive
      mode.

## Stage 10 — Menu-driven interactive review

- [ ] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): Stage 11 may begin**

### Description

Implement the bounded, TTY-only interactive client after its underlying query,
message, alias, type, and transaction services have been independently reviewed.
The `interactive` subcommand runs a menu state machine over those services, not
another textual command language and not a second mutation implementation.

### Included scope

- `lifecore interactive` dispatch, TTY validation, and session start/reload
  behavior.
- Browse, query results, locator-backed selection, entry inspection, aliases,
  selected add-child, type creation/override, message
  list/focus/dismiss/restore/apply, and staged review.
- Explicit commit/cancel with mandatory expected canonical head.
- Selection invalidation after commit or cancellation.
- Read-only browsing when a valid sidecar is attached.
- Clear startup rejection for unavailable or inconsistent sidecars.

### Deferred behavior

- Interactive sidecar attach/detach/commit.
- Analyzer configuration changes.
- Free-form sidecar authoring.
- Generic interactive `add` and `connect`.
- Persistent command history or hidden session state.
- JSON interactive output.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `InteractiveSession` | Loaded application services, current view, and canonical head | `interactive`; one TTY session |
| `Screen` | Explicit browse/query/selection/message/review state | `interactive`; changes within session |
| `InteractiveSelection` | Query plus durable locator and view evidence | `interactive`/`query`; until invalidated |
| `PlannedInteractiveAction` | Existing transaction or CLI-state request awaiting review | `interactive`; one review cycle |
| `ReviewDecision` | Explicit commit or cancel with no affirmative default | `interactive`; one prompt |

### Invariants

- The `interactive` subcommand requires interactive stdin and stdout and rejects
  JSON mode.
- It stores no hidden session or command history.
- It never invokes clap recursively.
- Every query selection is locator-backed and revalidated before use.
- Graph-changing commit/cancel invalidates all in-memory selections.
- Every interactive graph commit uses the canonical head captured at staging as
  a mandatory expectation.
- Review shows exact planned entries, predicted locators, and unfiltered
  Inconsistencies before offering commit.
- Commit has no default affirmative response.
- Cancel and EOF save nothing.
- Attached ready sidecars hide mutation choices; unavailable/inconsistent
  sidecars prevent startup with recovery guidance.
- Interactive operations call the same query, reference, type, message,
  transaction, state, and output-model services as batch commands.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `InteractiveSession::start(app, tty)` | Load effective state and enter Browse |
| `InteractiveSession::transition(input)` | Perform one explicit screen transition |
| `InteractiveSelection::revalidate(...)` | Rerun query and membership before action |
| `prepare_review(action)` | Use existing services to stage and analyze without saving |
| `commit_review(review, expected_head)` | Execute shared transaction with mandatory guard |
| `cancel_review(review)` | Discard the candidate and invalidate selections |
| `reload_after_save()` | Rebuild canonical/effective application state |

### Resulting architecture

```text
menu state -> existing request/service -> structured result -> menu renderer
                                      |
                                      +-> transaction review/commit
```

Interactive screens may select and sequence operations, but may not implement
their graph, message, or persistence semantics.

### Implementation checklist

- [ ] Implement TTY capability checks and explicit screen state machine.
- [ ] Implement browsing, query, selection, and inspection screens.
- [ ] Implement alias and type-edit action flows using existing requests.
- [ ] Implement message focus/dismiss/restore/apply flows.
- [ ] Implement planned-action review, cancel, guarded commit, and reload.
- [ ] Implement attached-sidecar read-only capability filtering.
- [ ] Add focused transition tests with scripted I/O, including EOF, invalid
      input, revalidation failure, concurrent-head mismatch, cancellation, and
      sidecar states.

### Verification

#### Automated checks

- [ ] Run `cargo test -p lifecore-cli`.
- [ ] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [ ] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [ ] Run `cargo test --workspace`.

#### Manual walkthrough

- [ ] Run `lifecore interactive`, then browse, query, select, inspect, and bind
      an alias in the TTY session.
- [ ] Create and patch a type through staged review.
- [ ] Dismiss, restore, dry-run, and commit a Suggestion.
- [ ] Cancel a planned action and compare state bytes.
- [ ] Cause a canonical-head mismatch before commit and confirm safe reload.
- [ ] Browse a valid attached sidecar and confirm mutation choices are absent.
- [ ] Attempt startup with unavailable and inconsistent sidecars.

### Review questions

- Does the menu remain small and navigable without becoming a second command
  language?
- Does Review provide enough information for a confident commit decision?
- Are cancellation, stale selection, and concurrent-head recovery unsurprising?

### Stage gates

- [ ] **Implementation gate (implementor only):** interactive state machine, safety behavior,
      tests, checks, and walkthrough are complete.
- [ ] **Review gate (reviewer only):** interactive feedback is incorporated or explicitly
      deferred by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and no unchecked Stage 10 item blocks final hardening.

## Stage 11 — Cross-feature acceptance, determinism, and release readiness

- [ ] **Stage implementation complete (implementor only)**
- [ ] **Human review accepted (reviewer only)**
- [ ] **Exit gate (implementor only): the planned CLI implementation may be declared complete**

### Description

Validate the assembled CLI as one product, close specification gaps discovered
by integration, and demonstrate failure atomicity and deterministic output
across feature boundaries. This stage should not introduce a new product
surface; material new behavior requires a plan amendment.

### Included scope

- End-to-end acceptance coverage for every criterion in `SPEC.md`.
- Stable JSON envelopes and deterministic human presentation.
- Cross-command and cross-process persistence workflows.
- Failure injection around parsing, analysis, apply, expectation, external
  process, and atomic save boundaries.
- Help/error consistency and logging review.
- Removal of obsolete scaffolding and unavailable handlers for completed
  commands.
- Final user walkthrough from empty state through meaningful graph history.

### Deferred behavior

- Type-impact/health summaries.
- Date, civil-time, and duration input.
- Circumstance provenance columns.
- Saved named queries.
- Broader interactive `add`, `connect`, sidecar, or analyzer editing.
- Performance refactors that do not block correctness.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `AcceptanceHarness` | Isolated state, sidecar, environment, and captured I/O for scenarios | tests; one scenario |
| `StateSnapshot` | Canonical bytes and semantic state used to prove no-change behavior | tests; before/after operation |
| `ExpectedOutput` | Stable structured assertions without incidental terminal details | tests; one scenario |

No new production-domain type should be necessary. If hardening requires one,
place it in the module that owns the invariant rather than a generic test or
utility module.

### Invariants

- Every `SPEC.md` acceptance criterion has automated evidence or a documented
  manual check with a reason automation is impractical.
- JSON listings use `{ "view": ..., "items": [...] }`.
- Transaction JSON contains its applicable view, state, entries, and messages.
- Full digests and display prefixes are present where specified.
- All presentation order is explicit and deterministic.
- No invalid user input, locator, view, field path, sidecar, or stored state
  causes a panic.
- Every failed or dry-run mutation leaves prior canonical bytes unchanged.
- Every successful graph save contains no `Mock`.
- No command handler contains a private mutation, analysis, reference, view, or
  sidecar lifecycle.
- User-visible help and errors match the implemented behavior.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `AcceptanceHarness::invoke(args)` | Run the library entry point in an isolated environment |
| `StateSnapshot::assert_unchanged()` | Prove byte-level non-mutation |
| `assert_json_contract(...)` | Verify stable envelopes, full identities, views, and transaction state |
| `assert_no_mock(...)` | Reload persisted state and enforce the canonical boundary |

### Implementation checklist

- [ ] Turn every acceptance criterion at the end of `SPEC.md` into a traceable
      test or documented manual verification.
- [ ] Add cross-feature scenarios for query-attested mutations, historical type
      editing, messages, aliases, sidecars, and GraphViz.
- [ ] Add independent acceptance scenarios for interactive commits.
- [ ] Audit deterministic iteration and rendering paths.
- [ ] Audit error variants for invariant-specific context and absence of panics.
- [ ] Audit all filesystem writes and state-byte rollback guarantees.
- [ ] Audit command adapters for duplicated shared-service logic.
- [ ] Remove completed-command unavailable scaffolding and dead transitional
      code.
- [ ] Review trace/debug/info/warn/error levels and default stderr behavior.
- [ ] Reconcile final help text, `SPEC.md`, and this plan; amend documentation
      for any explicitly accepted deviation.

### Verification

#### Automated checks

- [ ] Run `cargo test -p lifecore-cli`.
- [ ] Run `cargo test --workspace`.
- [ ] Run `cargo +nightly fmt --check -p lifecore-cli`.
- [ ] Run `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings`.
- [ ] Repeat deterministic-output scenarios and compare byte-identical JSON/DOT
      where applicable.
- [ ] Run the CLI test suite with selective `RUST_LOG` settings and confirm logs
      stay on stderr.

#### Manual walkthrough

- [ ] Initialize a fresh state and apply the Type bootstrap Suggestion.
- [ ] Create types, add nodes and edges, supersede nodes, and observe Interactive
      suggestions.
- [ ] Query, explain, select, alias, inspect, and patch historical declarations.
- [ ] Dismiss/restore/apply messages and verify reload behavior.
- [ ] Attach, diagnose, dump, browse, commit, and recover from sidecars.
- [ ] Produce DOT and SVG artifacts.
- [ ] Repeat representative review/commit/cancel flows in interactive mode.
- [ ] Exercise expectation mismatches and confirm all failed saves preserve the
      previous state.

### Review questions

- Does the complete CLI feel like one coherent tool rather than a collection of
  subcommand islands?
- Are state, view, identity, provisional work, and commit status consistently
  communicated across all output modes?
- Are any accepted-spec behaviors still surprising enough to warrant a plan or
  specification revision before declaring completion?

### Stage gates

- [ ] **Implementation gate (implementor only):** every accepted behavior and acceptance criterion
      has evidence, all checks pass, and the final walkthrough is complete.
- [ ] **Review gate (reviewer only):** final human feedback is incorporated or explicitly
      deferred by agreement.
- [ ] **Progression gate (implementor only):** both reviewer-owned boxes are
      checked in this file, and the implementor and reviewer agree that the
      `lifecore-cli` implementation described by `SPEC.md` is complete.
