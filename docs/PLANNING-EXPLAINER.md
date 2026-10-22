# Reviews as the clock, stances as commitment

A planning analyzer for LifeCore, worked through by example. The rules it
illustrates are owned by [PLANNING](PLANNING.md); this document is the
rationale. It assumes the reader knows LifeCore's append-only graph,
revisions and supersession, exact-endpoint edges, bounded views, configured
analyzers with full-prefix replay, finding keys, observations, and dismissal.

Diagram conventions: solid labeled arrows are edges, dashed arrows are
predecessor links, and position labels such as `p4` count only the nodes
drawn. Each stance and event is followed by its `About` edge, which is an
entry of its own but is not counted.

## 1. The ask

A person wants to plan their life on top of LifeCore. They have goals, the
projects and tasks that lead to them, habits that sustain them, and broad
concerns that never finish. They want two things that pull against each
other.

- **Breadth.** Everything that matters should be in the graph, including work
  they may never do, so that neglect and conflict become visible.
- **Pressure.** Recorded work should push them to act, under finite capacity,
  in a way that is hard to evade, without the database turning into a guilt
  generator.

The analyzer that serves this must coexist with whatever else the user keeps
in the same database. It owns a handful of types, and nothing else.

## 2. Why findings cannot carry it

The obvious design is a set of warnings: "this task has no path to a goal",
"this goal has had no progress", "this item is overdue". In LifeCore those
warnings run into two walls.

**Findings are facts with identity.** Re-emitting a finding merges into one
observation, and dismissing its key hides it for good. That is right for
"this edge closes a cycle". It is wrong for pressure, which needs to *renew*
after being ignored and needs *magnitude*. To fake renewal you would vary the
certificate with a counter, and every variation is a new finding that
accumulates forever. To fake magnitude you would emit escalating variants.
Neither is what the user wants, which is simply what presses most right now.

**Completion by supersession erases the plan.** If "done" means revising a
task with a new status, then the moment a prerequisite is done its dependency
edges become historical, because edges hold exact revisions. An analyzer over
the current graph can no longer tell a bypassed prerequisite from a satisfied
one, and every status change asks the user to re-assert relationships that
did not change.

The design answers both with graph structure rather than analyzer cleverness.
In one breath: an explicit **review** node is the only clock, and the window
between reviews is a macro-transaction whose state is judged only when it is
sealed; the dependency graph records what matters while a **stance** records
what the user has decided; status changes are **events**, not revisions, and a
finished revision is final; every connection names an exact revision, as the
core already says; and pressure between reviews is a **ranking**, not a
finding. The rest of this document earns each claim with an example.

## 3. A first plan

Reza wants to swim a kilometer. Two tasks lead there: book lessons, then
attend eight of them. Here is the graph after the first planning session.

```mermaid
flowchart BT
    G["p1 Goal: Swim 1 km"]
    T1["p2 Task: Book lessons"] -- Enables --> T2["p3 Task: Attend 8 lessons"]
    T2 -- Enables --> G
    S1["p4 Stance: pursued"] -- About --> G
    S2["p5 Stance: committed, cost 1"] -- About --> T1
    R1["p6 Review: budget 3"]
```

> **Items** are the nodes the plan is about: `Goal`, `Project`, `Task`,
> `Habit`, `Concern`, `External`. Two edge labels connect them, both pointing
> the way work flows, from contributor to what it serves. `Enables` is strict:
> the target cannot be finished until the source is. `Helps` is optional: a
> worthwhile contribution that blocks nothing. Their union must be acyclic.

Nothing in `p1` to `p3` says Reza owes anything. The goal exists, the tasks
exist, the dependencies exist. That is *awareness*. Positions `p4` and `p5`
add decisions.

> A **stance** is a node recording one decision about one revision, with a
> `kind`, a timestamp, and an `About` edge to the revision. The kinds are
> `committed`, `deferred`, `waiting`, `pursued`, `sufficient`, and
> `abandoned`. Changing a decision supersedes the stance with a successor,
> as with any node.

