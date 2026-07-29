# Crux map: Broad, dependency-driven personal planning

> This is a shared exploratory document, not a specification. Requirement
> headings state what seems desirable. Bullets pressure-test them; nested
> bullets explore possible answers. Nothing here is settled merely because it
> has been written down.

## Question

How can a LifeCore-based planning framework put tasks in a broader context,
document how they support multiple end goals through strict and optional
dependencies, be exhaustive and broad, and create adequate pressure to get
things done?

## LifeCore

LifeCore is a general personal database for building systems, not storage hidden
behind a single-purpose application. It is closer in spirit to an Obsidian
vault: the user brings their own structures and can develop several systems over
the same body of information. LifeCore seeks much stronger coherence than a
loose collection of notes.

The database is a typed, append-only, versioned graph. Nodes and edges accumulate
as history. Newer nodes can supersede older nodes, and type declarations are
themselves nodes in the graph. The active schema—and therefore the meaning of
stored type names—can evolve across history. Inspection and analysis happen in
explicit historical contexts so that later schema changes do not silently
rewrite earlier meaning.

Validation protects graph integrity. Optional analyzers add the rules of
particular user-chosen systems and can interpret the same graph differently.
Types can be created and modified as those systems evolve. A planning analyzer
is therefore one participant in a bring-your-own-systems environment; it does
not own the database, prescribe its universal ontology, or prevent unrelated
systems from coexisting.

## Scope of this exploration

This session is intended to produce a new proposal. The earlier productivity
proposal is not a source of requirements or a boundary on the design.

This map concentrates on the planning model and the rules an analyzer should
enforce. Message presentation, dismissal, and emittance belong to the porcelain
CLI. Dismissal is an act of resistance to a Warning, not proof that its
condition disappeared; a rolling `DiagnosticLocation` may also cause related
Warnings to be emitted again.

Capture ergonomics are also primarily a porcelain concern. Sidecars and other
authoring facilities may reduce the cost of entering richly connected data.
That possibility should not determine the analyzer's semantics prematurely.

Collaborative planning is outside scope. The graph describes one person's
planning system; it does not model shared authority, consent, assignment, or
another person's commitments.

## Current graph framing

All work items, projects, and end goals form a directed acyclic graph. Edges
point in the direction work flows: from a dependency to the work item, project,
or goal it supports. Operational paths should bottom out in immediately
actionable TODO items; sinks are end goals.

A strict dependency must be satisfied before its dependent can be satisfied.
An optional dependency records a valuable contribution whose absence does not
prevent the dependent from being satisfied. Both kinds create pressure to do
unfinished work. Strictness determines whether failure blocks the dependent,
not whether the dependency matters.

A non-TODO item with no strict dependencies—whether it has no dependencies or
only optional ones—lacks a concrete route to completion and should generate
pressure for a strict dependency hierarchy. That hierarchy should eventually
bottom out in actionable TODO items.

Fan-in expresses several dependencies on one node. Fan-out lets one dependency
support several downstream work items, projects, or end goals without
duplicating it. Any source or intermediate node may therefore lie on paths to
several end goals.

Pressure is derived from the current fresh graph. Completing, abandoning, or
otherwise superseding a dependent without reinstating its old dependency edge
makes that connection stale, so it ceases to create current pressure. No
separate pressure-cleanup mechanism is needed.

- Staleness applies when either endpoint is superseded, not only when the
  dependent is superseded. Completing a prerequisite therefore also makes its
  old dependency edge stale.
  - If the edge is not reinstated, the dependent appears to have lost the
    prerequisite precisely when the prerequisite was satisfied.
  - If completing the dependent makes all of its incoming edges stale, an
    analyzer over only the fresh graph cannot tell whether unfinished strict
    prerequisites were bypassed. Freshness-as-cleanup therefore conflicts with
    enforcing dependency order and retaining the explanation of completion.
  - Reinstating every surviving incident relationship on each replacement
    would make a node version an adjacency snapshot, but even a small status
    edit could require a large, atomic fan-out rewrite.
  - Inspecting the pre-replacement graph could enforce the transition without
    retaining those relationships in the current graph. Representing completion
    separately from node supersession could retain them, but would introduce a
    different status/event model.

