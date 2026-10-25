# Planning analyzer

Status: implemented in `lifecore-prod`, whose factory the application
registers as `Planning`, beside Reconnect; `lifecore init --default-analyzers`
configures both ([INTERFACES §2](INTERFACES.md#2-cli-surface)). The valuation
of section 12 is delivered as `lifecore valuation` and the RPC method
`planning.valuation`. This document owns the contract of
the planning analyzer and its valuation.
[PLANNING-EXPLAINER](PLANNING-EXPLAINER.md) walks through the same design by
example, with every entry written as a change file; a test in
`lifecore-prod` replays that walk and checks every seal it shows. The graph
and analyzer contracts it relies on are [SPEC](SPEC.md),
[ANALYZERS](ANALYZERS.md), the declaration encoding in
[FILES §2.6](FILES.md#26-type-declaration-payloads), and the general
reconnection analyzer of [RECONNECT](RECONNECT.md). Section 13
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
   suggested by the general [RECONNECT](RECONNECT.md) analyzer, checked where
   it is appended, and named at the seal if missing (decisions 3, 4, and
   57).
4. **Every open item is in play; commitment is a stance.** `Enables` and
   `Helps` edges say what matters and oblige no action. A recorded item is
   **in play**, owed a decision but not action, until a `Done` finishes it
   or a `Stance`, one decision about one revision, sets it aside: `deferred`
   for a while, `abandoned` for good, or `sufficient` when a goal is good
   enough. A deferral makes the item **dormant**, and with it everything
   that serves only dormant items, so one decision quiets a whole branch.
   Abandoning does not spread: an item is abandoned only once its
   prerequisites are resolved. Only a `committed` stance owes action
   (decisions 7, 40, 41, 42, and 74).
5. **Status changes are events, not revisions.** Completion appends a `Done`
   about the revision, which stays current, so its edges stay current and a
   bypassed prerequisite is a plain check. A finished revision is final, and
   so is a stance that abandons an item or calls a goal sufficient
   (decisions 9, 11, and 56).
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
| `Stance` | Bookkeeping: one decision about an item | `kind` text (section 3.3); `at` timestamp; `cost` optional integer; `horizon` optional integer; `reason` optional text | Superseded to change the decision, except `abandoned` and `sufficient`, which are final |
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
| `Stance` | `About` | Any item; the stance's `kind` narrows the type (section 3.3) | The decision's subject. One or more over time, each later one to a descendant of an earlier target; a revision has at most one current stance (sections 4 and 8.3). |
| `Done` | `About` | `Task`, `Project`, `External` | The completed revision. At most one; a `Done` without one has no subject and no effect (decision 59). |
| `Did` | `About` | `Habit`, `Concern` | The attended revision. One or more over time, each later one to a descendant of an earlier target (sections 4 and 8.3). |
| `Stance` | `Until` | `Task`, `Project`, `External` | Only from a `deferred` stance, and at most one current. Ends the deferral early when the target is terminal. After a revision of its target it is re-pointed to a descendant, like `About`; a deferral left with no current `Until` lapses after the next seal (sections 6 and 8.3). |

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
| `resumed` | Any item | | Ends a deferral at once: supersedes a `deferred` stance, and the item is in play again unless it is dormant through what it serves (section 7). Never a decision: it decides no frontier revision and has no lifetime. |
| `sufficient` | `Goal` | | Terminal and final: judged good enough. Permitted only once every `Enables` source of the goal is done (section 8.3). |
| `abandoned` | Any item | | Terminal and final. Permitted only once no `Enables` source of the item is open (section 8.3). |

`cost` is permitted only on `committed`, and `horizon` only on `deferred` and
`waiting`; either field on any other kind is an inconsistency (section 8.1).
A `deferred` stance's `horizon` may not exceed the configured
`max_deferral_horizon` (section 11). Someday/maybe is therefore a deferral
that is acknowledged and renewed at least once per cap, and `abandoned` is the
only stance that sets an item aside indefinitely (decisions 43 and 44).

A `resumed` stance about an item that is not deferred is permitted and changes
nothing beyond replacing the stance it supersedes. A revision has at most one
current stance (section 8.3), so a `resumed` stance about a deferred item must
supersede the deferral (decisions 54 and 58). An `abandoned` or `sufficient`
stance has no successor at all (section 8.1): neither can be undone, by
`resumed` or any other kind, and reviving abandoned work means recording a new
item (decision 56).

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
[SPEC §7.1](SPEC.md#71-append-checks) rule 5. An item's own `successors` are
the user's: a transition between item types is allowed, and age counts only
over the run of item types (section 4; decision 71), while a transition between
an item and a bookkeeping type is an inconsistency (section 8.1; decision 72).

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
whose current `About` edges include that revision. A revision has at most one:
section 8.3 rejects an `About` edge from a stance to a revision that already
has a different current stance (decision 58). No lifetime is read from a
stance's ancestors (decision 6). A revision's **state** is `abandoned` if its
current stance is `abandoned`; else `done` if a `Done` currently targets it,
or `sufficient` if its current stance is `sufficient`; else `open`. Every
state but `open` is **terminal**; only `done` satisfies an `Enables` edge.

A terminal revision is final: a successor of a terminal revision is an
inconsistency at the successor's own prefix. Whether the predecessor is
terminal is judged in the view immediately before the successor, as
TypeCheck judges a type transition: at the successor's own prefix the
predecessor is superseded, its `About` edges are historical, and its state
reads `open`. New work is a new item. A split is ordinary: each current
revision carries its own connections, decisions, and state.

The analyzer makes two history queries over revision trees.

- `ancestors` finds a revision's **planning root**: walking back through its
  predecessors, the oldest revision of the youngest unbroken run of item
  types that ends at it. The run stops at the first predecessor that is not
  an item, so time spent as another type, or out of the plan before a
  return, does not count, while a `Task` revised into a `Project` keeps its
  age. The planning root measures how long an item has been part of the plan
  (section 9). A narrower run, of `Habit` and `Concern` revisions only,
  decides whether a habit or concern is new at a seal (section 7)
  (decision 71). `ancestors` also checks that a re-pointed `About` or `Until`
  edge targets a descendant of an earlier target (section 8.3; decisions 57
  and 73).
- The Dropped connection warning looks up the current successors of a
  revision superseded in the window (section 8.5).

Each measures, validates, or reports; none resolves a connection through a
revision tree, so decision 3 holds.

## 5. Windows and seals

```text
... [Review k] s s e s ... [Review k+1] ...
     ^ opens window k                   ^ seals window k, opens window k+1
```

- **Window k** is the run of positions after `Review k` up to and including
  `Review k+1`. Positions up to and including the first review form window 0.
  Every `Review` entry is its own seal, including several in one plan; a
  second review in one plan seals an empty window.
- **Sealing** happens at the prefix whose newest entry is a `Review`. The
  protocol rules of sections 8.4 and 8.5 are judged at exactly those
  prefixes, the way TypeCheck judges a type transition only at the
  successor's prefix (decision 2).
- An entry's **sealing review** is the first review after it, and an entry
  with one is **sealed**. A stance's lifetime counts from its sealing review
  (section 6), and the valuation reads the sealing review of an item's root
  (section 9).
- Stances appended in window k and sealed by `Review k+1` are the plan for
  window k+1. `Review k+1`'s `budget` bounds its sealed commitments.
- At a non-seal boundary, the **open window** is the run since the latest
  review. The valuation (section 9) reports on it, and reads every boundary
  that way: at one whose newest entry is a review, the open window is the
  empty one that review opens (decision 76).

The ritual is: look at the ranking, append decisions, events, and
reconnections, then append the review. If the seal is rejected, adjust and
retry. The transaction that fails is the one that claimed the window was
reviewed.

## 6. Lifetimes

Lifetimes count seals from a stance's position; no lineage is involved
(decision 6). The count starts at the stance's sealing review (section 5).

- `committed` stands through the window its sealing review opens and lapses at
  the next seal.
- `deferred` with `horizon = n` stands through the n-th seal after its sealing
  review and lapses after it; it lapses earlier at any boundary where its
  `Until` target is terminal. A deferral that has an `Until` edge but no
  current one at a seal, because its target was revised and the edge not
  re-pointed, stands through that seal and lapses after it: the thing it
  waited on changed, so the item comes back to be reassessed (decision 73).
- `waiting` stands through `horizon` seals like `deferred`.
- `resumed` has no lifetime: it is never a decision.
- `sufficient` and `abandoned` are terminal and final.

Relative to a seal, a stance appended in the window being sealed is
**fresh**; one sealed earlier and still within its lifetime is **standing**.
Relative to a non-seal boundary, fresh means appended in the open window.
Fresh or standing stances are **decisions**; lapsed ones are history. A
`resumed` stance is the exception: it has no lifetime and is never a
decision, even while fresh (section 3.3; decision 54).

A lapsed stance stays the item's current stance until superseded, but it no
longer decides anything. When a deferral lapses, the item returns to play by
itself, with no new entry, unless the rule of section 7 keeps it dormant
because everything it serves is dormant. A returning item in the
frontier needs a new decision at the next seal.

A deferral runs until it lapses, until its `Until` target is terminal or is
left behind by a revision, as above, or until a successor stance ends it:
`resumed` for any item, `committed` for a task, or `waiting` for an external. A
`resumed` stance ends the deferral at once and decides nothing, so the item is
in play again unless it is dormant through what it serves (decision 54).
Superseding a deferral with another `deferred` stance starts a new lifetime
instead.

## 7. In play, ready, and the frontier

Every term below is judged at one boundary: a seal, or the boundary the
valuation is asked for, which it reads as one in the open window (section 5).
"Decision", "fresh", and "standing" are relative to that boundary as section
6 defines them.

1. An item **serves** the targets of its current outgoing `Enables` edges,
   and of its current outgoing `Helps` edges whose target is not terminal. A
   `Helps` edge into a finished or abandoned item serves nothing (decision
   55); otherwise a `Helps` edge serves exactly as an `Enables` edge does, so
   optional work is owed a decision like any other (decision 42).
2. An open revision is **dormant** when either holds:
   - its current stance is a `deferred` decision;
   - it serves at least one item, and every item it serves is dormant.

   One exception comes first: an open revision whose current stance is a
   `committed` or `waiting` decision is never dormant. Every open revision
   that is not dormant is **in play** (decision 41). So an open revision that
   serves nothing, such as every goal and concern, is in play unless its own
   stance defers it. Section 3.2 keeps the union of current `Enables` and
   `Helps` edges acyclic, so the rule settles every revision, starting from
   those that serve nothing. An open item never serves a terminal one: a
   `Helps` edge into a terminal item serves nothing (item 1), section 8.2
   rejects an `Enables` edge into a terminal target, and section 8.3 rejects
   a `Done`, `sufficient`, or `abandoned` while its target has an open
   `Enables` source (decision 74).
3. So one deferral of a goal makes dormant every task, project, external, and
   habit that serves only that goal, directly or through other dormant items.
   When the deferral lapses, or a `resumed` stance ends it, they all return to
   play with no further entry. A committed task stays in play under a deferred
   goal, and so do the prerequisites that serve it. When what an item
   helped finishes or is abandoned, the helping work and habits whose `Helps`
   target it was serve nothing through that edge, so they stand on their
   own: in play, and asked about like any item (decision 55). Abandoning, by
   contrast, makes nothing dormant. An item can be abandoned only once each
   of its `Enables` sources is done, abandoned, or redirected by a revision,
   so its prerequisites are settled by explicit decisions; and abandoning a
   prerequisite blocks what it enabled until that is revised without it or
   abandoned too (sections 8.3 and 8.4; decision 74).
4. **Ready** means an open `Task` or `Project` whose `Enables` sources are all
   done, or an open `Goal` that has at least one current `Enables` source, all
   of them done; in play or dormant alike (decision 61).
5. The **strict subgraph** of a goal or project is the item itself and every
   open revision reached from it by walking `Enables` backward, target to
   source, through open revisions, in play or dormant.
6. A habit or concern is **new** at a seal when the oldest revision of the
   youngest unbroken run of `Habit` and `Concern` revisions that ends at it
   (section 4) was appended in the window being sealed; at a non-seal
   boundary, when it was appended in the open window. Revising the item does
   not make it new again, but an item that has just become a habit or
   concern, from a task or from outside the plan, is new (decisions 46 and
   71).
7. The **frontier**, the revisions that need a decision, is:
   - every ready `Task` in play;
   - every ready `Project` in play;
   - every ready `Goal` in play;
   - every open `External` in play;
   - every `Habit` or `Concern` in play that is not new.
8. A frontier revision is **decided** when its current stance is a
   `committed` decision, for a task, or a `waiting` decision, for an external,
   or, for a habit or concern, when a `Did` in the window currently targets it
   (decision 19). Otherwise the revision is **undecided**.

Every other answer takes the revision out of the frontier instead of deciding
it there: a `deferred` decision makes it dormant; `abandoned`, `sufficient`,
or a `Done` makes it terminal; a new `Enables` source that is not done makes a
task, project, or goal not ready. A `resumed` stance never decides. A ready
goal or project is decided only by leaving the frontier: `sufficient` for a
goal or `Done` for a project, which is the acceptance of decision 27, a
deferral, `abandoned`, or a new prerequisite (decision 61). Open revisions
that are not ready are blocked by the graph and need no decision.

A goal enters the frontier only when it is ready. Until then, section 8.4
judges each goal in play by whether its plan can advance (decisions 45 and
61). A new habit or concern is in play like any other item, and a new habit
still rescues a plan it helps (section 8.4), but neither is judged for itself
until the seal after the one that finds it new (section 8.5).

## 8. Findings

Every rule names its judging prefix: the node's own prefix, the prefix of a
particular edge, or a seal. Judging once at a fixed prefix keeps replay
stable. Certificates name exact subject entries and never a boundary; seal
certificates also name the review at the seal, so a condition that persists
into the next window is a distinct finding because the world holds a new
review. Where a rule does not list its subjects, they are the judged entry,
for a rule of sections 8.1 to 8.3, and the judged revision and the review,
for a rule at a seal.

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
with a predecessor; a successor of a revision that is terminal in the view
immediately before it (section 4); a node of an item type whose predecessor
is a bookkeeping node, or a bookkeeping node whose predecessor is an item
(decision 72); a successor of an `abandoned` or
`sufficient` stance, whatever its kind, so that abandoning an item or calling
a goal sufficient is as final as a `Done` (decision 56).

At the prefix of a declaration of its types that parses: one inconsistency
per requirement of section 3.4 it does not meet, naming the declaration and
the requirement. TypeCheck reports a declaration that does not parse.

### 8.2 At an `Enables` or `Helps` edge's prefix

Inconsistencies: endpoint types outside section 3.2; a terminal target; a
cycle the edge closes, one finding per strongly connected component with
sorted member identities. A cycle's subjects are the component's nodes and
edges, and its focus is the edge that closes it. An edge with endpoint types
outside section 3.2 is still judged for its target and for the cycle it
closes, which read neither endpoint's type.

### 8.3 At an `About` or `Until` edge's prefix

Inconsistencies: endpoint types outside section 3.2, such as a source that is
not a stance or event or a target that is not an item; a kind that does not
apply to the target's type; `Until` from anything but a `deferred` stance, or a
second current `Until` from the same stance; a second `About` from a `Done`;
any `About` to a revision that is terminal in the view immediately before the
edge, since the `About` edge of a `Done`, or of an `abandoned` or `sufficient`
stance, makes its own target terminal; `Done` whose target has an `Enables`
source that is not done; `sufficient` whose target has an `Enables` source that
is not done; `abandoned` whose target has an open `Enables` source, so that
each prerequisite is finished, abandoned, or redirected by a revision before
what it enables is abandoned (decision 74). These three are judged immediately:
events cannot be taken back (decision 10), and `sufficient` and `abandoned` are
final (decision 56).

Two more inconsistencies concern re-pointing and the one current stance:

- **Re-pointing to an unrelated item.** An `About` edge from a stance or `Did`
  that already has `About` edges, current or historical, whose target has
  none of their targets among its ancestors (decision 57). Re-pointing after
  a revision or a split always passes, since the successor descends from the
  earlier target, and so does every edge the [RECONNECT](RECONNECT.md)
  analyzer suggests. A second `About` edge in one plan to an unrelated item
  is rejected. The same holds for an `Until` edge from a stance that already
  has `Until` edges, current or historical, against their targets
  (decision 73).
- **A second current stance.** An `About` edge from a stance to a revision
  that already has a different current stance (decision 58). Supersession
  passes: once a stance's successor is appended, the old stance is no longer
  current and its `About` edges are historical, so when the successor's
  `About` edge is appended the revision has no current stance. Re-pointing
  passes too, because a revised item's successor starts with no connections.

An `About` or `Until` edge whose endpoint types section 3.2 does not permit
is judged for its endpoints alone: every other rule of this section reads the
source's kind or the target's item type (decision 77).

### 8.4 At a seal: inconsistencies

- **Undecided item.** A frontier revision that is undecided. One finding per
  revision; subjects are the revision and the review; focus the revision.
- **Budget overrun.** The sum of `cost`, counted once per fresh `committed`
  stance that is the current stance of at least one open task, exceeds the
  review's `budget` (decision 60). A stance re-pointed to both halves of a
  split counts once. Subjects are the review and every counted stance. Every
  open task counts, and a `committed` decision keeps its task in play
  (section 7), so no commitment escapes the budget (decision 48).
- **Broken plan.** A `Goal` or `Project` in play whose strict subgraph holds
  no ready item and no open `External` with a fresh or standing `waiting`
  stance, and into which (the item or its strict subgraph) no `Habit` in play
  currently `Helps`. No other helping work rescues a plan (decisions 45 and
  49). Subjects are the goal or project and the review.
- **Blocked by an abandoned prerequisite.** One per current `Enables` edge
  from an abandoned source into an open revision in play. The target can
  never become ready, so the user revises it without that edge, if it goes
  ahead without the prerequisite, or abandons it, which section 8.3 then
  permits, since the source is no longer open. A dormant target is judged
  once it returns to play. Subjects are the edge, its source and target, and
  the review (decision 74).

A dormant goal or project is not judged, and a done project is not open, so
neither is ever broken. A ready goal or project is never broken either: its
strict subgraph holds a ready item, itself, and it is in the frontier instead
(section 7). A goal with no `Enables` source at all is broken unless a habit
in play helps it, so a new goal needs a first ready task, a helping habit, or
a deferral before the next seal (decisions 45 and 61).

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
  Subjects are the connection, each revision it touched that was superseded
  in the window together with that revision's current successors, and the
  review. A warning, not a gate, because dropping a connection is sometimes
  what the edit meant (decision 17). The match is a heuristic, and a few
  misfires are acceptable: an equivalent connection is read as a current
  edge with the same label and direction whose endpoints are each the
  original endpoint, if still current, or one of its current descendants.
  Each connection is judged once, at the seal of the window in which it
  became historical, which is the window in which the first of its endpoints
  was superseded, so the warnings never accumulate across windows (decisions
  75 and 78). Two connections are never asked for: the `Until` edge of a
  deferral that a successor of another kind ended, since only a deferral may
  have one, and the `About` edge of a `Did` from an earlier window, which
  decides and counts nothing any more (decision 83).

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

The valuation ranks revisions at the requested boundary, with the terms of
section 7 judged there as at a boundary in the open window (decision 76).
The ranking covers the revisions in play and, in tier 3 only, the open
revisions whose own current stance is a standing `deferred`. A revision that
is dormant only through what it serves, everything it serves being dormant,
is not ranked (decision 51). Each revision appears once, in the first tier whose members
include it. Rows are ordered by tier, then the tier's keys, then planning-root
position ascending, then the revision's own position ascending, which orders
split siblings that share one (decisions 70 and 71).

| Tier | Members | Keys | Reasons |
| --- | --- | --- | --- |
| 0 | Undecided frontier revisions, ready goals among them, and goals and projects in play whose plan is broken (section 8.4): what the next seal would reject as it stands, but for the standing commitments of tier 1 | goals and concerns first; then fan-out, the distinct goals in play reachable forward over `Enables` and `Helps`, descending | `Undecided` or `BrokenPlan` |
| 1 | Open tasks whose current stance is a standing `committed` | `OpenFor` descending; then the deadline: `DeadlinePassed` first, then `DeadlineIn` ascending, then no deadline | `OpenFor`; `DeadlineIn` or `DeadlinePassed` when present |
| 2 | Habits and concerns in play, not new, below `quota` so far in the open window | shortfall descending | `Shortfall` |
| 3 | Open revisions whose current stance is a standing `deferred` or `waiting` | lapsing at the next seal first; then `OpenFor` descending | `Standing`; `LapsesAtNextSeal` when it applies; `OpenFor` |
| 4 | Goals and projects in play, with a sealed planning root, whose strict subgraph holds no task with a standing `committed` | `Starved` descending | `Starved` |
| 5 | The rest of the revisions in play, including a new habit or concern | planning-root position | `New` for a new habit or concern, else none |

A row carries the reasons its tier's membership and keys read, and no others
(decision 79).

Tier 0's broken plans are decision 50, and its ready goals decision 61. A
standing commitment decides its task in the open window, so the task ranks
in tier 1, but the commitment lapses at the next seal, which asks for a new
one; every tier 1 row needs one by then.
There is no tier of available contributions: work that only helps is in play
like any other (decision 42).

Reasons are structured values with rendered text. Tests assert the
structured value; clients show the text.

- `Undecided`: an undecided frontier revision, a ready goal included.
- `BrokenPlan`: a goal or project in play whose plan is broken (section 8.4).
- `OpenFor { seals }`: seals since the item's planning root (section 4) was
  appended, so an edit never resets it, while time as another type or out of
  the plan does not count (decisions 65 and 71).
- `Standing { kind, seals_left }`: given only for a standing `deferred` or
  `waiting` stance, in tier 3. Tier 1's standing `committed` stances carry no
  `Standing` reason, since a commitment always lapses at the next seal.
- `LapsesAtNextSeal`: with `Standing`, as below.
- `DeadlineIn { reviews }` and `DeadlinePassed`: section 10.
- `Starved { seals }`: seals since the latest review that sealed a `committed`
  stance, current or superseded, with a current or historical `About` edge to a
  task in the item's strict subgraph or to a done task its walk reaches; with
  none, seals since the sealing review of the item's planning root (decisions
  52 and 67).
- `Shortfall { have, quota }`: a habit or concern below quota so far in the
  open window.
- `New`: a new habit or concern (section 7), which ranks in tier 5; a seal
  would give it the Grace period warning instead of judging it (decision
  69).

`seals_left` is the number of seals after the boundary at which the stance
is still a decision, counting the next seal, as the graph stands. A standing
`deferred` or `waiting` stance sealed by review *j* with horizon *n*, at a
boundary in window *k*, has `seals_left` = *j* + *n* − *k*, which is at least
1, because after its last seal it has lapsed and is no longer standing. A
deferral whose `Until` edges are all historical at the boundary, its target
revised and the edge not re-pointed, stands through the next seal and no
further (section 6), so its `seals_left` is 1 (decision 82).
`LapsesAtNextSeal` is given exactly when `seals_left` is 1: the next seal is
the last one the stance decides. In the window that seal opens, a deferred
item returns to play unless it is dormant through what it serves, and a
waited-on external, already in play, needs a new decision (section 6). The
two reasons overlap: `LapsesAtNextSeal` repeats `seals_left: 1`, and it is
what tier 3's first key sorts on.

The valuation has no identity, no dismissal, and no persistence; it never
affects transaction acceptance; it is computed only at the requested
boundary, so it adds nothing to replay cost. Before the first review, tiers 1
to 4 are empty: no stance has been sealed, every root is unsealed, and every
habit and concern is new, so it ranks in tier 5 with `New`. No deadline
reason is produced before the second review.

## 10. Time

- **Deadlines.** A task's deadline is the nearest `deadline` over the goals
  and projects in play reachable forward over `Enables`. Terminal and dormant
  ones are excluded, since a deferral means "not now" (decision 68).
  `DeadlineIn` is the time from the latest review's `at` to the deadline,
  divided by the median window length of a sample, rounded down; the median
  of an even sample is the mean of its two middle lengths (decision 80). The
  sample takes the pairs of consecutive reviews whose `at` strictly increases
  and keeps the last `deadline_sample_windows` of them, a valuation option of
  section 12, 6 by default (decision 62). `DeadlineIn` is omitted with fewer
  than two reviews, no increasing pair, or a sample of 0; a negative result
  is `DeadlinePassed`.
- **Stale review.** With `stale_review_after_days` set, the analyzer requires
  `evaluation_time` and errors without it. At every prefix it emits one
  nonsticky warning when the view holds at least one current item and either
  no review exists (constant certificate) or the latest review's `at` is more
  than `stale_review_after_days` days before the evaluation time (certificate
  naming that review). Unset, evaluation
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
  "max_deferral_horizon": 12,
  "stale_review_after_days": null,
  "first_sealed_review": null
}
```

A key left out of the object takes its default. Unknown fields, repeated
fields, fractions, and negatives reject construction, and so does any JSON
value that is not an object.
`max_deferral_horizon` is a positive integer or `null`. It caps the `horizon`
of a `deferred` stance, judged at the stance's own prefix (section 8.1), and
`null` removes the cap; `waiting` is not capped (decisions 43 and 44). The
cap is part of the configuration string, and so of every planning finding's
identity (decision 15), and an analyzer in the suite is never reconfigured
([INTERFACES §2](INTERFACES.md#2-cli-surface)), so changing the cap means
adding a new planning instance. Section 8.1 judges all history, so adding a
planning instance to a database whose history holds a deferral longer than
its cap is rejected; such a database configures a cap of at least its
longest deferral, or `null` (decision 66).

`first_sealed_review` is an entry position; seals before it are not judged
under sections 8.4 and 8.5, so that adopting the analyzer on a database that
already holds reviews, or upgrading to stricter seal rules, cannot lock the
database behind a historical seal inconsistency (decision 25). It does not
exempt sections 8.1 to 8.3, which judge every prefix of history, so adding the
analyzer to a database whose history uses its names differently is rejected
([INTERFACES §2](INTERFACES.md#2-cli-surface)). Adopting requires a history
that uses planning's vocabulary as planning does; namespacing in the type
system ([PLAN §3](PLAN.md#3-backlog)) is the remedy for one that does not
(decision 66). The configuration string is part of analyzer identity, so
defaults should rarely change (decision 15). The valuation reads nothing from
it (section 12).

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

1. **Library.** `lifecore-prod` exports the analyzer, its factory, registered
   as `Planning`, and a plain function
   `valuation(view, &ValuationOptions) -> Result<Valuation, PayloadError>`
   over a bounded `View`. The output is bounded by its view. The function
   reads payloads as strictly as the analyzer does, so a field it reads that
   is missing or of another kind is an error, which TypeCheck and the
   judgment of declarations of section 8.1 together keep out of a database
   whose suite configures both (decision 81).
   `ValuationOptions { deadline_sample_windows }` defaults to 6 and is not
   analyzer configuration: it affects no finding, so it is no part of any
   finding's identity (decision 62).
   `Valuation { boundary, ranked: [ { revision, tier, reasons: [ { reason, text } ] } ] }`
   is a Serde type owned by the crate; reasons are its own enum, tagged by
   `name` in snake case, with rendered text beside each. Counts of seals and
   events serialize as JSON numbers, and `Shortfall`'s `quota`, `Standing`'s
   `seals_left`, which grows with the stance's `horizon`, and `DeadlineIn`'s
   `reviews` as decimal strings, since what a payload integer bounds and the
   deadline estimate can exceed what a JavaScript number holds exactly.
   The function is pure, so another analyzer may call it itself; nothing
   passes the valuation through the runner (decision 63).
2. **Executable.** `lifecore valuation`, a subcommand of the CLI, opens the
   selected database read-only as a preview ([STORAGE](STORAGE.md)) and
   prints the ranking at the head, or before `--as-of <position>`, as the
   JSON result of the RPC method below
   ([INTERFACES §2](INTERFACES.md#2-cli-surface)). It takes
   `--deadline-sample-windows` and does not read the analysis suite, since
   the valuation needs nothing from the analyzer configuration (decision
   62). Line output is not built; the JSON carries each reason's text.
3. **Application.** The application service's one RPC method,
   `planning.valuation` ([RPC §2](RPC.md#2-methods)), calls the same
   function. Fixtures and the client contract carry the `Valuation` shape.
   The browser client's Plan screen, under Findings, renders the tiers and
   each reason's text with links into the inspector
   ([UI §7](UI.md#7-planning-valuation)).

Nothing else is needed: `ancestors`, for planning roots and the re-pointing
checks; current descendants and the all-edges selection, for the
dropped-connection rule and `Starved`; newest-entry inspection; timestamp
fields; and suggestion actions all exist.

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
    sometimes what the edit meant (section 8.5; decision 64 keeps it a
    warning once re-pointing is one click).
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
    the rule (section 4, decision 3). **Superseded by decision 58.**
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
    8.1 to 8.3 still judge all history (section 11; decision 66).
26. **A `Done` has exactly one `About` edge and is never re-pointed.**
    Rejected: re-pointing a `Done` like a stance or `Did`. Reason: its
    target is final, so there is never a successor to point at (section 4).
    **Revised by decision 59:** at most one `About` edge; never re-pointed
    stands.
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
    **Superseded by decision 62.**
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
    (section 7). **Revised by decision 55:** a `Helps` edge into a terminal
    item serves nothing; the rest stands. **Revised by decision 74:** abandoning
    no longer makes anything dormant, so the rule loses its "or terminal"
    clause.
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
    8 (sections 1, 7, and 9). **Revised by decision 55:** except a `Helps`
    edge into a terminal item, which serves nothing.
43. **Someday/maybe is a deferral, and a configuration key caps a
    deferral's horizon.** Rejected: deferrals of any length, which would let
    a long horizon do what not pursuing an item used to do. Reason (owner):
    a parked item must be acknowledged and deferred again at least once per
    cap, and `abandoned` stays the only indefinite quiet state. Two
    consequences follow. The configuration string is part of finding
    identity (decision 15) and an analyzer is never reconfigured
    ([INTERFACES §2](INTERFACES.md#2-cli-surface)), so changing the cap means
    adding a new planning instance, the concern that led decision 62 to move
    `horizon_windows` out of the configuration. And section 8.1 judges all
    history, so adopting the analyzer on a database whose history holds
    longer deferrals is rejected (decision 66) (sections 3.3, 8.1, and 11).
44. **The cap is `max_deferral_horizon`, 12 by default and `null` for none;
    it caps `deferred` stances only, and is an inconsistency at the stance's
    own prefix.** Rejected: no cap by default; capping `waiting` as well;
    judging the cap at the seal. Reason (owner approved, 2026-10-25): a
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
    (sections 7 and 8.4). **Revised by decision 61:** a ready goal is in the
    frontier and is never broken.
46. **A habit or concern is spared at the seal that finds it new, and that
    seal emits a warning instead.** A habit or concern whose root was
    appended in the window being sealed is not in the frontier and not
    judged for Shortfall at that seal; `root_of` measures it, so a revision
    does not restart the grace. Rejected: judging a new habit at its first
    seal; sparing it silently. Reason (owner): without the grace, a habit
    recorded just before a review is undecided or short at that seal, for a
    window it barely existed in, as a repro showed. The warning says why the
    item was not judged; it is a warning because there is no `info` message
    kind yet ([PLAN §3](PLAN.md#3-backlog)) (sections 7 and 8.5). **Revised by
    decision 71:** newness counts from the youngest unbroken run of habit
    and concern revisions, not from the tree's root.
47. **The grace warning is the sticky Grace period warning, with the item
    and the review as subjects.** Rejected: a nonsticky warning re-emitted
    while the item is new. Reason (owner approved, 2026-10-25): it
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
    decision 21 (section 8.4). **Revised by decision 60:** each stance's
    `cost` counts once, not once per task.
49. **A broken plan is rescued by a ready task or project, a waited-on
    external, or a habit in play that helps, and by no other helping work.**
    Rejected: letting a helping task or project in play rescue the plan, now
    that decision 42 makes it owed a decision. Reason (owner approved,
    2026-10-25): decision 30's reason, that optional contributions carry no
    obligation, no longer holds. What remains is that a `Helps` edge blocks
    nothing, so finishing helping work never brings a goal closer to
    `sufficient` or a project closer to `Done`. Reason for admitting a habit:
    still **not recorded**. Supersedes decision 30 (section 8.4; open question
    17).
50. **Tier 0 of the valuation also ranks the goals and projects in play
    whose plan is broken, with the reason `BrokenPlan`.** Rejected: leaving
    them to the lower tiers. Reason (owner approved, 2026-10-25): tier
    0 holds what the next seal would reject as it stands, and a new goal
    with no plan, which used to rank in tier 0 as an undecided sink, stays
    there (section 9; open question 19).
51. **The ranking shows a deferred item in tier 3, but not the items dormant
    through it.** It covers the revisions in play and, in tier 3, the
    revisions whose own current stance is a standing `deferred`. Rejected:
    ranking only what is in play, which would never show a deferral or that it
    lapses at the next seal; ranking every dormant revision. Reason (owner
    approved, 2026-10-25): it is the smallest change that keeps tier 3 and
    `LapsesAtNextSeal` useful. One row per deferral shows what the user
    decided, and the items it quiets return with it (section 9; open question
    18).
52. **Tier 4's `Starved` counts from the sealing review of the item's root
    when its strict subgraph has never held a standing commitment.** Rejected:
    counting from the item's latest return to play. Reason (owner approved,
    2026-10-25): with no `pursued` stance to count from, the root is the
    anchor the analyzer already reads (`root_of`, section 4); counting from a
    return to play would need the item's dormancy at earlier boundaries
    (section 9; open question 19). **Revised by decision 67:** the root is the
    fallback when no commitment toward the item was ever sealed. **Revised by
    decision 71:** the fallback is the planning root, not the tree's root.
53. **An implementation may assume that payloads fit their declarations,
    and may return an error at a prefix TypeCheck rejects.** Rejected:
    requiring the analyzer never to fail on a payload TypeCheck rejects.
    Reason (owner): the requirement was pedantic. An analyzer error aborts
    the run ([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)), but
    at such a prefix the plan is rejected either way, so only the report
    differs: the error text replaces TypeCheck's finding. If TypeCheck
    rejects a field planning does not read, planning runs normally
    (section 8; decision 39).
54. **A `resumed` stance ends a deferral at once.** It applies to any item,
    carries only `at` and an optional `reason`, and is never a decision: it
    decides no frontier revision and has no lifetime. Superseding a `deferred`
    stance with it puts the item back in play, unless the item is dormant
    through what it serves; about an item that is not deferred it is permitted
    and changes nothing. Rejected: letting deferrals run their course, bounded
    by the cap; letting a `Did` end the deferral of a habit or concern. Reason
    (owner approved, 2026-10-25): with no `pursued` stance, a goal, project,
    habit, or concern had no way out of a deferral before it lapsed: a habit
    deferred for four seals for a knee injury stayed dormant after the knee
    healed in two. A task could already be resumed with `committed` and an
    external with `waiting` (sections 3.3 and 6; open question 20).
55. **A `Helps` edge into a terminal item serves nothing.** An item serves the
    targets of its current `Enables` edges and of its current `Helps` edges
    whose target is not terminal; the rest of decision 41 stands. Rejected:
    letting a terminal `Helps` target spread dormancy, which left helping work
    and habits dormant for good once what they helped finished or was
    abandoned, with nothing ever asking about them, a bucket list by another
    door; a special frontier entry for such items; and letting only dormant
    targets spread dormancy, which would put the open prerequisites of an
    abandoned item back in play. Reason (owner approved, 2026-10-25): helping
    work and habits stand on their own once what they helped is finished, so
    they stay in play and are asked about like any item; an open prerequisite
    of an abandoned item stays dormant, as the abandonment implies; and an
    open `Enables` source of a done or sufficient item cannot exist (sections
    8.2 and 8.3). Revises decisions 41 and 42 (section 7; open question 21).
    **Revised by decision 74:** an open `Enables` source of an abandoned item
    can no longer exist.
56. **A successor of an `abandoned` or `sufficient` stance is an inconsistency
    at the successor's own prefix.** Neither can be undone, by `resumed` or
    any other kind, and reviving abandoned work means recording a new item.
    Rejected: un-abandoning by superseding the stance, which left the item
    open again. Reason (owner approved, 2026-10-25): undoing would make
    `abandoned` an uncapped deferral, the bucket list again (decisions 11 and
    43). A mistaken `abandoned` is as final as a mistaken `Done` (section 8.1;
    open question 10).
57. **A re-pointed `About` edge must target a descendant of an earlier
    target.** At an `About` edge's prefix, when its source, a stance or `Did`,
    already has `About` edges, current or historical, the new target must have
    one of their targets among its ancestors. Rejected: leaving re-pointing
    unchecked, which let a `deferred` stance with horizon left decide an
    unrelated task at the next seal with no fresh decision, and let one stance
    be about two unrelated items, against principle 4. Reason (owner approved,
    2026-10-25): Reconnect's suggestions always satisfy the rule, so it costs
    an honest user nothing. It adds `ancestors` to the history queries, for
    validation and not resolution, so decision 3 holds (sections 4 and 8.3;
    open question 8).
58. **A revision has at most one current stance.** An `About` edge from a
    stance to a revision that already has a different current stance is an
    inconsistency at the edge's prefix. Supersession passes, because the
    superseded stance's edges are historical by the time its successor's edge
    is appended. Rejected: several root stances, the latest by position
    winning (decision 22). Reason (owner approved, 2026-10-25): with several,
    an outranked stance stayed current with no sign of which one counted,
    re-pointing only the older one after a revision quietly revived it, and
    Dropped connection warned about the outranked stances' edges. The one use
    was changing the decision for one branch of a split while a shared clocked
    stance kept the other branch's lapse. That is given up: the shared stance
    must be branched instead, so the unchanged branch's lapse moves at least
    one window later, bounded by the cap of decision 43, and the cost is
    accepted. A `resumed` stance about a deferred item must therefore
    supersede the deferral. Supersedes decision 22 (sections 4 and 8.3; open
    question 9).
59. **A `Done` has at most one `About` edge, and a `Done` without one has no
    subject and no effect.** Rejected: requiring exactly one, which no prefix
    can judge, because the edge comes after the node; judging orphan `Done`
    events at the seal. Reason (owner approved, 2026-10-25): an orphan `Done`
    completes nothing, so there is nothing to judge. Revises decision 26,
    whose "never re-pointed" stands (section 3.2; open question 11).
60. **Budget overrun counts each fresh `committed` stance's `cost` once.** It
    sums `cost` once per fresh `committed` stance that is the current stance
    of at least one open task. Rejected: summing per task (decision 48), which
    counted a stance re-pointed to both halves of a split twice. Reason (owner
    approved, 2026-10-25): splitting a task does not double the work. Revises
    decision 48 (section 8.4; open question 12).
61. **A goal is ready when it has at least one `Enables` source and all of
    them are done, and a ready goal is in the frontier.** A ready goal or
    project is decided only by leaving the frontier: `sufficient` or `Done`, a
    deferral, `abandoned`, or a new prerequisite. A ready goal is never a
    Broken plan, since its strict subgraph holds a ready item, itself; a goal
    with no `Enables` source keeps decision 45's Broken plan unless a habit
    helps it. Rejected: a Broken plan at every seal for a goal whose
    prerequisites are all done; leaving a ready project out of the frontier
    with `Done` left to the ranking. Reason (owner approved, 2026-10-25):
    "broken" was the wrong word for "finished, declare it sufficient". Ready
    projects stay in the frontier because acceptance is a decision (decision
    27) and the review is where it is made. Revises decision 45 (sections 7
    and 8.4; open questions 2 and 14).
62. **The deadline estimate's sample is `deadline_sample_windows`, a valuation
    option, not analyzer configuration.** `valuation(view, &ValuationOptions)`
    takes it, default 6, and the executable takes `--deadline-sample-windows`.
    The sample is the pairs of consecutive reviews whose `at` strictly
    increases, of which the last N are used. Rejected: `horizon_windows` in
    the analyzer configuration. Reason (owner approved, 2026-10-25): it
    affects no finding, yet as configuration it was part of every finding's
    identity, and its name collided with the stance field `horizon`. With
    nothing else the valuation reads from the analyzer configuration, the
    executable no longer reads the analysis suite. `max_deferral_horizon`
    keeps its name. Supersedes decision 38 (sections 10, 11, and 12; open
    question 13).
63. **The valuation stays presentation-only.** Rejected: passing it to other
    analyzers through the runner. Reason (owner approved, 2026-10-25): it is a
    pure library function over a bounded view, which another analyzer can call
    itself (decision 13; section 12; open question 3).
64. **Dropped connection stays a warning.** Rejected: making it a gate once
    re-pointing is one click in the client. Reason (owner approved,
    2026-10-25): decision 17's reason, that dropping a connection is sometimes
    what the edit meant, does not depend on the UI (section 8.5; open question
    4).
65. **`OpenFor` counts from the root of the revision tree.** Rejected:
    counting from the current revision. Reason (owner approved, 2026-10-25):
    an edit must not reset an item's age (section 9; open question 5).
    **Revised by decision 71:** `OpenFor` counts from the planning root.
66. **There is no adoption exemption beyond seals.** Rejected: exempting
    sections 8.1 to 8.3 and the declaration rule before a configured position,
    as `first_sealed_review` does for seals. Reason (owner approved,
    2026-10-25): adopting requires a history that uses planning's vocabulary
    as planning does, and namespacing in the type system ([PLAN
    §3](PLAN.md#3-backlog)) is the remedy for one that does not. A database
    whose history holds deferrals longer than the default cap configures a cap
    of at least its longest deferral, or `null` (section 11; open question 7).
67. **`Starved` counts from the latest review that sealed a commitment toward
    the item.** That is a review that sealed a `committed` stance, current or
    superseded, with a current or historical `About` edge to a task in the
    item's strict subgraph or to a done task its walk reaches; with none, the
    count starts at the sealing review of the item's root. Rejected: an
    undefined "since it last did". Reason (owner approved, 2026-10-25): the
    key must be computable from the view. Counting tasks that are done keeps a
    finished commitment as progress, so completing work does not make an item
    look more starved. Revises decision 52 (section 9). **Revised by decision
    71:** the fallback is the planning root's sealing review.
68. **A task's deadline comes only from goals and projects in play.**
    Rejected: every goal and project reachable forward over `Enables`,
    terminal and dormant ones included. Reason (owner approved, 2026-10-25): a
    deferral means "not now", and a finished or abandoned item presses nothing
    (section 10).
69. **A habit or concern in its grace period carries the reason `New` and
    ranks in tier 5.** Rejected: leaving it among the rest unexplained. Reason
    (owner approved, 2026-10-25): the ranking then says why the item does not
    press, as the Grace period warning does at the seal (decisions 46 and 47;
    section 9).
70. **Rows that tie on every key and on root position are ordered by the
    revision's own position.** Rejected: leaving split siblings, which share a
    root, in an unspecified order. Reason (owner approved, 2026-10-25): the
    valuation must be deterministic (section 9).
71. **Age and newness count over the youngest unbroken run of item types
    that ends at a revision, its planning root.** Walking back through
    predecessors, the run stops at the first revision that is not an item;
    newness uses the narrower run of `Habit` and `Concern` revisions.
    `OpenFor`, `Starved`'s fallback, tier 4's sealed root, and row order use
    the planning root. Rejected: counting from the revision tree's root
    whatever its type, counting from the oldest item revision anywhere in the
    tree, and rejecting cross-type transitions. Reason (owner, 2026-10-25):
    an item's age is how long it has been part of the plan; time as another
    type, or before it left the plan and came back, is not. A task promoted
    to a project keeps its age, and a task that just became a habit has had
    no window to log a `Did` in, so it is new. Revises decisions 46, 52, 65,
    and 67, and closes open question 6 (sections 4, 7, and 9).
72. **A transition between an item and a bookkeeping type is an
    inconsistency at the successor's own prefix.** Rejected: leaving item
    declarations free to permit a `Stance` successor, or another type's to
    permit an item successor of a bookkeeping node. Reason (owner approved,
    2026-10-25): an item and a decision or event about it are different
    things, and no rule reads a revision chain that crosses between them;
    section 3.4 already keeps bookkeeping declarations from permitting other
    types (section 8.1).
73. **An `Until` edge is re-pointed like `About`, and a deferral left
    without a current one lapses after the next seal.** A stance has at most
    one current `Until`; a later one must target a descendant of an earlier
    target. Rejected: one `Until` ever, which left Reconnect's suggestion
    unappliable and turned the deferral into a plain timed one; and ending
    the deferral at every revision of its target. Reason (owner, 2026-10-25):
    a revision of what a deferral waits on may mean the situation changed, so
    the item should come back to be reassessed; but every edit is a revision,
    a typo fix included, so re-pointing keeps the deferral, and only a
    revision left unanswered by the seal ends it (sections 3.2, 6, and 8.3).
74. **Abandoning does not spread: an item may be abandoned only once no
    `Enables` source of it is open, and an abandoned prerequisite of an open
    item in play is a seal inconsistency.** The dormancy rule loses its
    "or terminal" clause, since an open item can no longer serve a terminal
    one. Rejected: letting an abandoned item's prerequisites go dormant for
    good, which decision 41 did, and letting an abandoned prerequisite block
    its target silently. Reason (owner, 2026-10-25): abandonment is final,
    so its consequences must be explicit decisions, the way `Done` and
    `sufficient` already require their prerequisites done (decisions 10 and
    28); items left dormant behind an abandoned one, never asked about and
    never ranked, were a bucket list by another door. When a prerequisite is
    abandoned, its target is revised without it or abandoned too; authoring
    tools can write the cleanup for a whole branch. Revises decisions 41 and
    55 (sections 2, 7, 8.3, and 8.4).
75. **Dropped connection is a heuristic, judged once per connection.**
    Rejected: defining connection equivalence exactly now. Reason (owner,
    2026-10-25): the warning's job is to notify, and a misfire has still
    notified; what matters is that it fires once, at the seal of the window
    that superseded the revision, and so never accumulates long after the
    work is done. It can be refined later (section 8.5).

76. **The valuation reads every boundary as one in the open window, a
    boundary whose newest entry is a review included.** There the open window
    is the empty one the review opens: the stances it sealed are standing, and
    a deferral it was the last seal to decide has lapsed. Rejected: judging
    such a boundary as the seal it is. Reason: **Provisional, owner to
    confirm:** the ranking says what presses in the window the user is in. Read
    as a seal, every commitment the review sealed would be fresh instead of
    standing and so leave tier 1, and an item whose deferral the review decided
    for the last time would stay hidden although it is back in play (sections
    5 and 9).
77. **An `About` or `Until` edge whose endpoint types section 3.2 does not
    permit is judged for its endpoints alone.** Rejected: judging the kind,
    terminal-target, prerequisite, re-pointing, and current-stance rules of
    section 8.3 on it as well. Reason: **Provisional, owner to confirm:** each
    of those rules reads the source's kind or the target's item type, which
    such an edge lacks, so they would only restate the endpoint finding
    (section 8.3).
78. **A connection becomes historical once, at the first supersession of
    either endpoint, and Dropped connection judges it at the seal of that
    window.** If the other endpoint is superseded in a later window, the
    connection is not judged again. Rejected: judging a connection at the seal
    of each window that supersedes one of its endpoints. Reason:
    **Provisional, owner to confirm:** decision 75 asks that each connection be
    judged once, and the window that made it historical is the one in which it
    was dropped (section 8.5).
79. **A valuation row carries the reasons its tier's membership and keys read,
    and no others.** In tier 1, `DeadlinePassed` ranks before every
    `DeadlineIn` and a task with no deadline after them. Rejected: every
    reason that holds on every row, such as `OpenFor` on an undecided task.
    Reason: **Provisional, owner to confirm:** each reason then explains why
    the row is where it is; the rest is a click away in the inspector
    (section 9).
80. **The median of an even sample is the mean of its two middle window
    lengths.** Rejected: the lower or the upper of the two. Reason:
    **Provisional, owner to confirm:** it is the usual median, and the
    estimate is rounded down afterwards anyway (section 10).
81. **The valuation reads payloads as strictly as the analyzer.** A field the
    rules of sections 9 and 10 read that is missing, or of another kind, is
    an error from `valuation`, as a field the analyzer reads is from the
    analyzer. The valuation reads some fields the analyzer may not, such as
    every review's `at` for the deadline estimate. Rejected: lenient reads
    that take such a field as absent. Reason: **Provisional, owner to
    confirm:** one reading of payloads serves both, and TypeCheck with the
    analyzer's judgment of declarations against section 3.4 keeps such
    payloads out of a committed database, so an error means a database whose
    suite lacks TypeCheck or Planning, where planning's behavior is not
    specified (section 12; decisions 39 and 53).

82. **`seals_left` reads the graph as it stands.** A deferral whose `Until`
    edges are all historical at the boundary has `seals_left` 1 and
    `LapsesAtNextSeal`, since the next seal is the last it decides unless an
    edge is re-pointed first. Rejected: *j* + *n* − *k* alone, which promised
    seals the deferral will not decide. Reason: **Provisional, owner to
    confirm:** section 9 defines `seals_left` as the seals at which the stance
    is still a decision, and decision 73 already fixes that number (sections
    6 and 9).
83. **Dropped connection does not ask for an ended deferral's `Until` edge or
    an earlier window's `Did`.** An `Until` connection whose stance was
    superseded, with no current successor that is a `deferred` stance, and an
    `About` connection from a `Did` appended before the window being sealed,
    are not warned about. Rejected: warning about them like any connection.
    Reason: **Provisional, owner to confirm:** the first can never be carried,
    since an `Until` edge from anything but a deferral is an inconsistency,
    so ending a deferral that waited on something always warned; the second
    no longer matters, since only a `Did` in the window decides a habit or
    counts toward its quota, so every edit of a habit warned once per `Did`
    ever recorded about it. Refines decision 75 (section 8.5).

## 14. Open questions

1. *Retired by decision 40.* It asked whether pursuing a habit or concern
   should expire, so that the pursuit is reaffirmed. A habit or concern in
   play is now judged at every seal, and setting one aside takes a deferral
   whose horizon is capped (decision 43).
2. *Decided by decision 61.* A ready project stays in the frontier, because
   acceptance is a decision and the review is where it is made.
3. *Decided by decision 63.* The valuation stays presentation-only; another
   analyzer can call the library function itself.
4. *Decided by decision 64.* Dropped connection stays a warning, whatever the
   client makes of re-pointing.
5. *Decided by decision 65.* `OpenFor` counts from the root, so an edit
   cannot reset an item's age.
6. *Decided by decisions 71 and 72.* Transitions between item types, and
   into or out of the plan, are allowed; age counts from the oldest revision
   of the youngest unbroken run of item types, newness over the run of
   habits and concerns; a transition between an item and a bookkeeping type
   is an inconsistency.
7. *Decided by decision 66.* No adoption exemption beyond seals: namespacing
   is the remedy for a foreign vocabulary, and a cap of at least history's
   longest deferral, or `null`, admits long deferrals.
8. *Decided by decision 57.* A re-pointed `About` edge must target a
   descendant of an earlier target (section 8.3).
9. *Decided by decision 58.* A revision has at most one current stance; the
   later lapse of a branched shared stance is accepted.
10. *Decided by decision 56.* An `abandoned` or `sufficient` stance has no
    successor; reviving abandoned work means a new item.
11. *Decided by decision 59.* A `Done` has at most one `About` edge; one
    without has no subject and no effect.
12. *Decided by decision 60.* Budget overrun counts each fresh `committed`
    stance's `cost` once, not once per task.
13. *Decided by decision 62.* `horizon_windows` became the valuation option
    `deadline_sample_windows`, the last N increasing review pairs.
14. *Decided by decision 61.* A goal whose prerequisites are all done is ready
    and in the frontier, decided by `sufficient`, and is not a Broken plan.
15. *Decided by decision 44.* The owner approved the cap as proposed:
    `max_deferral_horizon`, 12 by default, `deferred` only.
16. *Decided by decision 47.* The owner approved the sticky Grace period
    warning, with the item and the review as subjects.
17. *Decided by decision 49.* The owner approved that a habit is the only
    helping work that rescues a broken plan.
18. *Decided by decision 51.* The owner approved that tier 3 shows deferred
    items themselves and not the items dormant through them.
19. *Decided by decisions 50 and 52.* The owner approved tier 0's broken plans
    and tier 4's count from the root, which decision 67 refines.
20. *Decided by decision 54.* A `resumed` stance ends a deferral at once, for
    any item.
21. *Decided by decision 55.* A `Helps` edge into a terminal item serves
    nothing, so helping work and habits stand on their own.
22. Should the valuation rank fresh stances like standing ones? A deferral
    appended in the open window makes its item dormant at once, but tier 3
    ranks only standing deferrals, so the item is in no tier until the next
    seal; a fresh commitment or wait leaves its task or external in tier 5
    until then. Both follow from tiers 1 and 3 naming standing stances, which
    the implementation keeps (decisions 51 and 76).
23. Should `Starved` count from no earlier than the item's planning root? It
    counts from the latest review that sealed a commitment to a task the
    item's walk reaches, and a done task can be a prerequisite of an item
    recorded long after the task was committed, so a new project can read as
    starved for longer than it has existed. The implementation follows
    decision 67 as written.
24. Should a deferral be able to wait on both halves of a split target? A
    stance has at most one current `Until` (section 3.2), so after its target
    is split only one branch can be re-pointed, and Reconnect's suggestion for
    the other cannot be applied. The deferral then ends when that one branch
    is terminal.

## 15. Acceptance tests

Structural assertions on typed findings or valuation reasons, with
history-sensitive cases checked before and after the relevant append. The
tests of `lifecore-prod` cover each of these and replay the walk of
[PLANNING-EXPLAINER](PLANNING-EXPLAINER.md).

1. Canonical declarations round-trip; bootstrap suggests each missing
   declaration and never an amended one.
2. Endpoint pairs of section 3.2 accepted; reversed and foreign pairs, a
   `Habit` as `Enables` source, and an edge into a terminal target rejected
   at the edge's prefix.
3. Cycles over `Enables`, `Helps`, and mixed reported once per component at
   the closing edge's prefix, not earlier.
4. `Done` with an open `Enables` source rejected at the `About` prefix;
   accepted after the source's `Done`; the edge stays current throughout. A
   successor of a done revision is rejected at its own prefix. A `Done`'s
   `About` edge to an open task is accepted although it makes the task done,
   and a second `Done` about that task is rejected at its `About` edge's
   prefix and not at the prefix before; likewise an `abandoned` stance about
   a habit is accepted, and a `Did` about the abandoned habit is rejected.
5. A split task yields two frontier revisions; a `Done` about one leaves the
   other open; both share `OpenFor`.
6. Revising a task makes its stance's `About` historical; without a new
   `About` edge to the successor, the revision is undecided at the seal and a
   dropped-connection warning names the `About` edge; with one, neither, and
   the stance keeps its lifetime.
7. Seal with an undecided ready task fails; a fresh `committed` makes it
   pass, and so does a fresh `deferred`, which makes the task dormant; a
   successor stance replaces its predecessor for budget and decision. A
   second root stance about a revision that has a current stance is an
   inconsistency at its `About` edge's prefix and not at the prefix before;
   superseding the first stance instead passes.
8. Budget: equal passes, one over fails; each fresh `committed` stance that
   is the current stance of an open task counts once, including one whose
   goal is deferred, and a stance re-pointed to both halves of a split
   counts once.
9. `committed` lapses at the next seal; `deferred` stands exactly `horizon`
   seals and lapses early when its `Until` target is done or abandoned; once
   it lapses, the item is in play again with no new entry. Revise the `Until`
   target and re-point the `Until` edge to the successor before the seal: the
   deferral keeps its lifetime. Revise it without re-pointing: the deferral
   stands through that seal and lapses after it. A second current `Until`,
   or one re-pointed to an unrelated item, is an inconsistency at its edge's
   prefix.
10. A ready task whose only edge is a `Helps` edge into a goal in play is in
    the frontier; deferring the goal makes the task dormant; abandoning the
    goal, or a project's `Done`, leaves a task that only helped it serving
    nothing, in play and in the frontier; no tier ranks available
    contributions.
11. Shortfall counts the window's `Did` events, is sticky, and a single `Did`
    decides a habit in play. Record a habit and seal: one Grace period
    warning, and neither Undecided item nor Shortfall about it. Revise the
    habit in the next window and seal again: judged, because the revision
    does not restart the grace. Defer it and seal: neither in the frontier
    nor short. Let the deferral lapse and seal: judged again. Check before
    and after each append.
12. Broken plan for a goal in play with nothing ready and nothing waited on,
    and for a new goal with no `Enables` source; none for a deferred goal;
    not rescued by a helping task; rescued by a `waiting` external or by a
    habit in play that helps, a new one included. Complete a project, then
    seal: no finding about the done project. Finish a goal's last
    prerequisite, then seal: an Undecided item for the goal and no Broken
    plan; append `sufficient`, or a deferral, and seal: no finding. A ready
    project is undecided until `Done`. Check before and after each `Done`.
13. Valuation tiers and reasons for the explainer's examples; identical across
    runs, with split siblings ordered by their own positions; unaffected by
    dismissals; tiers 1 to 4 empty before the first review; a deferred goal in
    tier 3 and its dormant tasks in no tier; a goal with a broken plan in tier
    0 with `BrokenPlan`, and a ready goal there with `Undecided`; a new habit
    in tier 5 with `New`; `Standing` only on deferred and waiting stances, with
    `LapsesAtNextSeal` exactly when `seals_left` is 1; `Starved` counting from
    the latest review that sealed a commitment to a task in the strict
    subgraph, a task since done included, and otherwise from the planning
    root's sealing review; `DeadlineIn` omitted with fewer than two increasing
    review timestamps, taken over the last `deadline_sample_windows` increasing
    pairs, and ignoring dormant and terminal goals and projects.
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
    `budget` is rejected, whether the planning analyzer returns an error at
    that prefix or not (decision 53), and the planning analyzer emits no
    finding of its own about the budget's type.
19. Dormancy: defer a goal, and every task and habit that serves only it is
    dormant at every boundary from the prefix of the deferral's `About` edge
    through the deferral's sealing review, so none is in the frontier and the
    goal is not judged for Broken plan. A task that also serves a goal in play
    stays in play. A task under the deferred goal with a fresh `committed`
    stays in play, keeps its prerequisites in play, and counts toward the
    budget. When the deferral lapses, all of them are in play again at the next
    seal. A habit that helps only an abandoned goal serves nothing and is in
    play; abandoning a goal that still has an open `Enables` source is an
    inconsistency (test 28). Check before and after the deferral, at the seal
    where it stands for the last time, and at the one after.
20. Deferral cap: a `deferred` stance whose `horizon` equals
    `max_deferral_horizon` is accepted; one above it is an inconsistency at
    the stance's own prefix and not at the prefix before; with the key
    `null`, any `horizon` of at least 1 is accepted; a `waiting` stance is
    not capped.
21. `resumed`: defer a habit and seal, and it is dormant; supersede the
    deferral with `resumed` and seal, and the habit is judged again at once.
    Supersede a task's deferral with `resumed`, and the task is undecided at
    the next seal, because `resumed` decides nothing. A `resumed` stance about
    an item with no stance changes nothing; a root `resumed` stance about a
    deferred item is an inconsistency at its `About` edge's prefix. Check
    before and after each append.
22. Finality: a successor of an `abandoned` or `sufficient` stance, of any
    kind and `resumed` included, is an inconsistency at its own prefix and
    not at the prefix before; a successor of a `deferred` or `committed`
    stance is accepted.
23. Re-pointing: an `About` edge from a stance to a successor of its target,
    and to both branches of a split, is accepted; to an unrelated task, or a
    second `About` edge in one plan to an unrelated item, is an inconsistency
    at the edge's prefix and not at the prefix before. The same holds for a
    `Did`.
24. A `Done` with no `About` edge yields no finding and completes nothing; a
    second `About` edge from a `Done` is rejected at its prefix.
25. Valuation options: a `deadline_sample_windows` other than 6 changes
    `DeadlineIn` and no finding; an analyzer configuration with
    `horizon_windows` is rejected as an unknown field.
26. Field and kind rules: an unknown `kind`, `committed` without `cost`,
    `deferred` or `waiting` without `horizon`, `cost` or `horizon` on a kind
    that forbids it, a negative `cost`, `budget`, or `quota`, a `horizon` of
    0, and a `Review`, `Done`, or `Did` with a predecessor are each an
    inconsistency at the node's own prefix and not at the prefix before. A
    kind about a type it does not apply to, an `Until` from a stance that is
    not `deferred`, and `sufficient` about a goal with an open `Enables`
    source are each an inconsistency at the edge's prefix.
27. Planning root: a `Task` whose predecessor is another type's node counts
    `OpenFor` from the `Task`; a `Task` revised into a `Project` keeps its
    age; an item that left the plan as another type and came back counts
    from its return; a `Task` revised into a `Habit` is new at the next seal,
    with a Grace period warning, and judged at the one after. A `Stance`
    whose predecessor is a `Task`, or a `Task` whose predecessor is a `Did`,
    is an inconsistency at its own prefix.
28. Abandonment: `abandoned` about a goal with an open `Enables` source is
    an inconsistency at its edge's prefix; after the source is done,
    abandoned, or revised without the edge, it is accepted. Abandoning a
    task that `Enables` an open project in play gives a Blocked by an
    abandoned prerequisite inconsistency at the next seal, and none once the
    project is revised without the edge or abandoned; none while the project
    is dormant. Check before and after each stance.
29. Dropped connection: a connection reported at one seal is not reported at
    a later one, even if it is never restored.