At `p4` Reza declares the goal *pursued*. That puts the goal and everything
that strictly enables it **in play**. Recording a goal obligates a pursuit
decision, not action; a goal with no stance is the first thing the analyzer
will ask about. At `p5` Reza commits to booking lessons this coming window at
a cost of one unit. Cost is whatever coarse capacity unit Reza likes; the
analyzer only adds it up.

Then `p6` appends a `Review` with a budget of three. This is the only entry
that moves the analyzer's clock.

> A **review** divides history into **windows**. Appending one **seals** the
> window since the previous review. At that prefix, and only there, the
> analyzer judges the protocol: was every item that needed a decision given
> one, and does the committed work fit the budget. The stances sealed by a
> review are the plan for the window it opens. Between reviews, no protocol
> rule blocks anything.

What does the seal at `p6` check?

1. **The frontier.** Which revisions need a decision right now? `G` is
   pursued, so not it. `T1` has no `Enables` sources, so it is *ready* and
   needs a decision. `T2` is blocked by `T1`; the graph itself says why
   nothing is happening there, so it needs no decision.
2. **Undecided items.** `T1` has a `committed` stance in this window. Nothing
   is undecided.
3. **Budget.** Committed cost 1, budget 3.
4. **Broken plans.** Can `G` advance at all? The chain of tasks that strictly
   enable it contains a ready one. Yes.

**Seal passes.** The transaction containing `p6` commits.

## 4. The next window

Reza books the lessons and comes back a week later.

```mermaid
flowchart BT
    T1["p2 Task: Book lessons"] -- Enables --> T2["p3 Task: Attend 8 lessons"]
    D1["p7 Done"] -- About --> T1
    S3["p8 Stance: committed, cost 2"] -- About --> T2
    R2["p9 Review: budget 3"]
```

> **Events** record that something happened to a revision. `Done` completes a
> task, project, or external; `Did` records one act of participation in a
> habit or attention to a concern. An event has a timestamp and an `About`
> edge, and is never revised. Completion is an append, not a revision of the
> item, and a done revision is final: it can never be superseded.

Look at what `p7` did not do: it did not supersede `T1`. The edge `T1 → T2`
is still current. Because of that, the analyzer can judge the `Done` against
the plan: at the prefix of its `About` edge it checks that every `Enables`
source of `T1` is done. `T1` has none, so the event is accepted.

Had Reza instead tried to mark `T2` done while `T1` was still open, that
`Done` would be an inconsistency at its `About` prefix and the transaction
would fail. Events cannot be taken back, so they are the one thing judged the
moment they appear. Completion is recorded against the plan, not around it.

> A stance's lifetime is counted in seals from its position. Relative to a
> seal, a stance appended in the window being sealed is **fresh**; one sealed
> earlier and still within its lifetime is **standing**; one past its lifetime
> has **lapsed**. Fresh or standing, a stance is a **decision**. A `committed`
> stance stands for exactly the one window after the seal that sealed it.

Now the seal at `p9`:

- `T1` is done, so `T2` is ready and enters the frontier. It has a fresh
  `committed` stance. Decided.
- `T1`'s commitment from `p5` was the plan for window 1 and lapses here. Had
  the task still been open it would need a new decision, but it is done.
- Cost 2, budget 3.

**Seal passes.** Suppose `p8` were missing. The seal would fail with one
**Undecided item** inconsistency about `T2` and the review. Reza appends the
stance and retries. The transaction that fails is the one that claimed the
window was reviewed, which is the right one to fail.

> **Why judge at seals?** It follows the pattern TypeCheck uses for type
> transitions: judge once, at a fixed prefix, against the view at that
> prefix. Later entries cannot change the verdict, so replay is stable, and
> the user is never locked out of repairing the graph by the rules that say
> it needs repair.

## 5. Editing a task

Reza decides eight lessons is too few and revises the task to say ten. This
is an ordinary LifeCore revision: a successor node with the same type.

```mermaid
flowchart BT
    T1["p2 Task: Book lessons"] -- Enables --> T2["p3 Task: Attend 8 lessons"]
    T2 -- Enables --> G["p1 Goal: Swim 1 km"]
    T2 -.-> T2b["p10 Task: Attend 10 lessons"]
    S3["p8 Stance: committed, cost 2"] -- About --> T2
```