## Requirements

### 1. Every work item should be traceable to broader end goals

- A dependency path answers “what requires this?” and ultimately “which end
  goals require this?” without forcing each item into one containing project.
- A work item with no path to an end goal has no broader justification inside
  this planning system.
  - The analyzer could pressure for a missing path. We have not decided whether
    an explicitly unclassified or exploratory item is temporarily valid.
  - If unfinished optional work loses its last downstream connection because a
    target was completed, abandoned, or replaced, this pressure encourages the
    user to reconnect it to a still-relevant goal, pivot it, finish it for
    another reason, or abandon it.
- Some necessary work has no inspiring higher goal: paying tax, fixing a leak,
  or recovering after illness may merely prevent loss.
  - Avoiding loss, satisfying an obligation, restoring capacity, and maintaining
    a condition may themselves be legitimate end goals.

### 2. The DAG should express reusable strict and optional dependencies

- One prerequisite may be required by several downstream nodes. Fan-out must
  preserve one identity for its effort and completion state.
  - Downstream views can each include it, while global capacity accounting must
    not charge the same work several times.
- Several incoming strict dependencies mean that every strict predecessor is
  required before the dependent node can be satisfied.
- Optional dependencies identify worthwhile contributions without claiming
  that they are necessary for the dependent's success.
  - Every unfinished optional dependency creates pressure while its downstream
    target remains relevant, but it never blocks that target's satisfaction.
  - A terminal target and a stale optional edge create no current contribution
    pressure.
  - An optional dependency that is not itself a TODO needs its own strict
    hierarchy down to actionable TODOs. Otherwise the contribution remains
    aspirational rather than executable.
- With mixed predecessors, strict dependencies gate satisfaction while optional
  dependencies add non-gating execution pressure.
- Project nesting is not a tree when a subproject is a prerequisite to several
  larger projects or goals.
  - Traversal and accounting must deduplicate nodes reached by multiple paths.
- A dependency cycle makes the prerequisite order impossible.
  - Cycles involving either edge kind violate the overall DAG, with
    deterministic reporting that remains finite even when several cycles
    overlap.

### 3. Completion and satisfaction should be defined separately for each node class

- A TODO is a small unit of work. It has an ordinary completion event: the work
  was done.
  - A topological source is not automatically a valid TODO. It may still be
    vague, deferred, externally blocked, or missing required information.
  - The analyzer needs a positive definition of actionable, or rules that force
    every unfinished operational path to end in an actionable TODO.
- A Project is organized around a defined deliverable, which makes adding
  dependencies comparatively straightforward.
  - Satisfying every dependency may make delivery possible without proving that
    the deliverable was accepted. Project completion may still require an
    explicit decision.
- A Cycler represents an extremely broad concern that may require lifelong,
  periodic reassessment. It is pacified rather than completed.
  - Pacification may consist of creating a TODO, establishing a Habit,
    reassessing a dormant goal's feasibility, or taking another deliberate
    response.
  - If creating a TODO immediately pacifies a Cycler, repeatedly planning work
    without completing it could satisfy the Cycler while evading its underlying
    concern. Follow-through may need separate pressure.
