# Planning analyzer

Status: proposal, not implemented. This document owns the contract of the
planning analyzer intended for `lifecore-prod`. [PLANNING-EXPLAINER](PLANNING-EXPLAINER.md)
walks through the same design by example. The graph and analyzer contracts it
relies on are [SPEC](SPEC.md), [ANALYZERS](ANALYZERS.md), and the declaration
encoding in [FILES §2.6](FILES.md#26-type-declaration-payloads).

## 1. Problem

A person records goals, the projects and tasks that lead to them, the habits
that sustain them, and the broad concerns that never finish. They want
breadth, so that everything that matters is in the graph and neglect is
visible, and pressure, so that recorded work pushes them to act under finite
capacity without the database becoming a guilt generator.

Two obstacles make a naive analyzer unsatisfying.

1. Recording is not committing. If a dependency edge means both "this matters"
   and "I owe action on this now", honest breadth produces chronic
   overcommitment and deleting edges becomes the cheapest way to feel feasible.
2. Findings are the wrong shape for pressure. A finding is a fact with a fixed
   identity. Pressure must renew after being ignored and must have magnitude.
   Faking renewal by varying certificates grows observations without bound;
   faking magnitude with escalating variants makes dismissal meaningless.

## 2. Design

1. **The review is the clock.** A `Review` node is the only tick the analyzer
   knows. Windows, deferrals, quotas, and ages are measured in reviews.
2. **A window is a macro-transaction.** Inside it the user revises items,
   re-points connections, and changes decisions freely. Appending a review
   **seals** the window, and the protocol rules are judged against the graph
   at that prefix only. Ordinary edits are never blocked by them.
3. **Identity is the exact revision, everywhere.** Edges, decisions, and
   events all attach to one revision and go historical when it is superseded,
   as [SPEC §5](SPEC.md#5-current-and-historical-data) says. The analyzer
   never resolves a connection through a revision tree. Re-pointing a
   connection after a revision is the ordinary LifeCore reconnection pattern,
   suggested by the general [RECONNECT](RECONNECT.md) analyzer and checked
   at the seal.
4. **Awareness is the dependency graph; commitment is a stance.** `Enables`
   and `Helps` edges say what matters and carry no obligation. A `Stance` node
   records one decision about one revision. Optional work is never owed; only
   what the user has pursued or committed to is in play.
5. **Status changes are events, not revisions.** Completion appends a `Done`
   about the revision, which stays current, so its edges stay current and a
   bypassed prerequisite is a plain check. A finished revision is final.
6. **Pressure is a valuation, not a finding.** Between reviews the analyzer
   ranks what is in play from graph structure. A ranking has no identity and
   cannot be dismissed; the only way to move an item is to append an entry.

## 3. Vocabulary

### 3.1 Items

| Type | Role | Fields | Terminal by |
| --- | --- | --- | --- |
| `Goal` | Sink | `title` text; `criterion` optional text; `deadline` optional timestamp | `sufficient` or `abandoned` stance |
| `Concern` | Sink, recurring attention | `title`; `quota` integer | `abandoned` |
| `Project` | Intermediate | `title`; `deliverable` optional text; `deadline` optional timestamp | `Done` event or `abandoned` |
| `Task` | Source, committable | `title`; `note` optional text | `Done` or `abandoned` |
| `Habit` | Source, recurring participation | `title`; `quota` integer | `abandoned` |
| `External` | Source, outside the user's control | `title` | `Done` or `abandoned` |

Fields without a stated policy are mandatory.

### 3.2 Edges

Every edge this analyzer interprets, with the complete set of permitted
endpoint types. `Enables` and `Helps` point the way work flows, from
contributor to what it serves.

| From | Edge | To | Meaning |
| --- | --- | --- | --- |
| `Task`, `Project`, `External` | `Enables` | `Task`, `Project`, `Goal` | Strict: the target cannot be `Done` or `sufficient` until the source is done. Propagates being in play. |
| `Task`, `Project`, `Habit`, `External` | `Helps` | `Task`, `Project`, `Goal`, `Concern` | Optional: a worthwhile contribution that blocks nothing and obligates nothing. |
| `Stance` | `About` | any item type permitted for its `kind` (section 3.5) | The decision's subject. |
| `Done` | `About` | `Task`, `Project`, `External` | The completed revision. Exactly one. |
| `Did` | `About` | `Habit`, `Concern` | The attended revision. |
| `Stance` with `kind = deferred` | `Until` | `Task`, `Project`, `External` | Ends the deferral early when the target is terminal. At most one. |

Any other endpoint pair for these four labels is an inconsistency. The union
of current `Enables` and `Helps` edges must be acyclic.

### 3.3 Events

| Type | About | Meaning |
| --- | --- | --- |
| `Done` | `Task`, `Project`, `External` | The revision is complete. For a project, an explicit acceptance. |
| `Did` | `Habit`, `Concern` | One act of participation or attention. |

Events carry `at` mandatory timestamp and `note` optional text. They are
never revised. A `Done` has exactly one `About` edge, since its target is
final; a `Did` may gain further `About` edges when its habit or concern is
revised (section 4). `at` exists because
the core rejects a node whose predecessor, type, and payload equal a current
node's ([SPEC §7.1](SPEC.md#71-append-checks) rule 5); the analyzer gives it no
meaning.

### 3.4 Reviews

`Review` carries `budget` mandatory integer, `at` mandatory timestamp, and
`note` optional text. It has no edges and is never revised. Its position is
what matters; `at` serves display and the deadline estimate (section 10).

### 3.5 Stances

`Stance` carries `kind` mandatory text, `at` mandatory timestamp, `cost`
optional integer, `horizon` optional integer, and `reason` optional text.
Changing a decision supersedes the stance with a successor (section 4). Its
edges are `About`, one or more over time, and for `deferred` at most one
`Until` (section 3.2).

| Kind | Applies to | Requires | Meaning |
| --- | --- | --- | --- |
| `committed` | `Task` | `cost` ≥ 0 | The plan for the window the sealing review opens. |
| `deferred` | any item | `horizon` ≥ 1 | Not now. Lapses after `horizon` seals, or earlier when the `Until` target is terminal. |
| `waiting` | `External` | `horizon` ≥ 1 | Externally gated; check in after `horizon` seals. |
| `pursued` | `Goal`, `Project`, `Habit`, `Concern` | | In play. Puts a goal's or project's strict subgraph in play too. |
| `sufficient` | `Goal` | all `Enables` sources done | Terminal: judged good enough. |
| `abandoned` | any item | | Terminal. |

`cost` on any other kind, or `horizon` on any other kind, is an inconsistency.

## 4. Identity, subjects, and state

Every connection names one exact revision. The **subject** of a stance or
event is the set of current revisions its current `About` edges target. When a
revision is superseded, the connections to it become historical and the
successor has none until they are re-asserted: for a dependency, a new edge
between the current revisions; for a stance or `Did`, a new `About` edge from
the existing node to the successor. The core permits both, since the node is
current and the successor is current, and the [RECONNECT](RECONNECT.md)
analyzer suggests exactly these edges. This is why `About` is not
one-to-one: re-pointing must keep the stance's position, because its
lifetime (section 6) is measured from it. A `Done` is never re-pointed,
because its target is final.

Changing a decision supersedes the stance: the successor carries the new
kind, its own `About` edge to the subject, and a lifetime measured from its
own position. The **current stance** of a revision is the current stance
whose current `About` edges include that revision; if the user appended
several root stances instead of superseding, the latest by position wins.
Nothing is validated across stances, and no lifetime is read from a stance's
ancestors. A revision's **state** is `abandoned` if its current stance is
`abandoned`; else `done` if
a `Done` currently targets it, or `sufficient` if its current stance is
`sufficient`; else `open`. The first two are **terminal**; only `done`
satisfies an `Enables` edge.

A terminal revision is final: a successor of a terminal revision is an
inconsistency at the successor's own prefix. New work is a new item. A split
is ordinary: each current revision carries its own connections, decisions,
and state.

The one history query the analyzer makes is `root_of`, to measure how long
an item has existed. All revisions of a tree share that age.

## 5. Windows and seals

```text
... [Review k] s s e s ... [Review k+1] ...
     ^ opens window k                   ^ seals window k, opens window k+1
```

- **Window k** is the run of positions after `Review k` up to and including
  `Review k+1`. Positions before the first review form window 0. Every
  `Review` entry is its own seal, including several in one plan; a second
  review in one plan seals an empty window.
- **Sealing** happens at the prefix whose newest entry is a `Review`. The
  protocol rules of section 8.4 and 8.5 are judged at exactly those prefixes,
  the way TypeCheck judges a type transition only at the successor's prefix.
- Stances appended in window k and sealed by `Review k+1` are the plan for
  window k+1. `Review k+1`'s `budget` bounds its sealed commitments.
- At a non-seal boundary, the **open window** is the run since the latest
  review. The valuation (section 9) reports on it.

The ritual is: look at the ranking, append decisions, events, and
reconnections, then append the review. If the seal is rejected, adjust and
retry. The transaction that fails is the one that claimed the window was
reviewed.

## 6. Lifetimes

Lifetimes count seals from a stance's position; no lineage is involved. The
**sealing review** of a stance is the first review after it.

- `committed` stands through the window its sealing review opens and lapses at
  the next seal.
- `deferred` with `horizon = n` stands through the n-th seal after its sealing
  review and lapses after it; it lapses earlier at any boundary where its
  `Until` target is terminal.
- `waiting` stands through `horizon` seals like `deferred`.
- `pursued` stands until superseded, or until a later root stance about the
  same revision outranks it.
- `sufficient` and `abandoned` are terminal.

Relative to a seal, a stance appended in the window being sealed is
**fresh**; one sealed earlier and still within its lifetime is **standing**.
Relative to a non-seal boundary, fresh means appended in the open window.
Fresh or standing stances are **decisions**; lapsed ones are history.

## 7. In play, ready, and the frontier

1. **Pursued items** are revisions whose current stance is `pursued`.
2. The **active set** is every pursued item plus every open revision reachable
   from a pursued `Goal` or `Project` by walking `Enables` backward, target to
   source, through open revisions. `Helps` never propagates: work reached
   only through a `Helps` edge is awareness until it is itself pursued. The
   **strict subgraph** of a pursued goal or project is that walk from it.
3. **Ready** means an open `Task` or `Project` in the active set whose
   `Enables` sources are all done.
4. The **frontier**, the revisions that need a decision, is:
   - every open `Goal` or `Concern` that is not pursued: record a sink and you
     owe a pursuit decision, not action;
   - every ready `Task` in the active set;
   - every ready `Project` in the active set;
   - every open `External` in the active set;
   - every pursued `Habit` or `Concern`.
5. A frontier revision is **decided** when it has a fresh or standing stance
   of an applicable kind other than `pursued`, or, for a habit or concern,
   when a `Did` in the window currently targets it. `pursued` puts an item in
   play; it never counts as this window's decision. Otherwise the revision is
   **undecided**.

Open revisions that are not ready are blocked by the graph and need no
decision. Pursued goals are not in the frontier.

## 8. Findings

Every rule names its judging prefix: the node's own prefix, the prefix of a
particular edge, or a seal. Judging once at a fixed prefix keeps replay
stable. Certificates name exact subject entries and never a boundary; seal
certificates also name the sealing review, so a condition that persists into
the next window is a distinct finding because the world holds a new review.

### 8.1 At a node's own prefix

Inconsistencies: unknown `kind`; `committed` without `cost`; `deferred` or
`waiting` without `horizon`; `cost` or `horizon` on a kind that forbids it;
negative `cost`, `horizon` below 1, negative `budget` or `quota`; a `Review`,
`Done`, or `Did` with a predecessor; a successor of a terminal revision.

### 8.2 At an `Enables` or `Helps` edge's prefix

Inconsistencies: endpoint types outside section 3.2; a terminal target; a
cycle the edge closes, one finding per strongly connected component with
sorted member identities.

### 8.3 At an `About` or `Until` edge's prefix

Inconsistencies: source not a stance or event, or target not an item; a kind
that does not apply to the target's type; `Until` from anything but a
`deferred` stance, or a second `Until`; a second `About` from a `Done`; any
`About` to a terminal revision; `Done` whose target has an `Enables` source
that is not done; `sufficient` whose target has an `Enables` source that is
not done. These are judged immediately because events cannot be taken back.

### 8.4 At a seal: inconsistencies

- **Undecided item.** A frontier revision that is undecided. One finding per
  revision; focus the revision.
- **Budget overrun.** The sum of `cost` over tasks in the active set whose
  current stance is a fresh `committed` exceeds the review's `budget`.
- **Broken plan.** A pursued `Goal` or `Project` whose strict subgraph holds
  no ready task, no ready project, and no open `External` with a fresh or
  standing `waiting` stance, and into which (the item or its strict subgraph)
  no pursued `Habit` currently `Helps`. Optional contributions never rescue a
  plan.

### 8.5 At a seal: sticky warnings

- **Shortfall.** A pursued `Habit` or `Concern` without a fresh or standing
  `deferred` stance, with fewer `Did` events in the window than `quota`.
  Subjects are the item, the review, and the window's `Did` events.
- **Dropped connection.** One per connection, whether an `Enables`, `Helps`,
  `About`, or `Until` edge, that touched a revision superseded in the window
  and has no equivalent current connection involving a current successor.
  A warning, not a gate, because dropping a connection is sometimes what the
  edit meant.

Sticky because each judges a closed window once; the next window gets its
own finding.

### 8.6 Suggestions

- **Bootstrap.** Nonsticky, re-emitted while the condition holds: one per
  canonical declaration of section 11 that is absent or not payload-equal to
  the current declaration of that name, adding or revising it.

Re-pointing connections after a revision is not this analyzer's job. The
general [RECONNECT](RECONNECT.md) analyzer suggests recreating a superseded
revision's edges on its successors, which covers `Enables`, `Helps`, `About`,
and `Until` alike; the planning analyzer only names what is still missing at
the seal (section 8.5). The analyzer never suggests stances or events.

## 9. Valuation

Between reviews the analyzer ranks revisions at the requested boundary. The
ranking covers the active set, the undecided sinks, and the **available
contributions**: open revisions outside the active set with a current `Helps`
edge into it. Rows are ordered by tier, then the tier's keys, then root
position ascending.

| Tier | Members | Keys |
| --- | --- | --- |
| 0 | Undecided frontier revisions | goals and concerns first; then fan-out, the distinct pursued goals reachable forward over `Enables` and `Helps`, descending |
| 1 | Open tasks whose current stance is a standing `committed` | `OpenFor` descending; then `DeadlineIn` ascending when present |
| 2 | Pursued habits and concerns below `quota` so far in the open window, without a fresh or standing `deferred` | shortfall descending |
| 3 | Revisions whose current stance is a standing `deferred` or `waiting` | lapsing at the next seal first; then `OpenFor` descending |
| 4 | Pursued goals and projects whose strict subgraph holds no task with a standing `committed` | seals since it last did, or since the `pursued` stance was sealed, descending |
| 5 | Available contributions | `OpenFor` descending |
| 6 | The rest of the active set | root position |

Reasons are structured values with rendered text: `Undecided`,
`OpenFor { seals }` (seals since the revision tree's root was appended),
`Standing { kind, seals_left }`, `LapsesAtNextSeal`, `DeadlineIn { reviews }`,
`DeadlinePassed`, `Starved { seals }`, `Shortfall { have, quota }`,
`Unselected`. Tests assert the structured value; clients show the text.

The valuation has no identity, no dismissal, and no persistence; it never
affects transaction acceptance; it is computed only at the requested
boundary, so it adds nothing to replay cost. Before the first review, tiers 1,
3, and 4 are empty and no deadline reason is produced.

## 10. Time

- **Deadlines.** A task's deadline is the nearest `deadline` over the projects
  and goals reachable forward over `Enables`. `DeadlineIn` is the time from
  the latest review's `at` to the deadline, divided by the median window
  length among the last `horizon_windows` consecutive review pairs whose
  `at` strictly increases, rounded down. It is omitted with fewer than two
  reviews or no increasing pair; a negative result is `DeadlinePassed`.
- **Stale review.** With `stale_review_after_days` set, the analyzer requires
  `evaluation_time` and errors without it. At every prefix it emits one
  nonsticky warning when the view holds at least one item and either no
  review exists (constant certificate) or the latest review's `at` is older
  than the threshold (certificate naming that review). Unset, evaluation
  inputs are never read.

Everything else counts seals. A user who stops reviewing gets no pressure
except that one nudge, which is a deliberate dependence on the ritual rather
than the clock.

## 11. Configuration and bootstrap

Type and edge names are canonical. Configuration is an empty string or a
strict JSON object; the empty string means:

```json
{
  "horizon_windows": 6,
  "stale_review_after_days": null,
  "first_sealed_review": null
}
```

Unknown fields, fractions, and negatives reject construction.
`first_sealed_review` is an entry position; seals before it are not judged
under sections 8.4 and 8.5, so that adopting the analyzer on a database that
already holds reviews, or upgrading to stricter rules, cannot lock the
database behind a historical inconsistency. The configuration string is part
of analyzer identity, so defaults should rarely change.

Bootstrap suggests ten declarations: `Goal`, `Concern`, `Project`, `Task`,
`Habit`, `External`, `Done`, `Did`, `Review`, `Stance`. Three of them:

```toml
[[entries]]
name = "task_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Task"
fields = { kind = "record", value = { title = ["mandatory", "text"], note = ["optional", "text"] } }
outgoing = ["Enables", "Helps"]
incoming = ["Enables", "Helps", "About", "Until"]
successors = []

[[entries]]
name = "stance_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Stance"
fields = { kind = "record", value = { kind = ["mandatory", "text"], at = ["mandatory", "timestamp"], cost = ["optional", "integer"], horizon = ["optional", "integer"], reason = ["optional", "text"] } }
outgoing = ["About", "Until"]
incoming = []
successors = []

[[entries]]
name = "review_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Review"
fields = { kind = "record", value = { budget = ["mandatory", "integer"], at = ["mandatory", "timestamp"], note = ["optional", "text"] } }
outgoing = []
incoming = []
successors = []
```

TypeCheck enforces field shapes and label permissions at endpoints. This
analyzer enforces what a declaration cannot say: the type at the opposite
endpoint, the kind vocabulary, the conditional fields, and the no-revision
rules, since same-type successors are always permitted ([SPEC §6](SPEC.md#6-type-declarations)).

## 12. Delivery without a core change

The valuation is not an analyzer output and needs no change to the analyzer
traits, the runner, or [ANALYZERS](ANALYZERS.md).

1. **Library.** `lifecore-prod` exports the analyzer, its factory, and a
   plain function `valuation(view, &config) -> Valuation` over a bounded
   `View` and a parsed configuration. The output is bounded by its view.
   `Valuation { boundary, ranked: [ { revision, tier, reasons: [ { reason, text } ] } ] }`
   is a Serde type owned by the crate; reasons are its own enum with rendered
   text beside each.
2. **Executable.** A `lifecore-plan` binary, or a subcommand of the CLI,
   opens the selected database read-only as a preview
   ([STORAGE](STORAGE.md)), reads the planning configuration string from the
   current analysis suite so that the ranking uses the configured analyzer's
   settings, and prints the ranking at the head or at an "as of" boundary as
   lines or JSON.
3. **Application later.** When the browser client wants the ranking, the
   application service adds one RPC method that calls the same function, and
   the client renders tiers and reason text with links into the inspector.
   Fixtures and the client contract then carry the `Valuation` shape.

Nothing else is needed: `root_of`, the all-edges selection for the
dropped-connection rule, newest-entry inspection, timestamp fields, and
suggestion actions exist.

## 13. Alternatives considered

- **Decision chains with root resolution.** An earlier draft resolved `About`
  edges to the revision root and made stances a revision chain per item, so
  deferral counts and commitment age came from lineage. It needed rules
  against split items, second root stances, and chains whose `About` roots
  diverged, and it ran two identity models at once. Exact revisions remove
  all of that; the lost deferral count is replaced by `OpenFor`.
- **Squashing in-window revisions at the seal.** Treating connections to any
  revision created in the window as connections to its final in-window
  successor would make two consecutive seals read one edge differently.
  Rejected for the re-point suggestion and the dropped-connection warning.
- **A `declined` stance for optional work.** Needed only while optional work
  was in the frontier. With `Helps` carrying no obligation, nothing has to be
  declined; unselected contributions stay visible in the valuation.
- **Pressure as findings with escalating certificates.** Unbounded
  observation growth and meaningless dismissal.
- **Completion as supersession.** Makes every dependency edge historical the
  moment a prerequisite is done.
- **Role mapping by configuration.** The configuration string is part of
  finding identity, so remapping invalidates every dismissal.
- **Wall-clock windows.** Evaluation time is one frozen value per run; replay
  would judge every past window with today's clock.
- **Stances as edges from the review.** Edges carry no payload and could not
  hold cost, horizon, or reason.
- **A valuation hook on the analyzer traits.** It would have put a
  presentation output into the runner contract and the shared message types.
  A library function and an executable give the same ranking with no change
  to [ANALYZERS](ANALYZERS.md).

## 14. Open questions

1. Should `pursued` on a habit or concern expire, so that pursuit itself is
   periodically reaffirmed, or is tier 2 pressure enough?
2. Should a pursued project that is ready need a decision at all, given that
   `Done` is the natural next step, or is tier 0 pressure enough?
3. Should the valuation be visible to other analyzers, or remain
   presentation-only?
4. Should **Dropped connection** become a gate once the client makes
   re-pointing a one-click step?
5. Whether `OpenFor` should count from the root's position or from the
   current revision's, which lets an edit reset age.

## 15. Acceptance tests

Structural assertions on typed findings or valuation reasons, with
history-sensitive cases checked before and after the relevant append.

1. Canonical declarations round-trip; bootstrap suggests each missing or
   differing declaration with the right predecessor.
2. Endpoint pairs of section 3.2 accepted; reversed and foreign pairs, a
   `Habit` as `Enables` source, and an edge into a terminal target rejected
   at the edge's prefix.
3. Cycles over `Enables`, `Helps`, and mixed reported once per component at
   the closing edge's prefix, not earlier.
4. `Done` with an open `Enables` source rejected at the `About` prefix;
   accepted after the source's `Done`; the edge stays current throughout. A
   successor of a done revision is rejected at its own prefix.
5. A split task yields two frontier revisions; a `Done` about one leaves the
   other open; both share `OpenFor`.
6. Revising a task makes its stance's `About` historical; without a new
   `About` edge to the successor, the revision is undecided at the seal and a
   dropped-connection warning names the `About` edge; with one, neither, and
   the stance keeps its lifetime.
7. Seal with an undecided ready task fails; a fresh `committed` or
   `deferred` makes it pass; a successor stance replaces its predecessor for
   budget and decision, and among root stances the latest wins.
8. Budget: equal passes, one over fails, only fresh `committed` about active
   tasks count, the latest stance per task counts.
9. `committed` lapses at the next seal; `deferred` stands exactly `horizon`
   seals and lapses early when its `Until` target is done or abandoned.
10. Work reached only through `Helps` is never frontier and appears as an
    available contribution; pursuing an optional project puts its strict
    subgraph in the frontier.
11. Shortfall counts the window's `Did` events, is suppressed by a `deferred`
    decision, is sticky, and a single `Did` decides a pursued habit.
12. Broken plan for a pursued goal with nothing ready and nothing waited on;
    not rescued by a `Helps` source; rescued by a `waiting` external or a
    pursued habit that helps.
13. Valuation tiers and reasons for the explainer's examples; identical
    across runs; unaffected by dismissals; tiers 1, 3, 4 empty before the
    first review; `DeadlineIn` omitted with fewer than two increasing review
    timestamps.
14. With `stale_review_after_days` set and no evaluation time, analysis
    errors; unset, no evaluation time is required. `first_sealed_review`
    suppresses earlier seal findings.
15. Full-prefix replay: appending a review, stance, event, or edge never
    changes findings at earlier prefixes; two reviews in one plan are two
    seals.