Every connection to `p3` is now historical: both `Enables` edges and the
stance's `About` edge. That is exactly what SPEC §5 says supersession means,
and the analyzer adds no exception. `p10` has no prerequisites, serves no
goal, and has no decision. It is not even in play, because nothing pursued
reaches it any more.

Help comes from elsewhere. A separate, general analyzer
([RECONNECT](RECONNECT.md)) looks at every supersession and suggests
recreating the predecessor's edges on the successor: here two `Enables`
edges between current revisions, and one new `About` edge from the existing
stance `p8` to `p10`. The core permits the last one because `p8` is current
and `p10` is current; a stance may acquire `About` edges over time, and its
subject is whatever they currently point at. Accept all three and the plan is
as before, with the same stance, and the same lifetime, still counting. That
is why `About` is not one-to-one: a fresh stance would have a fresh position
and a fresh lifetime.

If Reza seals without accepting them, the seal says so: a sticky **Dropped
connection** warning per lost connection, naming `p10`, `p3`, and the edge.
It is a warning rather than a gate because sometimes dropping a connection is
exactly what the edit meant. Inside a window the graph is a working tree; the
seal is where loose ends are named.

> **Why not resolve connections through the revision tree instead?** An
> earlier draft did, for stances and events only, and paid for it with rules
> against split items, against second decision chains, and against chains
> whose roots diverged, because edges still used exact revisions. One
> identity model is simpler, and the reconnection it asks for is the same
> pattern every other LifeCore relationship already follows.

## 6. Splitting a task

The same rule makes splits ordinary. "Attend 10 lessons" turns out to be two
things: a block of six with the club and four private ones.

```mermaid
flowchart BT
    T2b["p10 Task: Attend 10 lessons"]
    T2b -.-> A["p11 Task: Six club lessons"]
    T2b -.-> B["p12 Task: Four private lessons"]
    T1["p2 Task: Book lessons"] -- Enables --> A
    T1 -- Enables --> B
    A -- Enables --> G["p1 Goal: Swim 1 km"]
    B -- Enables --> G
    S3["p8 Stance: committed, cost 2"] -- About --> A
    S4["p13 Stance: deferred, horizon 2"] -- About --> B
```

Both successors are current. Each gets its own edges, re-pointed from the
suggestions, and its own decision: the old commitment now points at the club
block, and the private lessons are deferred. A `Done` about one leaves the
other open. The only thing the two share is their age, because the analyzer
measures how long an item has existed from the root of its revision tree, and
that is the one history query it makes.

## 7. Too much on the plate

A busier window. Reza has three ready tasks, each enabling a pursued goal not
drawn here, and commits to all of them at cost 2 in one transaction. A later
transaction appends a review with budget 5.

```mermaid
flowchart BT
    A["Task: Six club lessons"]
    B["Task: Renew passport"]
    C["Task: Fix the bike"]
    SA["Stance: committed, cost 2"] -- About --> A
    SB["Stance: committed, cost 2"] -- About --> B
    SC["Stance: committed, cost 2"] -- About --> C
    R["Review: budget 5"]
```

**Seal fails.** **Budget overrun**: committed cost 6 against budget 5, with
the review and the three stances as subjects. The stances are already
committed; only the review was rejected. Reza could raise the budget,
re-estimate, or abandon a task. The move that keeps the plan honest is to
change one decision.

```mermaid
flowchart BT
    C["Task: Fix the bike"]
    SC["Stance: committed, cost 2"] -- About --> C
    SC -.-> SC2["Stance: deferred, horizon 1<br/>reason: lessons first"]
    SC2 -- About --> C
    R["Review: budget 5"]
```

The new stance supersedes the old one, as any revision does, and carries its
own `About` edge. The budget sums the current stance of each task, so the
total is now 4. **Seal passes.** The graph now
says, permanently: Reza intended to fix the bike this window and displaced it
for the lessons, with a reason. A list of priorities where everything is
"high" is not a choice. A budget that forces one stance to give way to
another is.

## 8. Deferring, and deferring again

The passport does not get renewed. Window after window, Reza defers it.