- A Habit defines recurring participation, not one completable action. “Run”
  and “swim” name activities or Habits; they are not immediately actionable
  TODO items.
  - The dependency graph can remain acyclic while a Habit produces a sequence
    of participation opportunities. A timeless Habit node alone does not say
    which opportunity currently exists, was satisfied, or was missed.
    - Deriving opportunities from wall-clock time makes analysis depend on an
      explicit evaluation time if historical results are to be reproducible.
      Recording each opportunity in the graph preserves history but requires
      some actor other than the analyzer to create the records reliably.
  - When a Habit is a strict dependency of a finite Project, each pending
    participation instance can be treated as an unfulfilled work item under
    that Project.
  - A truly missed participation opportunity cannot literally be completed
    later. The system needs a policy for acknowledging the miss, carrying
    compensatory work forward, or otherwise accounting for it without producing
    an unbounded review avalanche.
    - Some opportunities may expire as recorded lapses with no remaining work.
    - A quota may permit interchangeable participation within a time window and
      record only the eventual shortfall.
    - Some missed work may remain as item-specific debt. Flashcard review is an
      example where the queue itself accounts for accumulated missed
      participation, although the resulting avalanche still needs pressure and
      feasibility rules.
  - Adherence does not by itself establish that the Habit meaningfully serves a
    goal or to what degree. That relationship may require periodic reassessment.
- A Fuzzy End Goal names a desired direction whose sufficient degree of success
  is difficult to state precisely. It can depend on several more concrete goals,
  Projects, or Habits and may still carry a deadline.
  - Unlike a Cycler, it should permit life to move on after a sufficient degree
    of success. Its terminal event may be an explicit judgment of sufficiency
    rather than mechanically derived completion.
  - A deadline creates pressure to judge progress and sufficiency, but does not
    make the success criterion less fuzzy.
- An external dependency either has to appear in the graph or remain an
  unexplained reason that a source is not actionable.
  - Requiring it in the graph increases exhaustiveness; allowing an opaque
    blocker creates a boundary through which work can evade pressure.
- Completing a dependency does not prove that its dependent goal was achieved,
  even when the edge is strict.
  - Dependency truth and outcome evidence are separate questions. The analyzer
    can enforce the recorded dependency order without proving the outcome.

### 4. The map should be exhaustive enough to expose neglect and conflict

- Literal exhaustiveness is impossible in an open life: contexts, obligations,
  opportunities, and consequences can always be decomposed further.
  - “Exhaustive” may instead mean bounded completeness against an explicit set
    of currently recognized areas or review questions.
- Requiring every work item to expose every end goal that strictly depends on it
  can produce a dense graph, although intermediate nodes can share paths and
  reduce direct edges.
  - Reachability may provide exhaustive goal context without requiring a direct
    edge from every item to every goal.
- Optional dependencies let the graph retain useful contributions without
  falsely declaring them mandatory for the dependent's success.
  - They still create execution pressure. A broad graph can therefore surface
    many desirable contributions even though only strict branches are gating.
- An unfinished end goal whose dependency subgraph has no unfinished actionable
  source exposes a break in the plan.
  - The break might mean missing decomposition, an opaque external blocker, or a
    goal that should not currently claim to be pursued.
- A checklist of life areas can reveal omission, but the checklist itself can
  become an unquestioned ideology.
  - Areas should be user-authored, versioned, mergeable, and removable rather
    than built permanently into the analyzer.

### 5. The obligations created by broad awareness should be explicit

- It is unclear whether recording a goal means accepting an immediate
  obligation to advance it, an obligation to review it, or only acknowledging
  that it exists. These interpretations produce very different pressure.
- Recording an optional dependency accepts pressure to make its contribution;
  it does not make the dependent's success conditional on that contribution.
  - This gives one edge two meanings: “this would be valuable” and “I currently
    owe action on this.” Under broad capture and finite capacity, those claims
    need not coincide.
  - Removing the edge to decline the work restores feasibility by erasing
    useful context. Keeping it makes honest recognition an indefinite
    commitment. An explicit distinction between recognized contribution and
    selected commitment might preserve both, but selection could become a new
    route for evading pressure.
- If every recognized concern demands action, breadth deliberately creates more
  pressure and makes omission harder to ignore.
  - This may be a desired consequence rather than a flaw. We still need to ask
    what makes the resulting set of demands actionable rather than merely large.
- If only specially selected concerns demand action, unselected but important
  material can disappear indefinitely.
  - Selection would need to be something stronger than a convenient way to
    silence the broader graph.
