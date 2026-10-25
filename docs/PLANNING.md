# Planning analyzer

Status: proposal, not implemented. This document owns the contract of the
planning analyzer intended for `lifecore-prod`. [PLANNING-EXPLAINER](PLANNING-EXPLAINER.md)
walks through the same design by example, with every entry written as a
change file. The graph and analyzer contracts it
relies on are [SPEC](SPEC.md), [ANALYZERS](ANALYZERS.md), the declaration
encoding in [FILES §2.6](FILES.md#26-type-declaration-payloads), and the
general reconnection analyzer of [RECONNECT](RECONNECT.md). Section 13
records each design decision with the alternative it rejected and the reason;
the rules link to them by number.

## 1. Problem

A person records goals, the projects and tasks that lead to them, the habits
that sustain them, and the broad concerns that never finish. They want
breadth, so that everything that matters is in the graph and neglect is
visible, and pressure, so that recorded work pushes them to act under finite
capacity without the database becoming a guilt generator.

Two obstacles make a naive analyzer unsatisfying.

1. Recording is not committing, and it is not free either. If a dependency
   edge means both "this matters" and "I owe action on this now", honest
   breadth produces chronic overcommitment and deleting edges becomes the
   cheapest way to feel feasible. If recording obliges nothing at all, an item
   recorded and never decided about is deferred with no horizon, and the
   database becomes a bucket list.
2. Findings are the wrong shape for pressure. A finding is a fact with a fixed
   identity. Pressure must renew after being ignored and must have magnitude.
   Faking renewal by varying certificates grows observations without bound;
   faking magnitude with escalating variants makes dismissal meaningless.

## 2. Design

Seven principles, each a decision recorded in section 13. The node types, edge
labels, and stance kinds they name are defined in section 3; the rules that
use them begin in section 4.

1. **The review is the clock.** A `Review` node is the only tick the analyzer
   knows. Windows, deferrals, quotas, and ages are measured in reviews
   (decision 1).
2. **A window is a macro-transaction.** Inside it the user revises items,
   re-points connections, and changes decisions freely. Appending a review
   **seals** the window, and the protocol rules are judged against the graph
   at that prefix only. Ordinary edits are never blocked by them
   (decision 2).
3. **Identity is the exact revision, everywhere.** Edges, decisions, and
   events all attach to one revision and go historical when it is superseded,
   as [SPEC §5](SPEC.md#5-current-and-historical-data) says. The analyzer
   never resolves a connection through a revision tree. Re-pointing a
   connection after a revision is the ordinary LifeCore reconnection pattern,
   suggested by the general [RECONNECT](RECONNECT.md) analyzer and checked
   at the seal (decisions 3 and 4).
4. **Every open item is in play; commitment is a stance.** `Enables` and
   `Helps` edges say what matters and oblige no action. A recorded item is
   **in play**, owed a decision but not action, until a `Done` finishes it
   or a `Stance`, one decision about one revision, sets it aside: `deferred`
   for a while, `abandoned` for good, or `sufficient` when a goal is good
   enough. A deferral makes the item **dormant**, and with it everything
   that serves only dormant or finished items, so one decision quiets a
   whole branch. Only a `committed` stance owes action (decisions 7, 40, 41,
   and 42).
5. **Status changes are events, not revisions.** Completion appends a `Done`
   about the revision, which stays current, so its edges stay current and a
   bypassed prerequisite is a plain check. A finished revision is final
   (decisions 9 and 11).
6. **Pressure is a valuation, not a finding.** Between reviews the analyzer
   ranks what is in play from graph structure. A ranking has no identity and
   cannot be dismissed; the only way to move an item is to append an entry
   (decisions 12 and 13).
7. **Planning owns its vocabulary and acts only where it appears.** Its ten
   type names and four edge labels mean the same thing in every database that
   configures it. The analyzer judges entries of that vocabulary, seals, and
   declarations of its types, and nothing else. A user may amend those
   declarations with fields and labels of their own, as long as the analyzer
   can still do its job (decisions 35 to 37).

## 3. Vocabulary

The analyzer introduces ten node types in two groups.

- **Items** are the things the plan is about: `Goal`, `Concern`, `Project`,
  `Task`, `Habit`, and `External`. Items are connected to one another by
  `Enables` and `Helps`, are revised as the plan changes, and each revision
  has a state (section 4).
- **Bookkeeping nodes** record what the user decided, did, and reviewed. A
  `Stance` records a decision about an item, the **events** `Done` and `Did`
  record that an item was completed or attended to, and a `Review` records
  that the user reviewed the plan, which seals a window (section 5). Stances
  and events point at items with `About` and `Until`. None of the four labels
  of section 3.2 targets a bookkeeping node, and none touches a review.

Planning owns these ten type names and the four edge labels of section 3.2 in
every database that configures it: every `Review` is a seal and every `About`
edge is judged by section 8.3, whatever the user meant by them (decision 35).
The declarations are the user's to amend within the limits of section 3.4.

### 3.1 Node types

An item's role is its place in the flow of work (section 3.2): a **sink** is
never the source of an `Enables` or `Helps` edge, a **source** is never the
target of one, and an **intermediate** can be both. The Fields column gives
the canonical declarations of section 11; a field without a stated policy is
mandatory. **Revised** says whether a node
may gain successors: an item may until it is terminal, by a `Done` event or a
terminal stance kind (section 4).

| Type | Role | Fields | Revised |
| --- | --- | --- | --- |
| `Goal` | Item, sink: an outcome | `title` text; `criterion` optional text; `deadline` optional timestamp | Until terminal by `sufficient` or `abandoned` |
| `Concern` | Item, sink: an area that needs recurring attention | `title` text; `quota` integer | Until terminal by `abandoned` |
| `Project` | Item, intermediate: a deliverable | `title` text; `deliverable` optional text; `deadline` optional timestamp | Until terminal by `Done` or `abandoned` |
| `Task` | Item, intermediate: one piece of work, the only committable type | `title` text; `note` optional text | Until terminal by `Done` or `abandoned` |
| `Habit` | Item, source: recurring participation | `title` text; `quota` integer | Until terminal by `abandoned` |
| `External` | Item, source: something outside the user's control | `title` text | Until terminal by `Done` or `abandoned` |
| `Stance` | Bookkeeping: one decision about an item | `kind` text (section 3.3); `at` timestamp; `cost` optional integer; `horizon` optional integer; `reason` optional text | Superseded to change the decision |
| `Done` | Bookkeeping event: the item is complete; for a project, an explicit acceptance | `at` timestamp; `note` optional text | Never |
| `Did` | Bookkeeping event: one act of participation or attention | `at` timestamp; `note` optional text | Never |
| `Review` | Bookkeeping: seals a window, the analyzer's only clock | `budget` integer; `at` timestamp; `note` optional text | Never |

Section 4 owns what revision does: a revised item's connections, including
the `About` edges of the bookkeeping nodes about it, stay with the
predecessor until re-pointed to the successor, and a successor of a terminal
revision is an inconsistency. A stance is never edited in place. Changing a
decision supersedes the stance with a successor, which has its own position
and so its own lifetime (section 6).

The canonical declarations (section 11) give every bookkeeping node an `at`.
The analyzer reads it only on reviews, for the deadline estimate and the
stale-review nudge (section 10); otherwise it serves display. On events it
exists because the core rejects a node whose predecessor, type, and payload
equal a current node's ([SPEC §7.1](SPEC.md#71-append-checks) rule 5), so two
`Did` events about one habit need a distinguishing field (decision 23). The
same holds for stances: two root `abandoned` stances about different items
would otherwise be identical. A review's position, not its `at`, is what the
rules measure.

### 3.2 Edges

Every edge this analyzer interprets, with the complete set of permitted
endpoint types. `Enables` and `Helps` connect items and point the way work
flows, from contributor to what it serves. `About` and `Until` point from a
bookkeeping node to an item.

| From | Edge | To | Meaning |
| --- | --- | --- | --- |
| `Task`, `Project`, `External` | `Enables` | `Task`, `Project`, `Goal` | Strict: the target cannot be `Done` or `sufficient` until the source is done. The source serves the target (section 7). |
| `Task`, `Project`, `Habit`, `External` | `Helps` | `Task`, `Project`, `Goal`, `Concern` | Optional: a worthwhile contribution that blocks nothing. The source serves the target (section 7). |
| `Stance` | `About` | Any item; the stance's `kind` narrows the type (section 3.3) | The decision's subject. One or more over time (section 4). |
| `Done` | `About` | `Task`, `Project`, `External` | The completed revision. Exactly one. |
| `Did` | `About` | `Habit`, `Concern` | The attended revision. One or more over time (section 4). |
| `Stance` | `Until` | `Task`, `Project`, `External` | Only from a `deferred` stance, and at most one. Ends the deferral early when the target is terminal. |

Any other endpoint pair for these four labels is an inconsistency, judged at
the edge's own prefix (sections 8.2 and 8.3). The union
of current `Enables` and `Helps` edges must be acyclic.

### 3.3 Stance kinds

A stance's `kind` names the decision. **Applies to** is the set of item types
its `About` edges may target, and **Fields** is the conditional field the
kind requires.

| Kind | Applies to | Fields | Meaning |
| --- | --- | --- | --- |
| `committed` | `Task` | `cost` ≥ 0 | The plan for the window its sealing review opens (section 6). Keeps the task in play (section 7). |
| `deferred` | Any item | `horizon` ≥ 1, at most `max_deferral_horizon` | Not now: the item is dormant (section 7). Lapses after `horizon` seals, or earlier when its `Until` target is terminal. |
| `waiting` | `External` | `horizon` ≥ 1 | Externally gated; check in after `horizon` seals. Keeps the external in play. |
| `sufficient` | `Goal` | | Terminal: judged good enough. Permitted only once every `Enables` source of the goal is done (section 8.3). |
| `abandoned` | Any item | | Terminal. |

`cost` is permitted only on `committed`, and `horizon` only on `deferred` and
`waiting`; either field on any other kind is an inconsistency (section 8.1).
A `deferred` stance's `horizon` may not exceed the configured
`max_deferral_horizon` (section 11). Someday/maybe is therefore a deferral
that is acknowledged and renewed at least once per cap, and `abandoned` is the
only stance that sets an item aside indefinitely (decisions 43 and 44).

There is no `pursued` kind. Every open item is in play unless a stance or the
dormancy rule of section 7 says otherwise. Why not a stance that puts an item
in play? Then an item recorded and never pursued would be deferred with no
horizon, and the database would become a bucket list (decision 40).

### 3.4 Amending the declarations

Bootstrap suggests each canonical declaration of section 11 that is missing.
After that, the declarations are the user's to amend like any other
([SPEC §6](SPEC.md#6-type-declarations)). A user may add fields and labels of
their own to a planning type, and later remove, deprecate, or encourage them,
and may change or remove any canonical field the analyzer does not read. The
analyzer ignores fields and labels it does not read. It requires only what its
rules rely on, and judges each declaration of one of its ten names against
this table at the declaration's own prefix (section 8.1; decision 36).

| Declaration | Must keep |
| --- | --- |
| Every planning type | Every label its canonical declaration permits, in the same direction. |
| `Stance`, `Done`, `Did`, `Review` | An empty `successors`: a bookkeeping node is never replaced by a node of another type. |
| `Stance` | `kind` mandatory text; `cost` and `horizon` declared as integer, under any policy but mandatory. |
| `Review` | `budget` mandatory integer; `at` mandatory timestamp. |
| `Concern`, `Habit` | `quota` mandatory integer. |
| `Goal`, `Project` | `deadline` timestamp under any policy, or no `deadline` at all, which turns off deadline reasons (section 10). |

`cost` and `horizon` cannot be mandatory because section 3.3 forbids them on
most kinds. Same-type successors are always permitted
([SPEC §6](SPEC.md#6-type-declarations)); section 8.1 rejects them for reviews
and events. The canonical fields left out of the table are free: `title`,
`criterion`, `deliverable`, `note`, `reason`, and `at` on stances and events.
Removing `at` from a stance or event declaration is permitted, but two
otherwise identical stances or events then collide under
[SPEC §7.1](SPEC.md#71-append-checks) rule 5. Whether an item's `successors`
should be constrained is open (section 14, question 6).

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
lifetime (section 6) is measured from it (decision 5). A `Done` is never
re-pointed, because its target is final (decision 26).

Changing a decision supersedes the stance: the successor carries the new
kind, its own `About` edge to the subject, and a lifetime measured from its
own position. The **current stance** of a revision is the current stance
whose current `About` edges include that revision; if the user appended
several root stances instead of superseding, the latest by position wins.
Nothing is validated across stances, and no lifetime is read from a stance's
ancestors (decisions 22 and 6). A revision's **state** is `abandoned` if its current stance is
`abandoned`; else `done` if
a `Done` currently targets it, or `sufficient` if its current stance is
`sufficient`; else `open`. Every state but `open` is **terminal**; only
`done` satisfies an `Enables` edge.

A terminal revision is final: a successor of a terminal revision is an
inconsistency at the successor's own prefix. New work is a new item. A split
is ordinary: each current revision carries its own connections, decisions,
and state.

The one history query the analyzer makes is `root_of`, to measure how long
an item has existed and whether a habit or concern is new at a seal
(section 7). All revisions of a tree share that age.

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
  the way TypeCheck judges a type transition only at the successor's prefix
  (decision 2).
- Stances appended in window k and sealed by `Review k+1` are the plan for
  window k+1. `Review k+1`'s `budget` bounds its sealed commitments.
- At a non-seal boundary, the **open window** is the run since the latest
  review. The valuation (section 9) reports on it.

The ritual is: look at the ranking, append decisions, events, and
reconnections, then append the review. If the seal is rejected, adjust and
retry. The transaction that fails is the one that claimed the window was
reviewed.

## 6. Lifetimes

Lifetimes count seals from a stance's position; no lineage is involved
(decision 6). The **sealing review** of a stance is the first review after it.

- `committed` stands through the window its sealing review opens and lapses at
  the next seal.
- `deferred` with `horizon = n` stands through the n-th seal after its sealing
  review and lapses after it; it lapses earlier at any boundary where its
  `Until` target is terminal.
- `waiting` stands through `horizon` seals like `deferred`.
- `sufficient` and `abandoned` are terminal.

Relative to a seal, a stance appended in the window being sealed is
**fresh**; one sealed earlier and still within its lifetime is **standing**.
Relative to a non-seal boundary, fresh means appended in the open window.
Fresh or standing stances are **decisions**; lapsed ones are history.

A lapsed stance stays the item's current stance until superseded, but it no
longer decides anything. When a deferral lapses, the item returns to play by
itself, with no new entry, unless the rule of section 7 keeps it dormant
because everything it serves is dormant or terminal. A returning item in the
frontier needs a new decision at the next seal.

A deferral runs until it lapses, or until its `Until` target is terminal.
Superseding it with another `deferred` stance starts a new lifetime rather
than ending the deferral. A task can be resumed early by superseding its
deferral with `committed`, and an external with `waiting`, but no stance kind
ends the deferral of a goal, project, habit, or concern early, short of
`abandoned` or `sufficient` (section 14, question 20).

## 7. In play, ready, and the frontier

Every term below is judged at one boundary: a seal, or the non-seal boundary
the valuation is asked for. "Decision", "fresh", and "standing" are relative
to that boundary as section 6 defines them.

1. An item **serves** the targets of its current outgoing `Enables` and
   `Helps` edges. A `Helps` edge serves exactly as an `Enables` edge does, so
   optional work is owed a decision like any other (decision 42).
2. An open revision is **dormant** when either holds:
   - its current stance is a `deferred` decision;
   - it serves at least one item, and every item it serves is dormant or
     terminal.

   One exception comes first: an open revision whose current stance is a
   `committed` or `waiting` decision is never dormant. Every open revision
   that is not dormant is **in play** (decision 41). So an open revision that
   serves nothing, such as every goal and concern, is in play unless its own
   stance defers it. Section 3.2 keeps the union of current `Enables` and
   `Helps` edges acyclic, so the rule settles every revision, starting from
   those that serve nothing.
3. So one deferral of a goal makes dormant every task, project, external, and
   habit that serves only that goal, directly or through other dormant items,
   and when the deferral lapses they all return to play with no new entry.
   Abandoning or finishing what an item serves makes it dormant until it
   comes to serve something in play, or nothing at all; terminal items never
   return, so no seal asks about it in the meantime (section 14, question
   21). A committed task stays in play under a deferred goal, and so do the
   prerequisites that serve it.
4. **Ready** means an open `Task` or `Project` whose `Enables` sources are all
   done, whether it is in play or dormant.
5. The **strict subgraph** of a goal or project is the item itself and every
   open revision reached from it by walking `Enables` backward, target to
   source, through open revisions, in play or dormant.
6. A habit or concern is **new** at a seal when the root of its revision tree
   (`root_of`, section 4) was appended in the window being sealed; at a
   non-seal boundary, when it was appended in the open window. Revising the
   item does not make it new again (decision 46).
7. The **frontier**, the revisions that need a decision, is:
   - every ready `Task` in play;
   - every ready `Project` in play;
   - every open `External` in play;
   - every `Habit` or `Concern` in play that is not new.
8. A frontier revision is **decided** when its current stance is a
   `committed` decision, for a task, or a `waiting` decision, for an external,
   or, for a habit or concern, when a `Did` in the window currently targets it
   (decision 19). Otherwise the revision is **undecided**.

Every other answer takes the revision out of the frontier instead of deciding
it there: a `deferred` decision makes it dormant; `abandoned`, `sufficient`,
or a `Done` makes it terminal; a new `Enables` source that is not done makes a
task or project not ready. Open revisions that are not ready are blocked by
the graph and need no decision.

Goals are never in the frontier. Section 8.4 judges each goal in play by
whether its plan can advance (decision 45). A new habit or concern is in play
like any other item, and a new habit still rescues a plan it helps
(section 8.4), but neither is judged for itself until the seal after the one
that finds it new (section 8.5).

## 8. Findings

Every rule names its judging prefix: the node's own prefix, the prefix of a
particular edge, or a seal. Judging once at a fixed prefix keeps replay
stable. Certificates name exact subject entries and never a boundary; seal
certificates also name the sealing review, so a condition that persists into
the next window is a distinct finding because the world holds a new review.

**Activation.** The analyzer acts only at entries of its vocabulary and at
declarations of its types. An **entry of its vocabulary** is a node of one of
its ten types, a node that supersedes one, or an edge with one of its four
labels; a `Review` entry is also a seal. A **declaration of its types** is one
that claims one of the ten names. Every inconsistency and sticky warning is judged at one of
these prefixes. At any other prefix the analyzer judges nothing and only
re-emits its nonsticky messages, the bootstrap suggestion and the opt-in
stale-review nudge, while their condition holds (decision 37).

**Left to TypeCheck.** The analyzer assumes TypeCheck is configured in the
same suite, as `init` does by default
([INTERFACES §2](INTERFACES.md#2-cli-surface)), and does not repeat its checks
(decision 39). TypeCheck rejects a node whose payload does not fit its current
declaration; with section 3.4, that means every stance carries a text `kind`,
every review an integer `budget` and a timestamp `at`, every habit and concern
an integer `quota`, and `cost`, `horizon`, and `deadline` have their declared
types when present. It also rejects an edge whose label either endpoint's
declaration does not permit; a type transition no declaration permits, which
with section 3.4 keeps a bookkeeping node from being replaced by a node of
another type; and a declaration that does not parse or shares its name
([SPEC §6.2](SPEC.md#62-name-resolution)). The analyzer checks what a
declaration cannot say: the type at the opposite endpoint of each of its
labels, the kind vocabulary, the conditional fields, the signs of integers,
and the no-revision rules, since same-type successors are always permitted
([SPEC §6](SPEC.md#6-type-declarations); decision 33). It also judges each
declaration of its types against section 3.4, because TypeCheck checks nodes
against whatever the current declaration says (decision 36).

An implementation may assume that every payload fits its declaration. Every
configured analyzer replays every prefix, including a candidate prefix
TypeCheck rejects ([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)),
so the planning analyzer can still meet a payload that does not fit. If it
returns an error there, the plan is rejected either way; only the report
differs, because an analyzer error aborts the run and its text replaces
TypeCheck's finding (decision 53). The same holds at a seal later in a plan
that section 8.2 already rejects for a cycle, where the rule of section 7 may
not settle every revision. Its findings in a suite without TypeCheck are not
specified.

### 8.1 At a node's own prefix

Inconsistencies: unknown `kind`; `committed` without `cost`; `deferred` or
`waiting` without `horizon`; `cost` or `horizon` on a kind that forbids it;
negative `cost`, `horizon` below 1, negative `budget` or `quota`; a
`deferred` stance whose `horizon` exceeds `max_deferral_horizon`, unless the
key is `null` (section 11; decisions 43 and 44); a `Review`, `Done`, or `Did`
with a predecessor; a successor of a terminal revision.

At the prefix of a declaration of its types that parses: one inconsistency
per requirement of section 3.4 it does not meet, naming the declaration and
the requirement. TypeCheck reports a declaration that does not parse.

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
not done. These are judged immediately because events cannot be taken back
(decision 10).

### 8.4 At a seal: inconsistencies

- **Undecided item.** A frontier revision that is undecided. One finding per
  revision; focus the revision.
- **Budget overrun.** The sum of `cost` over open tasks whose current stance
  is a fresh `committed` exceeds the review's `budget`. A `committed` decision
  keeps its task in play (section 7), so no commitment escapes the budget
  (decision 48).
- **Broken plan.** A `Goal` or `Project` in play whose strict subgraph holds
  no ready task, no ready project, and no open `External` with a fresh or
  standing `waiting` stance, and into which (the item or its strict subgraph)
  no `Habit` in play currently `Helps`. No other helping work rescues a plan
  (decisions 45 and 49).

A dormant goal or project is not judged, and a done project is not open, so
neither is ever broken. A goal whose strict subgraph is the goal alone,
because it has no `Enables` source or all of them are done, is broken unless
a habit in play helps it. For a new goal, that means it needs a first ready
task, a helping habit, or a deferral before the next seal. For a goal whose
prerequisites are all done, it means a Broken plan at every seal until a
`sufficient` stance ends the goal; whether that case should be a Broken plan
at all is open (section 14, question 14).

### 8.5 At a seal: sticky warnings

- **Shortfall.** A `Habit` or `Concern` in play that is not new (section 7),
  with fewer `Did` events in the window than `quota`. Subjects are the item,
  the review, and the window's `Did` events. A deferred habit or concern is
  dormant, so it is never short.
- **Grace period.** A `Habit` or `Concern` in play that is new at this seal:
  one warning saying that the item is new and is first judged at the next
  seal. It stands in for the Undecided item and Shortfall findings the item
  is spared. Subjects are the item and the review (decisions 46 and 47).
- **Dropped connection.** One per connection, whether an `Enables`, `Helps`,
  `About`, or `Until` edge, that touched a revision superseded in the window
  and has no equivalent current connection involving a current successor.
  A warning, not a gate, because dropping a connection is sometimes what the
  edit meant (decision 17).

Sticky because each judges a closed window once; the next window gets its
own finding (decision 18).

### 8.6 Suggestions

- **Bootstrap.** Nonsticky, re-emitted while the condition holds: one per
  canonical declaration of section 11 whose name resolves as missing
  ([SPEC §6.2](SPEC.md#62-name-resolution)), adding it. An amended
  declaration is never suggested back to its canonical form; section 8.1
  judges it instead (decision 36).

Re-pointing connections after a revision is not this analyzer's job. The
general [RECONNECT](RECONNECT.md) analyzer suggests recreating a superseded
revision's edges on its successors, which covers `Enables`, `Helps`, `About`,
and `Until` alike; the planning analyzer only names what is still missing at
the seal (section 8.5; decision 4). The analyzer never suggests stances or
events.

## 9. Valuation

Between reviews the analyzer ranks revisions at the requested boundary, with
the terms of section 7 judged at that boundary. The ranking covers the
revisions in play and, in tier 3 only, the revisions whose own current stance
is a standing `deferred`. A revision that is dormant only because what it
serves is dormant is not ranked (decision 51). Each revision appears once, in
the first tier whose members include it. Rows are ordered by tier, then the
tier's keys, then root position ascending.

| Tier | Members | Keys |
| --- | --- | --- |
| 0 | What the next seal would reject as it stands: undecided frontier revisions, and goals and projects in play whose plan is broken (section 8.4) | goals and concerns first; then fan-out, the distinct goals in play reachable forward over `Enables` and `Helps`, descending |
| 1 | Open tasks whose current stance is a standing `committed` | `OpenFor` descending; then `DeadlineIn` ascending when present |
| 2 | Habits and concerns in play, not new, below `quota` so far in the open window | shortfall descending |
| 3 | Revisions whose current stance is a standing `deferred` or `waiting` | lapsing at the next seal first; then `OpenFor` descending |
| 4 | Goals and projects in play, with a sealed root, whose strict subgraph holds no task with a standing `committed` | seals since it last did, or since the sealing review of its root, descending |
| 5 | The rest of the revisions in play | root position |

Tier 0's broken plans are decision 50, and tier 4's count from the root is
decision 52. There is no tier of available contributions: work that only
helps is in play like any other (decision 42).

Reasons are structured values with rendered text: `Undecided`, `BrokenPlan`,
`OpenFor { seals }` (seals since the revision tree's root was appended),
`Standing { kind, seals_left }`, `LapsesAtNextSeal`, `DeadlineIn { reviews }`,
`DeadlinePassed`, `Starved { seals }`, `Shortfall { have, quota }`. Tests
assert the structured value; clients show the text.

`seals_left` is the number of seals after the boundary at which the stance
is still a decision, counting the next seal. A standing `deferred` or
`waiting` stance sealed by review *j* with horizon *n*, at a boundary in
window *k*, has `seals_left` = *j* + *n* − *k*, which is at least 1, because
after its last seal it has lapsed and is no longer standing.
`LapsesAtNextSeal` is given exactly when `seals_left` is 1: the next seal is
the last one the stance decides, and the item returns to play in the window
that seal opens (section 6). The two reasons overlap: `LapsesAtNextSeal`
repeats `seals_left: 1`, and it is what tier 3's first key sorts on.

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
than the clock (decisions 1 and 32).

## 11. Configuration and bootstrap

Type and edge names are canonical, and planning owns them (decision 35).
Configuration is an empty string or a strict JSON object; the empty string
means:

```json
{
  "horizon_windows": 6,
  "max_deferral_horizon": 12,
  "stale_review_after_days": null,
  "first_sealed_review": null
}
```

Unknown fields, fractions, and negatives reject construction.
`max_deferral_horizon` is a positive integer or `null`. It caps the `horizon`
of a `deferred` stance, judged at the stance's own prefix (section 8.1), and
`null` removes the cap; `waiting` is not capped (decisions 43 and 44). The
cap is part of the configuration string, and so of every planning finding's
identity (decision 15), and an analyzer in the suite is never reconfigured
([INTERFACES §2](INTERFACES.md#2-cli-surface)), so changing the cap means
adding a new planning instance. Section 8.1 judges all history, so adding a
planning instance to a database whose history holds a deferral longer than
its cap is rejected (open question 7).

`first_sealed_review` is an entry position; seals before it are not judged
under sections 8.4 and 8.5, so that adopting the analyzer on a database that
already holds reviews, or upgrading to stricter seal rules, cannot lock the
database behind a historical seal inconsistency (decision 25). It does not
exempt sections 8.1 to 8.3, which judge every prefix of history, so adding the
analyzer to a database whose history uses its names differently is rejected
([INTERFACES §2](INTERFACES.md#2-cli-surface); open question 7). The
configuration string is part of analyzer identity, so defaults should rarely
change (decision 15).

There are ten canonical declarations, one per node type of section 3.1:
`Goal`, `Concern`, `Project`, `Task`, `Habit`, `External`, `Stance`, `Done`,
`Did`, `Review`. Bootstrap suggests each one that is missing (section 8.6),
and section 3.4 says what a user may amend. Three of the ten:

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

Section 8 says which checks the analyzer leaves to TypeCheck and which it
makes itself, including its judgment of each declaration against section 3.4
(decisions 33, 36, and 39).

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
   ([STORAGE](STORAGE.md)), takes the configuration string of a planning
   analyzer in the current analysis suite so that the ranking uses that
   analyzer's settings, and prints the ranking at the head or at an "as of"
   boundary as lines or JSON. A suite may hold no planning analyzer or
   several, each added with its own configuration
   ([INTERFACES §2](INTERFACES.md#2-cli-surface)). With exactly one, the
   executable uses it; with several, the user selects one by its suite
   position. With none, or several and no selection, it errors and lists the
   suite (decision 38).
3. **Application later.** When the browser client wants the ranking, the
   application service adds one RPC method that calls the same function, and
   the client renders tiers and reason text with links into the inspector.
   Fixtures and the client contract then carry the `Valuation` shape.

Nothing else is needed: `root_of`, the all-edges selection for the
dropped-connection rule, newest-entry inspection, timestamp fields, and
suggestion actions exist.

## 13. Design decisions

One entry per decision: what was decided, the alternative it rejected, and
the reason. Entries whose reason is marked **not recorded** are decisions the
text makes without saying why; the owner fills them in or reopens them.
[PLANNING-EXPLAINER](PLANNING-EXPLAINER.md) links each of its "why not"
callouts here.

1. **The review is the only clock.** Rejected: wall-clock windows. Reason:
   evaluation time is one frozen value per run, so judging windows by the
   clock would replay every historical window under today's time and
   accumulate findings for weeks nobody looked at (sections 2 and 10).
2. **The protocol is judged at seals only; ordinary edits are never
   blocked.** Rejected: judging the protocol rules at every prefix, or
   blocking edits that leave the plan inconsistent. Reason: it follows the
   pattern TypeCheck uses for type transitions, judge once at a fixed prefix
   against the view at that prefix; later entries cannot change the verdict,
   replay is stable, and the user is never locked out of repairing the graph
   by the rules that say it needs repair (section 5).
3. **Identity is the exact revision, everywhere; the analyzer never resolves
   a connection through a revision tree.** Rejected: decision chains with
   root resolution, an earlier draft that resolved `About` edges to the
   revision root and made stances a revision chain per item, so deferral
   counts and commitment age came from lineage. Reason: it needed rules
   against split items, second root stances, and chains whose `About` roots
   diverged, and it ran two identity models at once, because edges still used
   exact revisions. Exact revisions remove all of that; the lost deferral
   count is replaced by `OpenFor` (sections 2 and 4).
4. **Re-pointing connections after a revision belongs to the general
   Reconnect analyzer.** Rejected: a Re-point suggestion inside this
   analyzer. Reason: the `About` edge from a stance or `Did` to a revised
   item is an ordinary eligible edge of the predecessor, so Reconnect covers
   every case the planning rule covered, and planning keeps only what is
   specific to windows, the dropped-connection warning at the seal
   (section 8.6, [RECONNECT §8](RECONNECT.md#8-planning-analyzer)).
5. **`About` is not one-to-one: a stance or `Did` is re-pointed by adding an
   `About` edge, keeping its position.** Rejected: a fresh stance about the
   successor. Reason: a stance's lifetime is measured from its position, so a
   fresh stance would have a fresh lifetime; re-pointing keeps the decision
   and its clock (section 4).
6. **Lifetimes count seals from a stance's position and read nothing from
   its ancestors.** Rejected: counting deferrals or commitment age through
   the stance's revision chain. Reason: that is the lineage model decision 3
   rejected; each lifetime comes from its own position and the list of
   reviews, and what presses is `OpenFor`, which needs no chain (section 6).
7. **Awareness is the dependency graph; commitment is a stance.** Rejected:
   a dependency edge that means both "this matters" and "I owe action on
   this now". Reason: honest breadth would then produce chronic
   overcommitment, and deleting edges would become the cheapest way to feel
   feasible (sections 1 and 2). **Revised by decision 40:** the commitment
   half stands, since no edge obliges action and only a `committed` stance
   does. The awareness half does not: a recorded item is in play and owed a
   decision until a stance or the dormancy rule sets it aside.
8. **`Helps` never propagates being in play, and there is no `declined`
   stance.** Rejected: a `declined` stance for optional work. Reason: it was
   needed only while optional work was in the frontier; with `Helps` carrying
   no obligation, nothing has to be declined, and unselected contributions
   stay visible in the valuation (section 7). **Superseded by decision 42.**
9. **Status changes are events, not revisions; a finished revision stays
   current.** Rejected: completion as supersession. Reason: it makes every
   dependency edge historical the moment a prerequisite is done, so a
   bypassed prerequisite cannot be told from a satisfied one and every status
   change asks the user to re-assert relationships that did not change
   (section 2).
10. **`Done` and `sufficient` are judged against their `Enables` sources at
    the `About` edge's prefix.** Rejected: judging them at the seal. Reason:
    events cannot be taken back, so they are the one thing judged the moment
    they appear (section 8.3).
11. **A terminal revision is final; a successor of one is an inconsistency,
    and new work is a new item.** Rejected: revising a done or abandoned
    revision. Reason: **not recorded**.
12. **Pressure is a valuation, not a finding.** Rejected: pressure as
    findings with escalating or counter-varied certificates. Reason: a
    finding is a fact with a fixed identity, re-emission merges and dismissal
    hides it for good; faking renewal by varying certificates grows
    observations without bound, and faking magnitude with escalating variants
    makes dismissal meaningless (sections 1 and 9).
13. **The valuation is a library function and an executable, not an analyzer
    output.** Rejected: a valuation hook on the analyzer traits. Reason: it
    would have put a presentation output into the runner contract and the
    shared message types; a function over a bounded `View` gives the same
    ranking with no change to [ANALYZERS](ANALYZERS.md) (section 12).
14. **Stances are nodes.** Rejected: stances as edges from the review.
    Reason: edges carry no payload and could not hold cost, horizon, or
    reason (section 3.1).
15. **Type and edge names are canonical.** Rejected: role mapping by
    configuration. Reason: the configuration string is part of finding
    identity, so remapping invalidates every dismissal (section 11).
16. **Connections to revisions superseded within a window are not squashed
    onto the final in-window successor at the seal.** Rejected: treating a
    connection to any revision created in the window as a connection to its
    final successor. Reason: two consecutive seals would read one edge
    differently (sections 8.5 and 8.6).
17. **Dropped connection is a warning, not a gate.** Rejected: an
    inconsistency that fails the seal. Reason: dropping a connection is
    sometimes what the edit meant (section 8.5; open question 4 revisits
    this once re-pointing is one click).
18. **Seal warnings are sticky.** Rejected: nonsticky warnings re-emitted
    while the condition holds. Reason: each judges a closed window once, and
    the next window gets its own finding because its certificate names the
    sealing review (section 8.5).
19. **A habit or concern in play is decided by a `Did` in the window.**
    Rejected: a stance per window on top of the participation. Reason:
    demanding one would be ceremony; a single `Did` is the decision even
    below quota, and the shortfall warning carries the rest. A habit or
    concern that should rest is deferred instead, which makes it dormant
    (decision 41), so the seal neither asks for a `Did` nor counts a
    shortfall until the deferral lapses (sections 7 and 8.5).
20. **`pursued` never counts as a window's decision.** Rejected: letting a
    standing `pursued` stance decide a frontier revision. Reason: **not
    recorded** beyond the statement that `pursued` puts an item in play
    (section 7). **Superseded by decision 40.**
21. **Budget overrun sums `cost` only over tasks in the active set whose
    current stance is a fresh `committed`.** Rejected: summing every fresh
    `committed`, or every standing one. Reason: **not recorded**
    (section 8.4). **Superseded by decision 48.**
22. **The current stance of a revision is the latest root stance by
    position; nothing is validated across stances.** Rejected: rejecting a
    second root stance about the same revision. Reason: the earlier draft
    needed rules against second root stances; letting the latest win removes
    the rule (section 4, decision 3).
23. **Events carry a mandatory `at` timestamp the analyzer gives no meaning
    to.** Rejected: payload-free events. Reason: the core rejects a node
    whose predecessor, type, and payload equal a current node's
    ([SPEC §7.1](SPEC.md#71-append-checks) rule 5), so two `Did` events
    about one habit need a distinguishing field (section 3.1).
24. **Reviews, `Done`, and `Did` are never revised.** Rejected: revising an
    event or a review. Reason for events: they cannot be taken back
    (section 8.3). Reason for reviews: **not recorded** (section 3.1).
25. **`first_sealed_review` suppresses seal findings before a configured
    position.** Rejected: judging every historical seal under the current
    rules. Reason: adopting the analyzer on a database that already holds
    reviews, or upgrading to stricter seal rules, must not lock the database
    behind a historical seal inconsistency. It covers seals only; sections
    8.1 to 8.3 still judge all history (section 11, open question 7).
26. **A `Done` has exactly one `About` edge and is never re-pointed.**
    Rejected: re-pointing a `Done` like a stance or `Did`. Reason: its
    target is final, so there is never a successor to point at (section 4).
27. **A project is completed by an explicit `Done`, not by its
    prerequisites.** Rejected: a project becoming done when its `Enables`
    sources are. Reason: satisfying every dependency makes delivery possible
    without proving the deliverable was accepted, so acceptance stays a
    decision (section 7).
28. **`sufficient` requires every `Enables` source done.** Rejected: a free
    judgment of sufficiency. Reason: sufficiency is a judgment, but not one
    that may contradict the plan (section 3.3).
29. **Goals end by `sufficient` or `abandoned`, never by `Done`.** Rejected:
    `Done` about a goal. Reason: **not recorded** (section 3.1).
30. **A broken plan is rescued by a ready task or project, a waited-on
    external, or a pursued habit that helps, and by nothing reached only
    through `Helps`.** Rejected: letting any `Helps` source rescue the plan.
    Reason for excluding optional contributions: they carry no obligation
    (decision 7). Reason for admitting a pursued habit: **not recorded**
    (section 8.4). **Superseded by decision 49.**
31. **Each stance kind applies to a fixed set of item types.** Rejected:
    any kind about any item. Reason: **not recorded** (section 3.3).
32. **The stale-review nudge is the only use of evaluation time, and it is
    opt-in.** Rejected: implicit clock reads. Reason: a user who stops
    reviewing should get no pressure except that one nudge, a deliberate
    dependence on the ritual rather than the clock; unset, evaluation inputs
    are never read (section 10).
33. **Conditional fields and kind vocabularies are analyzer inconsistencies,
    not schema.** Rejected: encoding them in the type declarations. Reason:
    a declaration cannot say the type at the opposite endpoint, the kind
    vocabulary, the conditional fields, or the no-revision rules, since
    same-type successors are always permitted
    ([SPEC §6](SPEC.md#6-type-declarations)) (section 8).
34. **The union of current `Enables` and `Helps` edges must be acyclic.**
    Rejected: permitting cycles in the dependency graph. Reason: **not
    recorded** (section 3.2).
35. **Planning owns its ten type names and four edge labels in every
    database that configures it.** Rejected: sharing them with other
    vocabularies, by judging `About` and the other labels only when the
    source is a planning type. Reason: two vocabularies can share a name only
    through namespacing, which belongs in the type system rather than in one
    analyzer; it is a likely improvement but not needed now. Until then a
    `Note` with an `About` edge to a `Task` is an inconsistency, and every
    `Review` is a seal, whatever the user meant by it (sections 3, 5, 8.2,
    and 8.3).
36. **A user may amend planning's declarations, and the analyzer requires
    only what its rules read.** Rejected: requiring the canonical
    declarations exactly, which bootstrap did by suggesting a revision of
    every declaration that was not payload-equal; and trusting amended
    declarations unchecked. Reason: a user should be able to work with
    fields and labels of their own on planning's types. A revert suggestion
    re-emitted forever made every amendment noise, and its action could not
    apply while a current edge used an added label
    ([SPEC §7.3](SPEC.md#73-resulting-view-checks) rule 4). An unchecked
    amendment that dropped `budget` or `About` would silently disable the
    rules that read them, so each declaration of a planning type is judged
    once, at its own prefix, as TypeCheck judges a type transition
    (sections 3.4, 8.1, and 8.6).
37. **The analyzer acts only at entries of its vocabulary and at
    declarations of its types.** Rejected: validating only stances at their
    own prefix, with dependency edges and events judged at the seal or not
    at all. Reason: these are the only prefixes at which a planning verdict
    can change. Judging nothing elsewhere keeps the user's own types,
    fields, and labels invisible to the analyzer, and judging an event at
    once still follows decision 10 (section 8).
38. **The planning executable does not assume one planning analyzer per
    suite.** Rejected: reading "the" planning configuration from the suite.
    Reason: a reasonable assumption, but nothing enforces it; a suite may
    hold none or several, each with its own configuration (section 12).
39. **The analyzer relies on TypeCheck in the same suite and does not repeat
    its checks.** Rejected: re-validating field presence, value types, label
    permissions, and type transitions so that the analyzer stands alone.
    Reason: TypeCheck already rejects all of them, and `init` configures it
    by default; a second check would only report one defect twice. An
    implementation may rely on it as well and assume that payloads fit their
    declarations; at a prefix TypeCheck rejects, an error from the planning
    analyzer still rejects the plan and only replaces TypeCheck's report
    (section 8; decision 53).
40. **There is no `pursued` stance: every open item is in play unless a
    stance says otherwise.** An item is set aside by `deferred` (not now, for
    a while), `abandoned` (never), or `sufficient` or `Done` (finished).
    Rejected: a `pursued` stance that put a goal, project, habit, or concern
    in play, together with a goal's or project's strict subgraph, and left
    every other recorded item out of play. Reason (owner, 2026-10-25): an
    item that was recorded but not pursued was in effect deferred with no
    horizon, which turns the database into a bucket list. Removing it also
    lets a lapsed deferral return an item to play by itself, and closes the
    gap through which a commitment to a task outside the active set escaped
    the budget (decision 48). It supersedes decision 20 and revises decision
    7; goals no longer owe a pursuit decision, and decision 45 judges them
    instead (sections 2, 3.3, 6, and 7).
41. **An open item is dormant when its own current stance is a `deferred`
    decision, or when it serves something and everything it serves is
    dormant or terminal. A `committed` or `waiting` decision keeps an item in
    play.** Rejected: quieting only the item a deferral is about.
    Reason (owner, approved): without the rule, deferring a goal would quiet
    only the goal, and each of its tasks would need its own decision. The
    rule is the mirror image of the old active set, which spread being in
    play from pursued items to what serves them; dormancy now spreads from
    deferred and finished items instead. The graph is acyclic, so the rule
    is well-defined. The exception for `committed` and `waiting` decisions
    came with the approved rule and its reason is **not recorded**; it is
    what makes every commitment count toward the budget (decision 48)
    (section 7).
42. **Optional work is owed a decision: an item serves the targets of its
    `Helps` edges exactly as it serves those of its `Enables` edges.**
    Rejected: a rule that an item reached only through `Helps` is dormant.
    Reason (owner, approved): a `Habit` can serve only through `Helps`
    (section 3.2), so that rule would leave every habit dormant until
    something like `pursued` put it back in play. Without "recorded but not
    pursued is free", the database avoids becoming a guilt generator through
    the dormancy rule and deferral instead: one decision sets a whole branch
    aside (decision 41). There is still no `declined` stance, since
    `deferred` or `abandoned` says it, and the valuation loses its tier of
    available contributions and the `Unselected` reason. Supersedes decision
    8 (sections 1, 7, and 9).
43. **Someday/maybe is a deferral, and a configuration key caps a
    deferral's horizon.** Rejected: deferrals of any length, which would let
    a long horizon do what not pursuing an item used to do. Reason (owner):
    a parked item must be acknowledged and deferred again at least once per
    cap, and `abandoned` stays the only indefinite quiet state. Two
    consequences follow. The configuration string is part of finding
    identity (decision 15) and an analyzer is never reconfigured
    ([INTERFACES §2](INTERFACES.md#2-cli-surface)), so changing the cap means
    adding a new planning instance, the same concern open question 13 raises
    for `horizon_windows`. And section 8.1 judges all history, so adopting
    the analyzer on a database whose history holds longer deferrals is
    rejected (open question 7) (sections 3.3, 8.1, and 11).
44. **The cap is `max_deferral_horizon`, 12 by default and `null` for none;
    it caps `deferred` stances only, and is an inconsistency at the stance's
    own prefix.** Rejected: no cap by default; capping `waiting` as well;
    judging the cap at the seal. Reason: **Provisional, owner to confirm:** a
    default makes the cap do its job without configuration, and 12 seals is
    about a quarter of weekly reviews; a `waiting` external stays in play and
    in the ranking, so it is not a quiet state; and a horizon over the cap is
    a field check like the others of section 8.1, so it is judged where the
    stance appears. The owner wanted the name chosen together with the rename
    of open question 13 (sections 8.1 and 11; open question 15).
45. **Broken plan judges every goal and project in play, and goals are never
    in the frontier.** Rejected: an open goal in the frontier, undecided
    until a `pursued` stance decided it, with Broken plan only for pursued
    goals and projects. Reason (owner, approved): with no `pursued`, a goal
    has no stance it owes; a new goal now needs a ready task or a deferral
    before the next seal, which is about as strict as the old undecided
    sink. A done project is not open, so a finished project is never broken
    (sections 7 and 8.4).
46. **A habit or concern is spared at the seal that finds it new, and that
    seal emits a warning instead.** A habit or concern whose root was
    appended in the window being sealed is not in the frontier and not
    judged for Shortfall at that seal; `root_of` measures it, so a revision
    does not restart the grace. Rejected: judging a new habit at its first
    seal; sparing it silently. Reason (owner): without the grace, a habit
    recorded just before a review is undecided or short at that seal, for a
    window it barely existed in, as a repro showed. The warning says why the
    item was not judged; it is a warning because there is no `info` message
    kind yet ([PLAN §3](PLAN.md#3-backlog)) (sections 7 and 8.5).
47. **The grace warning is the sticky Grace period warning, with the item
    and the review as subjects.** Rejected: a nonsticky warning re-emitted
    while the item is new. Reason: **Provisional, owner to confirm:** it
    judges a closed window once, like the other seal warnings (decision 18),
    and the name and subjects are the ones proposed in review (section 8.5;
    open question 16).
48. **Budget overrun sums `cost` over every open task whose current stance
    is a fresh `committed`.** Rejected: summing only over tasks in the
    active set (decision 21), which let a commitment to a task outside it
    escape the budget. Reason: a `committed` decision keeps its task in play
    (decision 41), so no fresh commitment is left out; the escape was among
    what removing `pursued` dissolves (decision 40). Reason for not summing
    standing commitments: **not recorded**, as in decision 21. Supersedes
    decision 21 (section 8.4).
49. **A broken plan is rescued by a ready task or project, a waited-on
    external, or a habit in play that helps, and by no other helping
    work.** Rejected: letting a helping task or project in play rescue the
    plan, now that decision 42 makes it owed a decision. Reason:
    **Provisional, owner to confirm:** decision 30's reason, that optional
    contributions carry no obligation, no longer holds. What remains is that
    a `Helps` edge blocks nothing, so finishing helping work never brings a
    goal closer to `sufficient` or a project closer to `Done`. Reason for
    admitting a habit: still **not recorded**. Supersedes decision 30
    (section 8.4; open question 17).
50. **Tier 0 of the valuation also ranks the goals and projects in play
    whose plan is broken, with the reason `BrokenPlan`.** Rejected: leaving
    them to the lower tiers. Reason: **Provisional, owner to confirm:** tier
    0 holds what the next seal would reject as it stands, and a new goal
    with no plan, which used to rank in tier 0 as an undecided sink, stays
    there (section 9; open question 19).
51. **The ranking shows a deferred item in tier 3, but not the items dormant
    through it.** It covers the revisions in play and, in tier 3, the
    revisions whose own current stance is a standing `deferred`. Rejected:
    ranking only what is in play, which would never show a deferral or that
    it lapses at the next seal; ranking every dormant revision. Reason:
    **Provisional, owner to confirm:** it is the smallest change that keeps
    tier 3 and `LapsesAtNextSeal` useful. One row per deferral shows what the
    user decided, and the items it quiets return with it (section 9; open
    question 18).
52. **Tier 4's `Starved` counts from the sealing review of the item's root
    when its strict subgraph has never held a standing commitment.**
    Rejected: counting from the item's latest return to play. Reason:
    **Provisional, owner to confirm:** with no `pursued` stance to count
    from, the root is the anchor the analyzer already reads (`root_of`,
    section 4); counting from a return to play would need the item's dormancy
    at earlier boundaries (section 9; open question 19).
53. **An implementation may assume that payloads fit their declarations,
    and may return an error at a prefix TypeCheck rejects.** Rejected:
    requiring the analyzer never to fail on a payload TypeCheck rejects.
    Reason (owner): the requirement was pedantic. An analyzer error aborts
    the run ([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)), but
    at such a prefix the plan is rejected either way, so only the report
    differs: the error text replaces TypeCheck's finding. If TypeCheck
    rejects a field planning does not read, planning runs normally
    (section 8; decision 39).

## 14. Open questions

1. *Retired by decision 40.* It asked whether pursuing a habit or concern
   should expire, so that the pursuit is reaffirmed. A habit or concern in
   play is now judged at every seal, and setting one aside takes a deferral
   whose horizon is capped (decision 43).
2. Should a ready project in play need a decision at all, given that `Done`
   is the natural next step, or is tier 0 pressure enough? No stance kind
   decides a project where it stands, so a ready project leaves the frontier
   only by `Done`, a deferral, `abandoned`, or a new prerequisite.
3. Should the valuation be visible to other analyzers, or remain
   presentation-only?
4. Should **Dropped connection** become a gate once the client makes
   re-pointing a one-click step?
5. Whether `OpenFor` should count from the root's position or from the
   current revision's, which lets an edit reset age.
6. What may become an item, and what an item may become. The canonical item
   declarations permit no successors of another type, but another type's
   declaration can permit a `Task` successor
   ([SPEC §10.4](SPEC.md#104-historical-transition-permission)) without
   amending planning's, and section 3.4 does not constrain an item's own
   `successors`. When an item's tree has a root of another type, does
   `OpenFor` count from that root, from the first item revision, or should
   such transitions be rejected? The same choice decides when such a habit
   or concern is new (section 7). The same question covers a `Task` revised
   into a `Project`.
7. Should adoption exempt sections 8.1 to 8.3 and the declaration rule before
   a configured position, as `first_sealed_review` does for seals, so that a
   database whose history used planning's names differently, or holds
   deferrals longer than the configured cap (decision 43), can still adopt
   the analyzer?
8. Must a re-pointed `About` edge target a descendant of the stance's or
   `Did`'s earlier targets? Nothing checks where one is re-pointed today, so
   a hand-written file can point a `deferred` stance with horizon left at an
   unrelated task, which it then decides at the next seal with no fresh
   decision, and one stance about two unrelated items contradicts principle
   4. The proposed rule, in section 8.3 at the `About` edge's prefix: when
   the source already has `About` edges, the new target must have one of
   their targets among its ancestors. It would apply to stances and `Did`,
   Reconnect's suggestions always satisfy it, and a second `About` edge in
   one plan to an unrelated item would be rejected. It adds `ancestors` to
   the history queries of section 4, for validation and not resolution, so
   decision 3 still holds. The owner thinks it may be a valid rule to add.
9. Should a second root stance about a revision be rejected (decision 22)?
   A ban is one section 8.3 check, an `About` edge from a stance to a
   revision that already has another current stance, and supersession still
   passes because the old stance's edges go historical. Allowing several
   roots leaves an outranked stance current with no sign of which one
   counts; after a revision, re-pointing only the older one quietly revives
   it; Dropped connection warns about the outranked stances' edges; and
   acceptance test 8 checks the latest stance per task only for this case.
   The one use is changing the decision for one branch of a split: a shared
   `deferred` or `waiting` stance re-pointed to both halves, then a new root
   about one half that leaves the other half's clock alone. Under a ban the
   shared stance must be branched, so the unchanged half's lapse moves at
   least one window later, by at most the cap of decision 43. Options: (a)
   ban, and accept the delayed lapse; (b) ban, and discourage re-pointing a
   clocked stance to more than one branch at split time, in the browser and
   in Reconnect's presentation; (c) allow a root stance only when the
   revision's current stance is shared with another revision. The owner is
   divided: a neat case, possibly too clever for its own good. The
   explainer's section 7 writes such a second root stance where superseding
   the first would do the same.
10. Can `abandoned` and `sufficient` be undone? Superseding such a stance
    makes it historical, so the item has no current stance and is open
    again, and the successor's `About` edge passes section 8.3 because the
    target is no longer terminal. Only `done` is final, which contradicts
    principle 5 and decision 11. With deferral capped (decision 43),
    `abandoned` is the only indefinite quiet state, so users will want to
    undo it. Options: forbid it with a section 8.1 rule against a successor
    of an `abandoned` or `sufficient` stance, so that reviving abandoned work
    means a new item; or make un-abandoning a documented operation,
    superseding the stance, and say what it means for `sufficient`, which
    requires every `Enables` source done.
11. Is a `Done` held to exactly one `About` edge, or at most one (section
    3.2, decision 26)? Section 8.3 rejects a second edge, but nothing judges
    a `Done` with none, and its own prefix cannot, because the edge comes
    after the node; such a `Done` is inert. Options: say "at most one",
    since an orphan `Done` has no subject and no effect, or judge orphans at
    the seal.
12. Does a `committed` stance's `cost` count per task or per stance? Section
    8.4 sums it per task, so a fresh `committed` stance re-pointed to both
    halves of a split before the seal counts twice. Question 8's rule would
    not prevent this, since both halves descend from the original.
13. What should `horizon_windows` be called, and should it be configuration
    at all? Its name collides with the stance field `horizon`, which is
    unrelated; a rename such as `deadline_sample_windows` should be chosen
    together with the cap's name of decision 44, so that the stance field,
    the sample, and the cap are named at once. It affects only the valuation
    (`DeadlineIn`, `DeadlinePassed`, and tier 1's second key), yet as part of
    the configuration string it is part of every planning finding's
    identity, and with no reconfiguration changing it needs a second
    planning instance. It could be a parameter of `valuation()` and the
    executable instead; then check whether section 12's selection of a
    planning instance (decision 38) is still needed. Section 10's wording
    "the last `horizon_windows` consecutive review pairs whose `at` strictly
    increases" also has two readings, the last N pairs without the
    non-increasing ones or the last N increasing pairs; pick one.
14. Should a goal whose `Enables` sources are all done be a Broken plan? It
    is today, at every seal until `sufficient` is appended (section 8.4). The
    escape exists, but "broken" is the wrong word for "finished, declare it
    sufficient". Options: keep Broken plan as the nudge and describe it as
    one, or put such a goal in the frontier, decided by `sufficient`,
    `abandoned`, or a new prerequisite. (A done project is no longer judged
    at all, decision 45.)
15. The deferral cap's details, provisional in decision 44: its name, to be
    chosen with question 13; its default, 12 seals or `null`; whether it
    also caps `waiting`, which has a `horizon` too and keeps a goal from
    being broken while it stands; and whether an inconsistency at the
    stance's own prefix is the right enforcement.
16. The grace warning's details, provisional in decision 47: its name,
    whether it is sticky, and its subjects. An `info` message kind, recorded
    in the backlog of [PLAN §3](PLAN.md#3-backlog), would suit a notice that
    needs no action better than a warning.
17. Should helping work in play rescue a broken plan, now that it is owed a
    decision (decision 49)? And why does a habit that helps rescue one,
    which decision 30 never recorded?
18. Should the valuation rank dormant items, and which (decision 51)? Today
    tier 3 shows only revisions whose own current stance is a standing
    deferral, so the tasks of a deferred goal leave the ranking until the
    goal returns. The alternatives are to rank only what is in play, or
    every dormant revision.
19. Are tier 0's broken plans (decision 50) and tier 4's count from the
    item's root (decision 52) the right ranking for goals and projects now
    that no stance puts them in play?
20. How does a deferral end early? The only non-terminal kind that applies
    to a goal, project, habit, or concern is `deferred`, so superseding a
    deferral means another deferral, with a horizon of at least 1, and the
    item returns to play only when the horizon lapses or an `Until` target
    is terminal (section 6). A task can be resumed with `committed` and an
    external with `waiting`; the other four types cannot. For example, Reza
    defers the habit Run for 4 seals for a knee injury, and the knee heals
    after 2. The only escape today is superseding the deferral with a stance
    that has no `About` edge, which is undocumented and is the same orphan
    trick as question 10. Options: (a) a stance kind that ends a deferral
    and decides nothing, applying to any item and with no lifetime; (b) a
    `Did` about a deferred habit or concern ends its deferral; (c)
    deferrals run their course, bounded by the cap of decision 43.
21. Should an item that serves only terminal items stay dormant for good?
    Under decision 41, an open item whose every served item is terminal is
    dormant, and since a terminal item never returns, nothing asks about it
    again unless it comes to serve something in play, through a new edge, or
    nothing at all, through a revision that leaves its edges behind. Two
    cases: a task that only helped a project now done, and a habit that only
    helped a goal now abandoned. That is a bucket list by another door,
    which decision 40 set out to remove. Options: (a) such orphaned items
    enter the frontier, decided by `abandoned`, by being linked to something
    in play, or by a revision that leaves the edge behind; (b) only dormant
    targets spread dormancy, not terminal ones, so orphans stay in play and
    are asked about through the normal frontier and Shortfall; (c) terminal
    targets spread dormancy only through `Enables`, not `Helps`, so a habit
    keeps its own value. The owner approved decision 41 as written, so the
    rule stands until this is decided.

## 15. Acceptance tests

Structural assertions on typed findings or valuation reasons, with
history-sensitive cases checked before and after the relevant append.

1. Canonical declarations round-trip; bootstrap suggests each missing
   declaration and never an amended one.
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
7. Seal with an undecided ready task fails; a fresh `committed` makes it
   pass, and so does a fresh `deferred`, which makes the task dormant; a
   successor stance replaces its predecessor for budget and decision, and
   among root stances the latest wins.
8. Budget: equal passes, one over fails, every fresh `committed` about an
   open task counts, including one whose goal is deferred, and the latest
   stance per task counts.
9. `committed` lapses at the next seal; `deferred` stands exactly `horizon`
   seals and lapses early when its `Until` target is done or abandoned; once
   it lapses, the item is in play again with no new entry.
10. A ready task whose only edge is a `Helps` edge into a goal in play is in
    the frontier; deferring the goal, or abandoning it, makes the task
    dormant; no tier ranks available contributions.
11. Shortfall counts the window's `Did` events, is sticky, and a single `Did`
    decides a habit in play. Record a habit and seal: one Grace period
    warning, and neither Undecided item nor Shortfall about it; revising the
    habit in that window does not extend the grace. Seal again: judged.
    Defer it and seal: neither in the frontier nor short. Let the deferral
    lapse and seal: judged again. Check before and after each append.
12. Broken plan for a goal in play with nothing ready and nothing waited on,
    and for a new goal with no `Enables` source; none for a deferred goal;
    not rescued by a helping task; rescued by a `waiting` external or by a
    habit in play that helps, a new one included. Complete a project, then
    seal: no finding about the done project. Finish a goal's last
    prerequisite, then seal: Broken plan until `sufficient` is appended.
    Check before and after each `Done`.
13. Valuation tiers and reasons for the explainer's examples; identical
    across runs; unaffected by dismissals; tiers 1, 3, 4 empty before the
    first review; a deferred goal in tier 3 and its dormant tasks in no
    tier; a goal with a broken plan in tier 0 with `BrokenPlan`; `DeadlineIn`
    omitted with fewer than two increasing review timestamps.
14. With `stale_review_after_days` set and no evaluation time, analysis
    errors; unset, no evaluation time is required. `first_sealed_review`
    suppresses earlier seal findings.
15. Full-prefix replay: appending a review, stance, event, or edge never
    changes findings at earlier prefixes; two reviews in one plan are two
    seals.
16. Declaration amendments: adding a field or a label to a planning type,
    removing an added one, and deprecating `title` are accepted. Dropping
    `budget`, making `kind` optional or `cost` mandatory, removing `About`
    from `Task`'s `incoming`, or permitting a `Review` successor of another
    type is an inconsistency at the declaration's prefix, naming the unmet
    requirement, and not at the prefix before it.
17. Activation: nodes and edges outside the vocabulary, including an edge
    with a user's own label between two planning nodes, change no planning
    finding at their prefixes; a `Note` with an `About` edge to a `Task` is
    an inconsistency at the edge's prefix.
18. With TypeCheck in the suite, a plan appending a `Review` with a text
    `budget` is rejected, and the planning analyzer emits no finding about
    that review.
19. Dormancy: defer a goal, and every task and habit that serves only it is
    dormant at the deferral's sealing review and at the boundaries before
    it, so none is in the frontier and the goal is not judged for Broken
    plan. A task that also serves a goal in play stays in play. A task under
    the deferred goal with a fresh `committed` stays in play, keeps its
    prerequisites in play, and counts toward the budget. When the deferral
    lapses, all of them are in play again at the next seal. A habit that
    helps only an abandoned goal is dormant. Check before and after the
    deferral, at the seal where it stands for the last time, and at the one
    after.
20. Deferral cap: a `deferred` stance whose `horizon` equals
    `max_deferral_horizon` is accepted; one above it is an inconsistency at
    the stance's own prefix and not at the prefix before; with the key
    `null`, any `horizon` of at least 1 is accepted; a `waiting` stance is
    not capped.
