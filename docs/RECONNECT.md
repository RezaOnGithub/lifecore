# Reconnect analyzer

Status: proposal, not implemented. This document owns the contract of a
general-purpose analyzer, provisionally named Reconnect, packaged as
`lifecore-reconnect` and registered as `Reconnect`. It relies on
[SPEC](SPEC.md), [ANALYZERS](ANALYZERS.md), and [SUGGESTIONS](SUGGESTIONS.md),
and replaces the Re-point suggestion of [PLANNING §8.6](PLANNING.md#86-suggestions).
[RECONNECT-EXPLAINER](RECONNECT-EXPLAINER.md) walks through the same rules
with one running example written as change files.

## 1. Purpose

Superseding a node does not carry its connections forward; keeping one means
appending a new edge on the successor ([SPEC §5](SPEC.md#5-current-and-historical-data)).
Reconnect watches each supersession and offers, as suggestions, the edges that
would carry the predecessor's current connections to the new revision, so that a
revision made through the CLI, a change file, or a draft that kept no edges does
not silently lose relationships. It infers nothing: every suggestion is an
explicit edge the user applies or dismisses.

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
| Eligible edge | An edge `e` of `D` that is present in `before` and whose other endpoint `O` is current in `before`. Section 3 gives the full rule. |
| Equivalent edge | The edge with `e`'s `ty_name`, `e`'s direction, `O` as the same exact endpoint, and `S` in `D`'s place. |

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
**eligible** when it is present in `before`, has `D` as one endpoint, and its
other endpoint `O` is current in `before`. `D`'s own currentness in `before` is
deliberately not required: the second successor of a split necessarily follows
the first, so `D` is already superseded when it is judged, and it must be
treated like the first. For the same reason a late branch from a long-superseded
predecessor is judged like a consecutive split successor ([decision 2](#9-design-decisions)).

For each eligible edge, Reconnect emits one suggestion whose single action adds
the **equivalent edge**: the same `ty_name`, the same exact other endpoint `O`,
the same direction, with `S` in `D`'s place. A suggestion is suppressed when

- `O` is superseded in `before`, since the equivalent edge would fail the
  endpoint check of [SPEC §7.1](SPEC.md#71-append-checks) and choosing among
  `O`'s successors would resolve a connection through a revision tree, which
  the model never does ([decision 3](#9-design-decisions)); or
- the equivalent edge is already present at the judging prefix. This cannot
  hold there, because an edge's endpoints are older than it and `S` is the
  newest entry; the condition is stated so that the rule is complete, and it
  is what the display filter of section 4 evaluates later against the current
  graph.

Consequences of the rule, each worked through in the explainer:

| Case | Behavior |
| --- | --- |
| Split | Each successor is judged at its own prefix against the same eligible edges of `D` and gets its own suggestions. Reconnecting one branch says nothing about the other. |
| Chain `D`, `S1`, `S2` | `S2` copies from `S1` only: edges `S1` acquired, not edges `D` lost. If `S1` never reconnected, `S2` gets nothing for `D`'s edges; the open suggestions for `S1` remain the record of the loss ([decision 4](#9-design-decisions)). |
| `O` revised in the same plan | At `S`'s prefix the suggestion targets `O`, the endpoint current immediately before `S`; the analyzer cannot see `O'` without reading past its boundary. This is acceptable: the finding is correct for its prefix, and once `O'` exists the action is no longer appliable and the filter of section 4 hides it. If the plan also adds `S`-`O'`, as the browser's Reconnection step does, that edge is the intended repair and is not the equivalent edge; no suggestion is emitted for `D`-`O` at `O'`'s prefix because `D` is superseded there. |
| Both endpoints superseded, neither reconnected | No live suggestion after the second supersession. Reconnect follows one supersession at a time; domain analyzers such as planning's dropped-connection warning cover their own cases ([decision 5](#9-design-decisions)). |
| `O` is `D` | Impossible, since endpoints differ ([SPEC §7.1](SPEC.md#71-append-checks)). Were a self-loop ever admitted, `O` would be superseded at the judging prefix and the first suppression applies without a special case. |

Subjects are `e`, `D`, `S`, and `O`; the focus is `S`. The text names the label
and direction and no boundary.

## 4. Sticky

Reconnect suggestions are sticky ([decision 6](#9-design-decisions)).
[ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal)
reserves the flag for an advisory about an event judged once, at the prefix
that event ends, and for conditions too costly to re-judge at every prefix.
Both apply:

- The supersession is an event. Judging it once at `S`'s prefix costs one scan
  of `before` for `D`'s edges; full replay judges every supersession exactly
  once, so Reconnect adds a cost linear in history length per prefix, the same
  shape as TypeCheck's transition findings
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
does not hide the second.

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
`deny_unknown_fields`:

```json
{ "filter": { "edge_types": ["Supports"], "node_types": ["Task"] } }
```

`filter` is a `SubgraphFilter` like DAG Check's: `edge_types` restricts the
lost edges considered by label, and `node_types` restricts the successors
judged by `ty_name`. Absent sets mean no restriction. There are no other knobs
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
   long-superseded predecessor is in the same position. Both must get the
   same suggestions as the first successor (section 3).
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

## 10. Open questions

None recorded.

## 11. Acceptance tests

Structural assertions on typed findings and actions, checked before and after
the relevant append.

1. `D` with one outgoing and one incoming current edge, then `S` revises `D`:
   at the prefix ending with `D`'s last edge nothing is emitted; at `S`'s prefix
   two suggestions, each with one `NewEdge` action with exact references and
   `S` in `D`'s position; later prefixes emit nothing, and each observation's
   range is exactly `S`'s prefix.
2. Split: `S1` and `S2` succeed `D`, with other entries between them. `S1`'s
   prefix suggests `S1`'s edges; `S2`'s prefix suggests `S2`'s edges although
   `D` is already superseded; the keys differ by successor only. A branch from
   `D` appended much later behaves like `S2`.
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
   identities; an unknown field is `InvalidAnalyzerConfiguration`.
10. Full-prefix replay: appending nodes, edges, or further revisions never
    changes observations at earlier prefixes, and a candidate with two
    supersessions in one plan yields two independent sets of suggestions.
