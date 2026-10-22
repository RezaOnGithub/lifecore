# Reconnect analyzer

Status: proposal, not implemented. This document owns the contract of a
general-purpose analyzer, provisionally named Reconnect, packaged as
`lifecore-reconnect` and registered as `Reconnect`. It relies on
[SPEC](SPEC.md), [ANALYZERS](ANALYZERS.md), and [SUGGESTIONS](SUGGESTIONS.md),
and replaces the Re-point suggestion of [PLANNING §8.6](PLANNING.md#86-suggestions).

## 1. Purpose

Superseding a node does not carry its connections forward; keeping one means
appending a new edge on the successor ([SPEC §5](SPEC.md#5-current-and-historical-data)).
Reconnect watches each supersession and offers, as suggestions, the edges that
would carry the predecessor's current connections to the new revision, so that a
revision made through the CLI, a change file, or a draft that kept no edges does
not silently lose relationships. It infers nothing: every suggestion is an
explicit edge the user applies or dismisses.

## 2. Trigger and heuristic

Reconnect emits only at a prefix whose newest entry is a node `S` with a
predecessor `D`; every other prefix, including initialization, yields nothing.
Let `before` be the view ending immediately before `S`. An edge `e` of `D` is
**eligible** when it is present in `before`, has `D` as one endpoint, and its
other endpoint `O` is current in `before`. `D`'s own currentness in `before` is
deliberately not required: the second successor of a split necessarily follows
the first, so `D` is already superseded when it is judged, and it must be
treated like the first. For the same reason a late branch from a long-superseded
predecessor is judged like a consecutive split successor.

For each eligible edge, Reconnect emits one suggestion whose single action adds
the **equivalent edge**: the same `ty_name`, the same exact other endpoint `O`,
the same direction, with `S` in `D`'s place. A suggestion is suppressed when

- `O` is superseded in `before`, since the equivalent edge would fail the
  endpoint check of [SPEC §7.1](SPEC.md#71-append-checks) and choosing among
  `O`'s successors would resolve a connection through a revision tree, which
  the model never does; or
- the equivalent edge is already present at the judging prefix. This cannot
  hold there, because an edge's endpoints are older than it and `S` is the
  newest entry; the condition is stated so that the rule is complete, and it
  is what the display filter of section 3 evaluates later against the current
  graph.

Consequences of the rule:

| Case | Behavior |
| --- | --- |
| Split | Each successor is judged at its own prefix against the same eligible edges of `D` and gets its own suggestions. Reconnecting one branch says nothing about the other. |
| Chain `D`, `S1`, `S2` | `S2` copies from `S1` only: edges `S1` acquired, not edges `D` lost. If `S1` never reconnected, `S2` gets nothing for `D`'s edges; the open suggestions for `S1` remain the record of the loss. |
| `O` revised in the same plan | At `S`'s prefix the suggestion targets `O`, the endpoint current immediately before `S`; the analyzer cannot see `O'` without reading past its boundary. This is acceptable: the finding is correct for its prefix, and once `O'` exists the action is no longer appliable and the filter of section 3 hides it. If the plan also adds `S`-`O'`, as the browser's Reconnection step does, that edge is the intended repair and is not the equivalent edge; no suggestion is emitted for `D`-`O` at `O'`'s prefix because `D` is superseded there. |
| Both endpoints superseded, neither reconnected | No live suggestion after the second supersession. Reconnect follows one supersession at a time; domain analyzers such as planning's dropped-connection warning cover their own cases. |
| `O` is `D` | Impossible, since endpoints differ ([SPEC §7.1](SPEC.md#71-append-checks)). Were a self-loop ever admitted, `O` would be superseded at the judging prefix and the first suppression applies without a special case. |

Subjects are `e`, `D`, `S`, and `O`; the focus is `S`. The text names the label
and direction and no boundary.

## 3. Sticky

Reconnect suggestions are sticky. [ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal)
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
application, not the analyzer, closes that gap: when it exports suggestions it
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

## 4. Finding identity

The certificate is the JSON encoding of the typed finding
`LostEdge { edge: EntryId, successor: EntryId }`: the exact identity of the
lost edge and the exact identity of the successor, and nothing about the
boundary, the other endpoint, the label, or the wording. The label and other
endpoint are determined by the edge identity, so the key is complete. This
gives exactly one key per (lost edge, successor): a split's successors have
distinct keys for the same lost edge, and two edges of the same label between
the same nodes cannot both exist. Re-asserting a connection and losing it again
later yields a distinct key, because the re-asserted edge is a new entry and the
later loss comes with a new successor; a dismissal of the first loss therefore
does not hide the second.

## 5. Suggestion shape

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
forced. Structural checks reject it as a duplicate when the user recreated the
edge by hand, or for a superseded endpoint when `O` was revised later. A
rejected export leaves the finding unacknowledged; the user dismisses it.

## 6. Configuration

The configuration is the empty string or a strict JSON object with
`deny_unknown_fields`:

```json
{ "filter": { "edge_types": ["Supports"], "node_types": ["Task"] } }
```

`filter` is a `SubgraphFilter` like DAG Check's: `edge_types` restricts the
lost edges considered by label, and `node_types` restricts the successors
judged by `ty_name`. Absent sets mean no restriction. There are no other knobs.
`""` and `"{}"` behave alike but are distinct analyzer identities
([ANALYZERS §2](ANALYZERS.md#2-finding-identity-and-observations)). The version
is parsed from the crate's `CARGO_PKG_VERSION`; changing the certificate
encoding or the eligibility rule requires a version change.

## 7. Planning analyzer

PLANNING's Re-point suggestion is superseded by this analyzer. Reconnect covers
every case it covered, including the `About` edge from an existing stance or
`Did` to a revised item, which is an eligible incoming edge of the predecessor.
The application registers both factories, and a planning database configures
both in its suite; planning drops section 8.6's Re-point
and keeps the Dropped-connection warning of
[PLANNING §8.5](PLANNING.md#85-at-a-seal-sticky-warnings) as its own seal-time
rule, since judging a connection at a window's close against any current
successor is planning's domain semantics, not a general one.

## 8. Acceptance tests

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