```mermaid
flowchart LR
    T["Task: Renew passport"]
    S1["deferred, horizon 1<br/>sealed by Review 3"] -- About --> T
    S2["deferred, horizon 1<br/>sealed by Review 4"] -- About --> T
    S3["deferred, horizon 2<br/>sealed by Review 5"] -- About --> T
    S1 -.-> S2 -.-> S3
```

A `deferred` stance carries a `horizon`, a number of seals. Sealed by review
*j* with horizon *n*, it stands through review *j+n* and lapses after it. It
may also carry an `Until` edge to a task, project, or external, and then it
lapses early as soon as that item is done or abandoned: "after the move" is
a deferral like any other.

| Seal | Decision for the task | Outcome |
| --- | --- | --- |
| Review 3 | `S1` fresh | decided |
| Review 4 | `S1` stands through Review 4; `S2` is fresh anyway | decided |
| Review 5 | `S2` stands through Review 5; `S3` is fresh anyway | decided |
| Review 6 | `S3` stands | decided |
| Review 7 | `S3` stands through Review 7 | decided; lapses after |
| Review 8 | nothing | **Undecided** unless a stance was appended in window 7 |

Each stance supersedes the last, but nothing is read from that chain. Each
lifetime comes from its own position and the list of reviews. What presses
on Reza is not a count of deferrals, which would mean walking the chain, but
the plain fact that the task has been open since Review 2 and is still
open. The ranking (section 14) carries that as
`OpenFor`, and it rises every window whatever Reza writes, until the task is
done or abandoned.

## 9. Work that matters but is not owed

Reza is planning a sabbatical abroad. A language certificate would help, and
would be a fine thing to have, but the sabbatical can go ahead without it.

```mermaid
flowchart BT
    Q["Task: Language certificate"] -- Helps --> P["Project: Sabbatical"]
    P -- Enables --> G["Goal: Live abroad a year"]
    S["Stance: pursued"] -- About --> G
```

This is the awareness-versus-commitment line, drawn by the two edge kinds.
Pursuing `G` puts `P` in play, because `P` strictly enables it. It does not
put `Q` in play, because `Helps` never propagates. `Q` is awareness: it sits
in the graph, it will still be there in five years, it appears in the ranking
as an *available contribution*, and nothing ever demands a decision about it.
There is no stance to write and nothing to delete.

If Reza wants it, Reza pursues it. Suppose the certificate were a project
with its own tasks:

```mermaid
flowchart BT
    Q["Project: Language certificate"] -- Helps --> P["Project: Sabbatical"]
    Q1["Task: Register for the exam"] -- Enables --> Q
    Q2["Task: Sit the exam"] -- Enables --> Q
    Q1 -- Enables --> Q2
    S2["Stance: pursued"] -- About --> Q
```

The `pursued` stance on `Q` puts its strict subgraph in play: `Q1` is ready
and in the frontier, `Q2` is blocked. The optional edge to the sabbatical is
unchanged. Selecting optional work is a decision about the work, not an edit
to the reason it was recorded.

> **What about an earlier draft's `declined` stance?** It existed to silence
> optional work that was in the frontier. With `Helps` carrying no
> obligation, there is nothing to decline, and nothing that has to be
> re-declined window after window.

## 10. Waiting on the world

Back to the sabbatical. Booking flights needs a visa decision Reza cannot
influence.

```mermaid
flowchart BT
    E["External: Visa decision"] -- Enables --> T["Task: Book flights"]
    T -- Enables --> P["Project: Sabbatical"]
    P -- Enables --> G["Goal: Live abroad a year"]
    Q["Task: Language certificate"] -- Helps --> P
    W["Stance: waiting, horizon 2"] -- About --> E
    S["Stance: pursued"] -- About --> G
```

An `External` is an item for something outside the user's control. It is
finished by a `Done` like a task, and the stance peculiar to it is `waiting`,
which must carry a horizon. The horizon is the check-in: after two seals the
wait lapses and Reza must look again, wait again, or abandon.

At the seal: `T` is blocked by `E`, so not in the frontier. `E` is in play
and in the frontier, with a fresh `waiting` stance. `Q` is awareness. Is the
goal **broken**? The rule asks whether anything can advance it: the goal's
strict subgraph, reached over `Enables` only, must contain a ready task, a
ready project, or an open external someone is waiting on; or a pursued habit
must help into it. Here `{P, T, E}` contains a waited-on external.
**Not broken.** The plan is externally gated and says so.