- A binary active/dormant status is insufficient when “not now” may mean
  scheduled, waiting on someone, blocked, incubating, intentionally neglected,
  or abandoned.
  - More states improve explanation but can also become a taxonomy used to
    rationalize avoidance.
- Deferral without a review trigger is abandonment with nicer language; a
  review trigger without capacity limits is recurring guilt.
  - Deferral might require a reason and a concrete reconsideration condition,
    while each review permits continued deferral.

### 6. Pressure should be difficult to evade and should produce action

- A Warning can be dismissed in porcelain without changing the graph condition
  that emitted it. Continued or newly located Warnings may appropriately nag
  again.
  - The analyzer still needs principled conditions for emittance; persistence
    alone does not distinguish useful pressure from a badly specified rule.
- Turning behavioral pressure into a fatal inconsistency could make ordinary
  planning edits impossible precisely when the graph most needs repair.
  - Non-fatal does not have to mean optional or quiet. The porcelain can preserve
    friction without the analyzer blocking every mutation.
- “No more than one task in progress” creates focus, but real life contains
  waiting, background, emergency, and recurring work.
  - Work-in-progress limits might apply to self-directed attention, not every
    task whose outcome remains unfinished.
- A goal can have a large dependency subgraph yet no executable source.
  - Every pursued unfinished goal might require at least one current, concrete
    TODO that is neither blocked nor waiting.
  - A legitimately waiting goal may have no action that can advance its outcome.
    Requiring an executable TODO then either misdiagnoses a coherent plan or
    encourages a ceremonial “check whether it is still blocked” task.
    Distinguishing a broken plan from an externally gated plan requires evidence
    and a review condition, not merely the absence of an actionable source.
- Pressure from an unfinished optional dependency should flow backward through
  its strict hierarchy to actionable TODOs.
  - Warning only on the abstract optional Project would identify importance
    without helping the user act on it.
- A downstream deadline may make an optional contribution time-sensitive even
  though it remains non-gating.
  - For example, a native-language qualification can be an optional dependency
    of a sabbatical: there is pressure to complete the qualification before
    departure, but the sabbatical can proceed without it.
  - If one item contributes to several downstream targets with different
    deadlines, it remains open which deadline or combination should govern its
    pressure.
    - Pressure may need to remain path-specific: missing one target's useful
      window can resolve that contribution while the same item remains valuable
      to another downstream target.
- A resurfaced Cycler can produce pressure for several legitimate kinds of
  response rather than one predefined task.
  - The analyzer must recognize a meaningful response without allowing
    ceremonial graph edits to pacify every Cycler.
- Habit pressure must distinguish an opportunity that is still actionable from
  one that has truly been missed.
  - Treating every missed instance as ordinary overdue work can accumulate an
    impossible backlog; silently deleting it erases useful evidence.
- Easy tasks can satisfy completion pressure while difficult high-leverage work
  remains untouched.
  - Pressure should inspect commitment age, repeated deferral, and the absence
    of movement on chosen outcomes, not merely completion counts.
- Mechanical urgency can crowd out rest, relationships, reflection, and
  opportunistic work.
  - Capacity, recovery, and deliberate slack may need first-class representation
    if the analyzer is expected to criticize overcommitment as well as inertia.

### 7. The framework should force explicit choice under finite capacity

- A list of priorities is not a choice if all items are “high.”
  - Explicit commitments could have a hard capacity budget or a small
    cardinality limit, making admission of one displace another.
- Estimates create a common unit but are routinely uncertain and can invite
  false precision.
  - Coarse capacity classes or confidence ranges may be sufficient. Minute
    estimates are useful for arithmetic but not necessarily trustworthy enough
    to govern life decisions.
- A task lying on dependency paths to multiple goals may be strategically
  attractive, but maximizing fan-out can neglect essential single-purpose work.
  - Multi-goal reachability should inform selection, not automatically score or
    rank tasks.
