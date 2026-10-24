# Reconnect analyzer

Status: the analyzer is implemented in `lifecore-reconnect`, but the
application does not register it yet, and neither the export marks of
section 4 nor the planning changes of section 8 are implemented. This
document owns the contract of a general-purpose analyzer, provisionally named
Reconnect, packaged as `lifecore-reconnect` and registered as `Reconnect`. It
relies on [SPEC](SPEC.md), [ANALYZERS](ANALYZERS.md), and [SUGGESTIONS](SUGGESTIONS.md),
and replaces the Re-point suggestion of [PLANNING §8.6](PLANNING.md#86-suggestions).
[RECONNECT-EXPLAINER](RECONNECT-EXPLAINER.md) walks through the same rules
with one running example written as change files.

## 1. Purpose

Superseding a node does not carry its connections forward; keeping one means
appending a new edge on the successor ([SPEC §5](SPEC.md#5-current-and-historical-data)).
Reconnect watches each supersession and offers, as suggestions, the edges that
would carry the predecessor's current connections to the new revision, so that a
revision made through the CLI, a change file, or a draft that kept no edges does
not silently lose relationships. When the predecessor already has other
current revisions, a new branch is offered only the connections none of them
carries, so that a split distributes the predecessor's connections instead of
duplicating them. It infers nothing: every suggestion is an explicit edge the
user applies or dismisses.

## 2. Vocabulary

Reconnect introduces no node types and no edge labels. It works on whatever
types the database declares, and it never requires a declaration of its own.
This section names what it reads and what it emits, and the terms the rules
of section 3 use.

What it reads:

| Term | Meaning |
| --- | --- |
| `S`, the successor | The newest entry of the judged prefix, when that entry is a node with a predecessor. Any node type. |
| `D`, the predecessor | `S`'s predecessor, an exact revision. |
| `before` | The view ending immediately before `S`, that is, the prefix one entry shorter than the judged one. |
| `e`, an edge of `D` | Any edge, of any label, that has `D` as its `from` or its `to` endpoint. |
| `O`, the other endpoint | The endpoint of `e` that is not `D`. |
| Current, superseded | As [SPEC §5](SPEC.md#5-current-and-historical-data) defines them, always evaluated in `before`. |
| `X`, a sibling | A node that is current in `before` and has `D` among its ancestors. Descent is transitive, so a revision of `D`'s other successor is a sibling. `S` is not in `before`, so it is never its own sibling. |
| Equivalent edge, on a node `N` | The edge with `e`'s `ty_name`, `e`'s direction, `O` as the same exact endpoint, and `N` in `D`'s place. Unqualified, `N` is `S`. |
| Claimed edge | An edge `e` of `D` whose equivalent edge on some sibling `X` is present in `before`. Because `X` and `O` are both current there, that equivalent edge is current. |
| Eligible edge | An edge `e` of `D` that is present in `before`, whose other endpoint `O` is current in `before`, and that is not claimed. Section 3 gives the full rule. |

What it emits:

| Output | Shape |
| --- | --- |
| Suggestion | One per eligible edge, at `S`'s prefix only, with a single action. |
| Action | `EntryChange::Edge(NewEdge)` adding the equivalent edge, with `Reference::Existing` for both endpoints. Section 6. |
| Certificate | The JSON encoding of `LostEdge { edge: EntryId, successor: EntryId }`. Section 5. |
| Subjects and focus | Subjects `e`, `D`, `S`, and `O`; focus `S`. |

Reconnect emits no inconsistencies and no warnings. Every finding is a
suggestion, and every suggestion is sticky (section 4).

## 3. Trigger and heuristic

Reconnect emits only at a prefix whose newest entry is a node `S` with a
predecessor `D`; every other prefix, including initialization, yields nothing.
Let `before` be the view ending immediately before `S`. An edge `e` of `D` is
**eligible** when it is present in `before`, has `D` as one endpoint, its
other endpoint `O` is current in `before`, and no sibling `X` carries its
equivalent in `before`. `D`'s own currentness in `before` is deliberately not
required: the second successor of a split necessarily follows the first, so
`D` is already superseded when it is judged, and it is judged like the first.
For the same reason a late branch from a long-superseded predecessor is judged
like a consecutive split successor; what distinguishes the two is only which
sibling edges exist by then ([decision 2](#9-design-decisions)).

For each eligible edge, Reconnect emits one suggestion whose single action adds
the **equivalent edge**: the same `ty_name`, the same exact other endpoint `O`,
the same direction, with `S` in `D`'s place. A suggestion is suppressed when

- `O` is superseded in `before`, since the equivalent edge would fail the
  endpoint check of [SPEC §7.1](SPEC.md#71-append-checks) and choosing among
  `O`'s successors would resolve a connection through a revision tree, which
  the model never does ([decision 3](#9-design-decisions)); or
- `e` is claimed: some sibling `X`, a current descendant of `D` in `before`,
  already has the equivalent edge on `X`, with the same label, the same
  direction, and `O` exactly ([decision 11](#9-design-decisions)). The
  connection has been carried forward on another branch, and `S` is offered
  the rest. The match is exact on label and direction: a sibling's edge with
  another label to `O`, or with `e`'s label in the other direction, claims
  nothing. A sibling's edge to a revision of `O` never comes into question,
  because that revision supersedes `O` and the first suppression already
  applies. How the sibling came by its edge is
  irrelevant: an edge it was created with claims `e` as much as an applied
  suggestion does. Only current siblings count, so an edge that a superseded
  revision once carried and its own successor dropped claims nothing
  ([decision 12](#9-design-decisions)). Order decides: a sibling edge appended
  after `S` is outside `before` and never affects `S`'s suggestions
  ([decision 13](#9-design-decisions)); or
- the equivalent edge on `S` is already present at the judging prefix. This
  cannot hold there, because an edge's endpoints are older than it and `S` is
  the newest entry; the condition is stated so that the rule is complete, and
  it is what the display filter of section 4 evaluates later against the
  current graph.

Consequences of the rule, each worked through in the explainer:

| Case | Behavior |
| --- | --- |
| Consecutive split | `S1` and `S2` succeed `D` with no edge between them. Each is judged at its own prefix against the same eligible edges of `D`, since at `S2`'s prefix `S1` has no edges yet, and each gets its own suggestions. Reconnecting one branch afterwards says nothing about the other. |
| Late branch | `S2` succeeds `D` after `S1` has reconnected some of `D`'s edges. Those edges are claimed and `S2` is offered the rest. If `S1` reconnected everything, `S2` gets nothing; if nothing, `S2` is judged like a consecutive split successor. |
| Chain then branch `D`, `S1`, `S1'`, then `S2` from `D` | `S1'` is a sibling of `S2` and `S1` is not, being superseded. The edges `S1'` carries are claimed; the edges `S1` once carried and `S1'` did not reconnect are not, and are offered to `S2` while the open suggestions for `S1'` remain the record of their loss. |
| Chain `D`, `S1`, `S2` | `S2` copies from `S1` only: edges `S1` acquired, not edges `D` lost. If `S1` never reconnected, `S2` gets nothing for `D`'s edges; the open suggestions for `S1` remain the record of the loss ([decision 4](#9-design-decisions)). `S2` has no siblings unless `S1` was split. |
| `O` revised in the same plan | At `S`'s prefix the suggestion targets `O`, the endpoint current immediately before `S`; the analyzer cannot see `O'` without reading past its boundary. This is acceptable: the finding is correct for its prefix, and once `O'` exists the action is no longer appliable and the filter of section 4 hides it. If the plan also adds `S`-`O'`, as the browser's Reconnection step does, that edge is the intended repair and is not the equivalent edge; no suggestion is emitted for `D`-`O` at `O'`'s prefix because `D` is superseded there. |
| Both endpoints superseded, neither reconnected | No live suggestion after the second supersession. Reconnect follows one supersession at a time; domain analyzers such as planning's dropped-connection warning cover their own cases ([decision 5](#9-design-decisions)). |
| `O` is `D` | Impossible, since endpoints differ ([SPEC §7.1](SPEC.md#71-append-checks)). |

Subjects are `e`, `D`, `S`, and `O`; the focus is `S`. The text names the label
and direction and no boundary.

## 4. Sticky

Reconnect suggestions are sticky ([decision 6](#9-design-decisions)).
[ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal)
reserves the flag for an advisory about an event judged once, at the prefix
that event ends, and for conditions too costly to re-judge at every prefix.
Both apply:

- The supersession is an event. Judging it once at `S`'s prefix costs one scan
  of `before` for `D`'s current descendants and one scan of the edges in
  `before`, which finds both `D`'s edges and those its descendants carry,
  however many descendants there are; full replay judges every supersession
  exactly once, so Reconnect adds a cost linear in history length per prefix,
  the same shape as TypeCheck's transition findings
  ([PERFORMANCE §1](PERFORMANCE.md#1-measurements)).
- A nonsticky version would re-emit at every prefix for every current node
  with a predecessor, scanning the predecessor's edges each time. `edges_of`
  scans the view ([PERFORMANCE §3](PERFORMANCE.md#3-backlog)), so each prefix
  costs nodes times edges and replay grows with the cube of history, which is
  what PERFORMANCE §1 removed. It would also need a rule for when a lost
  connection stops applying, which is the resolution judgment ANALYZERS §3
  says no analyzer may infer.

The user-facing consequence: a suggestion stays in current reports after the
user recreates the edge by hand, until the exported proposal is applied
([SUGGESTIONS §2](SUGGESTIONS.md#2-export)) or the finding is dismissed. The
application, not the analyzer, closes that gap ([decision 7](#9-design-decisions)):
when it exports suggestions it
checks each action against the current graph and marks the export `satisfied`
when the equivalent edge exists and `inapplicable` when either endpoint is
superseded, both as application result types outside the core observation. The
client hides or labels those by default; history mode still shows them. The
analyzer never re-judges and the finding key is unchanged.

The export check does not look at siblings ([decision 13](#9-design-decisions)).
When `S1` and `S2` are consecutive and the user then reconnects `S1`, the
suggestion for `S2` stays live, since the equivalent edge on `S2` does not
exist and its endpoints are current. The sibling rule of section 3 redistributes
only at the analyzer's judging prefix; afterwards the user applies or dismisses
`S2`'s suggestion. Whether a `claimed` export result should close that gap is
open question 1.

Memory: one retained observation per eligible edge per supersession for the
life of the database, each a few exact identities and one action. The total is
bounded by the sum over supersessions of the predecessor's degree, so it grows
with history itself, not with its square, and at human scale, thousands of
revisions with a handful of edges each, it stays on the order of the entry
count.

## 5. Finding identity

The certificate is the JSON encoding of the typed finding
`LostEdge { edge: EntryId, successor: EntryId }`: the exact identity of the
lost edge and the exact identity of the successor, and nothing about the
boundary, the other endpoint, the label, or the wording
([decision 8](#9-design-decisions)). The label and other
endpoint are determined by the edge identity, so the key is complete. This
gives exactly one key per (lost edge, successor): a split's successors have
distinct keys for the same lost edge, and two edges of the same label between
the same nodes cannot both exist. Re-asserting a connection and losing it again
later yields a distinct key, because the re-asserted edge is a new entry and the
later loss comes with a new successor; a dismissal of the first loss therefore
does not hide the second. The sibling rule of section 3 decides whether a
finding exists, not what its key is: a claimed edge yields no observation at
all, and an unclaimed one yields the same key it would have without siblings.

## 6. Suggestion shape

The single action is `EntryChange::Edge(NewEdge)` with the lost edge's
`ty_name` and `Reference::Existing` for both endpoints: `S` in the lost edge's
`from` or `to` position and `O` in the other. There is no `Reference::Added`,
no node, and no second action. The application exports it as a proposal with
`acknowledges` set to the finding key; applying it goes through the ordinary
suggestion workflow of SUGGESTIONS and acknowledges the finding.

Preparation can reject the action. TypeCheck rejects it with `BadEdgeEndpoint`
when `S`'s type does not permit the label, which is correct: the revision may
have changed type deliberately ([SPEC §10.3](SPEC.md#103-gradual-schema-migration)),
and a connection the new type forbids should be dropped or dismissed, not
forced ([decision 9](#9-design-decisions)). Structural checks reject it as a
duplicate when the user recreated the
edge by hand, or for a superseded endpoint when `O` was revised later. A
rejected export leaves the finding unacknowledged; the user dismisses it.

## 7. Configuration

The configuration is the empty string or a strict JSON object with
`deny_unknown_fields`. `filter`, when present, is an object, and each of its
sets an array of type names; `null` or an array in place of an object is an
error, not a spelling of absence:

```json
{ "filter": { "edge_types": ["Supports"], "node_types": ["Task"] } }
```

`filter` is a `SubgraphFilter` like DAG Check's: `edge_types` restricts the
lost edges considered by label, and `node_types` restricts the successors
judged by `ty_name`. Neither restricts which siblings can claim an edge: a
sibling of any type counts, and a claiming edge has the lost edge's label by
definition. Absent sets mean no restriction. There are no other knobs
([decision 10](#9-design-decisions)). `""` and `"{}"` behave alike but are
distinct analyzer identities
([ANALYZERS §2](ANALYZERS.md#2-finding-identity-and-observations)). The version
is parsed from the crate's `CARGO_PKG_VERSION`; changing the certificate
encoding or the eligibility rule requires a version change.

## 8. Planning analyzer

PLANNING's Re-point suggestion is superseded by this analyzer
([decision 1](#9-design-decisions)). Reconnect covers
every case it covered, including the `About` edge from an existing stance or
`Did` to a revised item, which is an eligible incoming edge of the predecessor.
The application registers both factories, and a planning database configures
both in its suite; planning drops section 8.6's Re-point
and keeps the Dropped-connection warning of
[PLANNING §8.5](PLANNING.md#85-at-a-seal-sticky-warnings) as its own seal-time
rule, since judging a connection at a window's close against any current
successor is planning's domain semantics, not a general one.

## 9. Design decisions

One entry per decision: what was decided, what it rejected, and why. Rules
above link here by number.

1. **Reconnection is a general analyzer, not a planning rule.** Rejected:
   keeping the Re-point suggestion inside the planning analyzer. Reason: the
   loss of connections on supersession is a property of the core model, not
   of planning; every edge label, including planning's `About` from a stance
   or `Did`, is an eligible edge of the predecessor, so one general analyzer
   covers every case the planning rule covered and serves databases that do
   not run planning at all (section 8).
2. **The predecessor need not be current in `before`.** Rejected: requiring
   `D` to be current, so that only the first successor of a split is judged.
   Reason: the second successor of a split necessarily follows the first, so
   `D` is already superseded when the second is judged; a late branch from a
   long-superseded predecessor is in the same position. Both must be judged
   like the first successor, or splitting would silently lose every
   connection on every branch but the first. What they are offered is then
   narrowed by decision 11 (section 3).
3. **A suggestion is suppressed when the other endpoint `O` is superseded in
   `before`.** Rejected: suggesting an edge to one of `O`'s successors.
   Reason: the equivalent edge to `O` would fail the endpoint check of
   [SPEC §7.1](SPEC.md#71-append-checks), and choosing among `O`'s successors
   would resolve a connection through a revision tree, which the model never
   does (section 3).
4. **In a chain, a successor copies from its own predecessor only.**
   Rejected: offering `S2` the edges `D` lost that `S1` never reconnected.
   Reason: `S2` is judged against `S1`'s edges in `before`; the open
   suggestions for `S1` remain the record of the loss, and following one
   supersession at a time keeps each finding about exactly one event
   (section 3).
5. **Reconnect follows one supersession at a time.** Rejected: emitting a
   live suggestion after both endpoints of an edge have been superseded
   without reconnection. Reason: the case is domain-specific; planning's
   dropped-connection warning judges it at a window's close against any
   current successor, which is planning's semantics rather than a general
   one (sections 3 and 8).
6. **Suggestions are sticky.** Rejected: a nonsticky suggestion re-emitted at
   every prefix while the connection is missing. Reason: the supersession is
   an event judged once at the prefix that ends it, which
   [ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal) reserves the flag
   for; a nonsticky version would scan every current node's predecessor
   edges at every prefix, growing replay with the cube of history, and would
   need a rule for when a lost connection stops applying, which is the
   resolution judgment no analyzer may infer (section 4).
7. **The application, not the analyzer, marks a stale suggestion.** Rejected:
   the analyzer re-judging a suggestion against later prefixes. Reason: the
   analyzer never re-judges and the finding key is unchanged; the export
   step already compares each action with the current graph, so it marks the
   export `satisfied` or `inapplicable` as application result types outside
   the core observation, and the client filters on them (section 4).
8. **The certificate is `(lost edge, successor)` and nothing else.**
   Rejected: including the boundary, the other endpoint, the label, or the
   wording. Reason: the label and other endpoint are determined by the edge
   identity, so the key is already complete; omitting the boundary keeps
   replay merging intact; and the pair gives exactly one key per split branch
   and a fresh key when a re-asserted connection is lost again, so dismissing
   one loss never hides a later one (section 5).
9. **A type that forbids the label rejects the suggestion rather than being
   bypassed.** Rejected: skipping the suggestion, or forcing the edge.
   Reason: the revision may have changed type deliberately
   ([SPEC §10.3](SPEC.md#103-gradual-schema-migration)); TypeCheck's
   `BadEdgeEndpoint` on the exported proposal tells the user the connection
   the new type forbids should be dropped or dismissed, and leaves the finding
   unacknowledged (section 6).
10. **Configuration is a `SubgraphFilter` and nothing more.** Rejected:
    further knobs. Reason: `edge_types` and `node_types` are the two
    dimensions along which a database would want to narrow a general
    analyzer, and the shape already exists in DAG Check; the configuration
    string is part of finding identity, so every extra knob is another way
    to invalidate dismissals (section 7).
11. **An edge a current sibling already carries is not offered to a new
    branch.** Rejected: offering every branch every edge of `D`, so that a
    late branch is offered connections its siblings already carry forward.
    Reason: in a split the predecessor's connections usually belong to one
    branch or the other, and the analyzer should help distribute them rather
    than duplicate them; a sibling's equivalent edge is the only evidence in
    `before` of where a connection went, and matching it exactly, on label,
    direction, and `O`, keeps decision 3 intact. [SPEC §5](SPEC.md#5-current-and-historical-data)
    treats both branches keeping a connection as ordinary, so this is a bias
    of the suggestion, not a rule of the model: the user can still write the
    duplicate edge by hand. Raised in review; the rule as first written
    offered every branch the same edges (section 3).
12. **Only current siblings claim an edge.** Rejected: letting any descendant
    of `D` in `before`, superseded or not, claim it. Reason: a superseded
    sibling's edge is historical, so the connection is not carried forward in
    the current graph; offering it to the new branch is right, and the open
    suggestion for the sibling's own successor remains the record of that
    loss, exactly as in a chain. The rule also uses the current-descendants
    query [SPEC §9](SPEC.md#9-inspection-and-queries) already defines
    (section 3).
13. **A sibling edge appended after `S` does not affect `S`'s suggestion.**
    Rejected: extending the export check of decision 7 with a `claimed`
    result when a current sibling of the successor carries the equivalent
    edge. Reason: the analyzer is prefix-bounded and judges once, so a
    consecutive split offers both branches every edge, and reconnecting one
    branch afterwards leaves the other's suggestions live; closing that gap
    at export would put Reconnect's notion of siblings into the application,
    which decision 7 keeps to a comparison of the action against the current
    graph. Deferred rather than refused: open question 1 (section 4).

## 10. Open questions

1. Whether the export check should gain a `claimed` result, marked when a
   current sibling of the successor carries the equivalent edge, so that a
   consecutive split redistributes at export time as a late branch does at
   judging time. Decision 13 leaves the export check as decision 7 describes
   it until the consecutive case proves to need more than a dismissal.

## 11. Acceptance tests

Structural assertions on typed findings and actions, checked before and after
the relevant append.

1. `D` with one outgoing and one incoming current edge, then `S` revises `D`:
   at the prefix ending with `D`'s last edge nothing is emitted; at `S`'s prefix
   two suggestions, each with one `NewEdge` action with exact references and
   `S` in `D`'s position; later prefixes emit nothing, and each observation's
   range is exactly `S`'s prefix.
2. Consecutive split: `S1` and `S2` succeed `D`, with other entries but no
   edges of `S1` between them. `S1`'s prefix suggests `S1`'s edges; `S2`'s
   prefix suggests the same edges with `S2` although `D` is already
   superseded; the keys differ by successor only. Reconnecting `S1` after
   `S2`'s prefix leaves `S2`'s observations unchanged and its export neither
   `satisfied` nor `inapplicable`.
3. Chain: `S1` revises `D` and reconnects edge `e` as `e1`, then `S2` revises
   `S1`: `S2`'s prefix suggests the equivalent of `e1` only, with certificate
   `(e1, S2)`. Without `e1`, `S2`'s prefix emits nothing and the `(e, S1)`
   observation remains.
4. Superseded other endpoint: `D`-`O`, then `O'` revises `O`, then `S` revises
   `D`: no suggestion for `D`-`O` at `S`'s prefix. With `S` appended before
   `O'`, the suggestion appears at `S`'s prefix and `O'`'s prefix adds nothing.
5. Both endpoints in one plan `[S, O', S-O']`: `S`'s prefix emits one
   suggestion whose action names `O` exactly; `O'`'s prefix emits nothing; the
   exported proposal is rejected by `prepare` for a superseded endpoint, and
   the export is marked `inapplicable`.
6. Equivalent edge present: after `S`-`O` is appended by hand, the observation
   still ends at `S`'s prefix and stays in the current report; the export is
   marked `satisfied`; applying it is rejected as a duplicate under
   [SPEC §7.1](SPEC.md#71-append-checks); dismissing the key hides it.
7. Certificate distinctness: `S`-`O` recreated as `e2`, then `S'` revises `S`:
   the new key names `e2` and `S'`; dismissing `(e, S)` leaves `(e2, S')`
   visible; two replays of the same history give equal keys.
8. Acknowledgment: applying the exported proposal with `acknowledges` records
   an `Acknowledged` dismissal; when `S`'s type forbids the label, TypeCheck
   rejects the candidate with `BadEdgeEndpoint` and nothing is dismissed.
9. Configuration: `edge_types` excludes other labels and `node_types` excludes
   other successor types; `""` and `"{}"` emit the same findings under distinct
   identities; an unknown or repeated field, `null` for `filter` or a set, and
   an array for the configuration or `filter` are each
   `InvalidAnalyzerConfiguration`.
10. Full-prefix replay: appending nodes, edges, or further revisions never
    changes observations at earlier prefixes, and a candidate with two
    supersessions in one plan yields two independent sets of suggestions.
11. Late branch: `D` with edges `e1` and `e2`, `S1` revises `D` and
    reconnects `e1` only, then `S2` branches from `D`. `S2`'s prefix suggests
    the equivalent of `e2` with certificate `(e2, S2)` and nothing for `e1`;
    with `S2` appended before `S1`'s reconnection instead, both are
    suggested. With `S1` reconnecting both, `S2`'s prefix emits nothing.
12. Claim is exact: `S1` carries an edge of another label to `O`, or of
    `e1`'s label in the other direction. Neither claims `e1`, and `S2`'s
    prefix suggests it. An edge `S1` was created with, in the same proposal as
    `S1`, claims it like an applied suggestion does. With `O'` revising `O`
    and `S1` carrying `e1`'s label to `O'`, `S2`'s prefix suggests nothing,
    since `O` is superseded.
13. Chain then branch: `S1` revises `D` and reconnects `e1` and `e2`, `S1'`
    revises `S1` and reconnects `e1` only, then `S2` branches from `D`.
    `S2`'s prefix suggests `e2`, since `S1`'s equivalent is historical and
    `S1'` carries none, and not `e1`, which `S1'`, a transitive descendant,
    claims. The `(e2', S1')` observation remains.
14. Siblings and the filter: under `node_types` excluding the sibling's type,
    the sibling still claims; under `edge_types` excluding `e1`'s label, `e1`
    is not considered at all, claimed or not.