Remove `W` and the seal fails twice over: **Undecided item** for `E`, and
**Broken plan** for `G`, since nothing in its strict subgraph can move. `Q`
does not rescue the goal; optional contributions never do.

When the visa arrives, Reza appends `Done` about `E`. `T` is ready at once
and the ranking shows it undecided. The edge `E → T` is still current, so
when `T` is eventually done, the analyzer confirms the prerequisite really
was satisfied.

## 11. Habits and concerns

Running three times a week helps the 10 km goal. Calling the parents is not a
goal at all; it is a `Concern`, a broad area that never finishes and should
get attention every window.

```mermaid
flowchart BT
    H["Habit: Run, quota 3"] -- Helps --> G["Goal: Finish the 10 km"]
    SG["Stance: pursued"] -- About --> G
    SH["Stance: pursued"] -- About --> H
    C["Concern: Parents, quota 1"]
    SC["Stance: pursued"] -- About --> C
    D1["p12 Did"] -- About --> H
    D2["p13 Did"] -- About --> H
    D3["p14 Did"] -- About --> C
    R["p15 Review"]
```

A habit only `Helps`, so pursuing the goal does not put it in play; Reza
pursues the habit itself, and likewise the concern. Pursued habits and
concerns cannot be committed or completed. They are decided by *doing*: a
single `Did` in the window counts as the decision, even below quota, because
demanding a stance per window on top of the participation would be ceremony.
If Reza is injured, a `deferred` stance about the habit is the decision
instead, and it also suppresses the shortfall below.

The seal at `p15`: both decided; a pursued habit helps into the goal, so it
is not broken; **passes**. It also emits one sticky **Shortfall** warning for
the habit, "have 2, quota 3", with subjects the habit, the review, and the
two `Did` events. The concern met its quota.

Shortfall is sticky for the reason the flag exists: it judges a closed window
once, and the next window gets its own finding with its own review in the
certificate. Nothing has to generate participation opportunities, and no
clock decides when one was missed. The review closes the window, and the
window is where the participation did or did not happen.

## 12. Projects and sufficient goals

The sabbatical project has three strict prerequisites. Two are done; the
third is "Hand over at work".

```mermaid
flowchart BT
    T1["Task: Book flights"] -- Enables --> P["Project: Sabbatical"]
    T2["Task: Sublet the flat"] -- Enables --> P
    T3["Task: Hand over at work"] -- Enables --> P
    P -- Enables --> G["Goal: Live abroad a year"]
    D1["Done"] -- About --> T1
    D2["Done"] -- About --> T2
```

A project is not ready while `T3` is open, so it needs no decision; its open
task does. If Reza appended `Done` about the project now, it would be
rejected at the `About` prefix, exactly as for a task. Once `T3` is done the
project becomes *ready* and enters the frontier, where its options differ
from a task's: accept it with `Done`, which is an explicit acceptance rather
than a mechanical consequence; add another strict prerequisite, which makes
it not ready again; defer it; or abandon it. Satisfying every dependency
makes delivery possible without proving the deliverable was accepted, so
acceptance stays a decision.

Goals never take `Done`. A year later Reza declares the goal `sufficient`.
That stance is legal only when every `Enables` source of the goal is done;
here the project is. Sufficiency is a judgment, but not one that may
contradict the plan. And like a done task, a sufficient goal is final.

## 13. Goals that arrive and goals that leave

Mid-window, Reza adds a goal, "Learn to cook", with no stance. From that
moment it is the first row of the ranking: a sink that is neither pursued nor
otherwise decided is undecided. At the next seal it would be an **Undecided
item**. Reza can pursue it, defer it, or abandon it. A deferred goal comes
back when its horizon lapses, like any other deferred item. Awareness has a
cost, and the cost is one decision, not action.