- An analyzer can detect that a choice was not documented; it cannot decide
  which sacrifice is right.
  - The system may pressure for a selection and an explicit displaced item
    without prescribing the selection.
- Capacity becomes enforceable only over commitments that share an accounting
  window and unit. TODO execution, Habit participation, Cycler review, and
  waiting for an external event do not consume attention in the same way.
  - Restricting the budget to selected actionable work gives it a coherent
    meaning, but leaves open how recurring and review obligations reserve future
    capacity rather than repeatedly surprising the current selection.

### 8. Goal classes should preserve their different success semantics

- A work container can combine a grouping role, a completion status, and a
  resource budget. A broad goal such as “maintain close friendships” has no
  natural completion point or total budget.
- Projects, Cyclers, Habits, and Fuzzy End Goals should not inherit one shared
  meaning of `Done` merely because each can depend on work.
- These classes combine several independent dimensions: whether something is
  actionable or a container, finite or recurring, and mechanically completed or
  explicitly judged sufficient.
  - A concern can occupy more than one point on those dimensions. Making the
    classes exclusive may require arbitrary classification or parallel nodes;
    modeling the dimensions independently permits nonsensical combinations that
    the analyzer would then need to reject.
- An ongoing area may be considered attended merely because it received some
  completed work, but a trivial action need not constitute meaningful care.
  - Coverage may need a review interval, meaningful evidence, or an explicit
    satisfaction decision.
- Goals without success or failure conditions cannot support honest progress
  assessment.
  - An outcome might state an observable desired condition and a review horizon,
    leaving projects as delivery structures beneath it.
- Quantified outcomes can distort behavior toward what is measurable.
  - Qualitative success criteria and narrative evidence may remain necessary,
    even though analyzers can validate only their presence and relationships.

### 9. Analyzer rules should remain deterministic and honest about their limits

- The analyzer can verify graph shape, current status, ages in graph history,
  capacity sums, and declared relationships. It cannot verify importance,
  sincerity, causality, or whether a person is genuinely overextended.
  - Diagnostics should say “no declared dependency path” or “no actionable
    source,” not “this task is pointless” or “this goal made no progress.”
- Wall-clock deadlines and recurrence intervals are tempting pressure devices,
  but they introduce a distinct dependence on time semantics and execution
  environment.
  - Pressure could use graph events and explicit review records, trusted time,
    or both. The choice remains open.
- A radical framework can still fail through a large, rigid canonical schema.
  - The analyzer might enforce a small protocol—map, select, act, review—over
    user-defined goal and area content rather than encode a universal theory of
    a good life.
- The analyzer nevertheless needs a deterministic way to recognize TODO,
  Project, Cycler, Habit, Fuzzy End Goal, strict dependency, and optional
  contribution roles across user-defined and historically changing types.
  - Canonical type names make the planning ontology rigid. Structural inference
    is ambiguous. Explicit role configuration preserves user control but must
    itself be versioned and may leave historical prefixes temporarily
    uninterpretable.
- Structural rules can make evasion visible as an explicit graph edit, omission,
  or contradiction; they cannot make the declared reason sincere or the
  completed action meaningful.
  - “Difficult to evade” may need to mean difficult to evade while preserving a
    coherent declared account. Stronger behavioral claims exceed what this
    analyzer can observe.

## Candidate crux

We want the graph to record every recognized goal and worthwhile contribution,
and we want recorded work to create pressure that is difficult to evade. But a
broad set of recognized possibilities cannot all be current obligations under
finite capacity. If the same dependency edge means both “this matters” and “I
owe action now,” honest breadth creates chronic overcommitment, while pruning
the graph becomes the easiest way to restore feasibility.

The missing distinction may be between awareness and commitment. Awareness
could create pressure to review or decide; commitment could create pressure to
act. That distinction is not yet an answer: the framework would still need to
make declining, deferring, and releasing a commitment explicit enough that
“not selected” does not become a quiet escape hatch.