Later, Reza abandons "Finish the 10 km". The habit "Run" is still pursued in
its own right, so it stays in play and keeps its quota; if Reza no longer
wants to run, that is a second decision. But a project that only strictly
enabled the abandoned goal leaves the active set: no frontier, no rank. It is
not deleted, and its edge is not deleted. If Reza later pursues a goal it
enables, it is back. Breadth is preserved because leaving the active set is a
consequence of a decision about a goal, not an edit to the thing recorded.

## 14. Pressure between reviews

Every protocol rule so far is judged at a seal, and every structural one at
the entry that breaks it. Between seals Reza needs to know what presses, and
that is not a set of facts to be dismissed; it is an ordering. The analyzer
produces a **valuation**: a deterministic ranking at the requested boundary,
each row with structured reasons and rendered text.

> A **valuation** has no finding identity, cannot be dismissed, is never
> recorded as an observation, and is computed only at the boundary asked for.
> It never affects transaction acceptance. Every change in rank has exactly
> one cause: an appended entry.

Taking the examples a few windows further, with the project of section 12
still waiting on its hand-over, the ranking at the head might read, abridged
to rows that illustrate each tier:

| Tier | Item | Reasons |
| --- | --- | --- |
| 0 | Goal: Learn to cook | `Undecided` |
| 0 | Task: Hand over at work | `Undecided` |
| 1 | Task: Six club lessons | `OpenFor { seals: 5 }`, `DeadlineIn { reviews: 2 }` |
| 2 | Concern: Parents | `Shortfall { have: 0, quota: 1 }` |
| 3 | Task: Renew passport | `Standing { kind: deferred, seals_left: 1 }`, `LapsesAtNextSeal`, `OpenFor { seals: 6 }` |
| 4 | Goal: Live abroad a year | `Starved { seals: 2 }` |
| 5 | Task: Language certificate | `Unselected`, `OpenFor { seals: 4 }` |

The tiers, in order of pressure:

0. **Undecided** frontier revisions. Sinks first, then by how many pursued
   goals the item reaches.
1. **Standing commitments** still open, by how long the item has existed,
   then by nearest deadline. The club lessons have been open five windows and
   committed for several of them. That is the honest shape of "the easy tasks
   got done and this one did not", and it rises without an escalation rule.
2. **Pursued habits and concerns below quota** so far this window.
3. **Deferred or waiting**, those lapsing at the next seal first, then by age.
4. **Starved goals and projects**: pursued, but nothing in their strict
   subgraph is committed this window.
5. **Available contributions**: recorded, helpful, unselected.
6. Everything else in play.

Nothing in this table can be clicked away. To move the passport down, Reza
appends a stance or a `Done`. To move the cooking goal down, Reza decides
whether to pursue it. That is what "difficult to evade" means here: not that
the analyzer is loud, but that the only quiet is a written decision.

## 15. Where time enters

Wall-clock time enters in exactly two places, both bounded.

**Deadlines.** Goals and projects may carry a `deadline` timestamp, and each
review carries an `at` timestamp written by the user. Suppose reviews were
sealed on January 5, 12, 19, and 26, and the swimming goal is due March 2.
The median window over the recent increasing review pairs is seven days; the
latest review was 35 days before the deadline; so `DeadlineIn { reviews: 5 }`.
A task inherits the nearest deadline of the projects and goals it enables,
directly or through other items. Everything in the estimate is graph data,
so it is reproducible at any historical boundary.

**A stale review.** If the configuration sets `stale_review_after_days`, the
analyzer requires the runner's explicit evaluation time and errors without
it, then emits one nonsticky warning when the latest review is older than
that, or when there is no review at all. Unset, which is the default, the
analyzer never reads evaluation inputs.

Everything else counts seals. If Reza stops reviewing, the system goes quiet
apart from that one nudge. That is a deliberate dependence on a ritual rather
than on a clock. The alternative, judging windows by wall-clock time, would
mean replaying every historical window under today's frozen evaluation time
and accumulating findings for weeks nobody looked at.

## 16. Where to go next

[PLANNING](PLANNING.md) states the rules compactly: types and edges, stance
kinds and lifetimes, the active set and the frontier, every finding with its
judging prefix, the valuation tiers, configuration and bootstrap, the one
core addition (a valuation hook on the analyzer traits), alternatives
considered, open questions, and acceptance tests.
