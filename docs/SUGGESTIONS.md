# Suggestions and acknowledgment

Status: selected application design, pending implementation. This document is
the single home for how applying a suggestion dismisses its finding; other
documents link here. [MESSAGES.md](MESSAGES.md) describes the current library
flow; [FILES.md](FILES.md) and [INTERFACES.md](INTERFACES.md) define the shared
proposal, storage, and transport contracts.

## 1. Identities

| Information | Role |
| --- | --- |
| `FindingKey` | Identifies an issue for grouping and dismissal: database, exact analyzer identity/configuration/version, category, and complete opaque certificate. |
| `Observation` | One occurrence of that issue: the contiguous range of prefixes at which it was emitted, wording, and, for a suggestion, its ordered actions. |
| `Proposal` | Ordered graph operations with exact references and explicit revision intent, an optional `plan_id` base expectation, and an optional `acknowledges` finding key. |
| `Dismissal` | Persistent display state for a whole finding key, with a reason and optional note. Not a resolution assertion or an application receipt. |

Observations of one finding may carry different actions at different prefixes
or evaluation times. Actions come from a concrete observation; dismissal targets
the issue identified by its complete key. Analyzers must distinguish materially
different issues in their certificates, so a dismissal also hides later
observations of the same issue.

## 2. Export

For each suggestion observation in an analysis result, the service also returns
a ready-made `Proposal`:

- `plan_id`: an ordinary token for the analyzed snapshot's base.
- `acknowledges`: the observation's complete `FindingKey`.
- `entries`: the observation's actions in order, with generated operation names,
  exact existing references, and revision intents.

```json
{
  "plan_id": "lc1:<database-uuid>:<graph-revision-uuid>:<proposal-uuid>",
  "acknowledges": {
    "database": "<database-uuid>",
    "analyzer": { "name": "<analyzer>", "version": "1.2.0", "configuration": "{}" },
    "kind": "suggestion",
    "analyzer_certificate": "<opaque certificate>"
  },
  "entries": [
    {
      "name": "action_0",
      "kind": "node",
      "ty_name": "Task",
      "predecessor": { "existing": { "origin": "<entry-origin-uuid>", "position": "42" } },
      "intent": "revise",
      "payload": { "title": "Write proposal", "status": "done" }
    }
  ]
}
```

The export is a convenience, not a credential: nothing in it is secret or signed,
and a client could write the same object by hand. Its JSON form is an ordinary
change file; TOML uses the same type, and format conversion preserves both
fields. Exporting changes nothing. Exported proposals from one analysis share a
base; applying one that changes the graph makes the others' tokens stale.

## 3. Verification by replay

`check` and `apply` accept `acknowledges` only if the service's own analysis of
the committed base reproduces the suggestion:

1. The key's database is the selected database and its category is `Suggestion`.
2. An observation evaluated at initialization, or whose prefix range begins
   within the committed base, has exactly this key. The range may extend into
   the candidate's prefixes, since a suggestion that still holds after its own
   first action is re-emitted there and merges into the same observation.
3. Its ordered actions equal the proposal's entries after lowering to core
   changes: exact and earlier-operation references, types, payloads, and revision
   intents. Operation names are authoring handles and are not compared.

This needs no extra analysis run. Preparing any proposal replays every prefix of
the candidate, and the committed base's prefixes are among them. Observations
that first appear at a candidate prefix do not count: they can refer to
provisional entries and would let a proposal vouch for itself.

If verification fails, reject the proposal before mutation with a distinct error;
never apply it silently without the acknowledgment. The user can remove
`acknowledges` to apply the same operations as an ordinary proposal, or analyze
again and select a fresh export.

Consequences:

- Edited actions, or a key belonging to a different finding, fail verification.
- Committed prefixes are immutable and analyzers deterministic, so a suggestion
  stays reproducible after later appends while implementations and inputs are
  unchanged. The `plan_id`, not the acknowledgment, decides staleness.
- Verification uses the apply's selected evaluation inputs and installed
  implementations. A time-dependent suggestion or a changed analyzer version may
  not reproduce; that rejection is intended. Clients apply a selected suggestion
  with the inputs its export used unless the user deliberately changes them.
- Acknowledgment does not certify validity. The operations still pass full-prefix
  checks; missing analyzers, inconsistencies, or invalid intent reject them.

## 4. Atomic application

1. Acquire the writer lock and reload the snapshot, including current dismissals.
2. Verify any `plan_id` against the selected database and base.
3. Prepare and check every prefix through the core transaction path.
4. Verify `acknowledges` against that replay (section 3).
5. Commit the graph into a candidate snapshot. If the finding has no dismissal,
   add `Acknowledged` with no note; otherwise keep the existing reason and note.
   Keep every other dismissal record from the reload.
6. Rotate the graph revision only if graph or suite changed. Publish the whole
   snapshot atomically and synchronize before releasing the lock. Return the
   entry mapping, analysis context, matched observation, and dismissal effect.

A rejected proposal dismisses nothing, and a failure before publication preserves
both graph and dismissals. After publication, synchronization or transport
failure can leave the outcome uncertain, but the published snapshot holds both
effects; report the uncertainty and never replay the write automatically. `check`
runs steps 2–4 and reports the prospective acknowledgment without saving anything.

The core transaction stays an in-memory graph operation; locking, dismissal
staging, and publication belong to the application layer. The proposal has no
general dismissal list; explicit dismiss/restore operations remain separate.

## 5. Display, retries, and empty suggestions

Show the exported observation's context beside the observation matched at
check/apply time; they can differ in evaluation time or prefix. Acknowledgment
does not assert resolution, and restoring the dismissal does not undo graph
changes. No dismissal weakens the unfiltered inconsistency gate.

Acknowledgment records nothing about use. After a graph-changing apply the old
base is stale; that rejection cannot distinguish a completed retry from an
intervening write, and an existing dismissal is not evidence of prior
application. An empty suggestion publishes only its acknowledgment without
changing the graph revision, so restoring and reapplying it acknowledges again.
Durable receipts remain [backlogged](BACKLOG.md#7-deferred-receipts-and-retry-recovery).

## 6. Why replay rather than a signature

An earlier draft signed exported proposals with a per-database HMAC key stored in
the snapshot. Replay gives the same guarantee — only a suggestion the service's
analyzers emit for committed history is acknowledged automatically, and edits
cannot keep the acknowledgment — without a stored secret, a canonical proposal
encoding, or a second token kind. A signature would protect nothing further for a
local single user, who can dismiss any advisory directly. The trade-off: a
signature accepted a suggestion issued under an earlier context, while replay
requires it to hold under the current one.

## 7. Acceptance scenarios

| Scenario | Required outcome |
| --- | --- |
| An exported proposal is applied unchanged | Graph changes and acknowledgment publish together; an existing reason/note is kept. |
| Actions are edited, or `acknowledges` names another finding | Reject before mutation. Without `acknowledges`, the same operations apply as an ordinary proposal and dismiss nothing. |
| The proposal is converted between JSON and TOML, reformatted, or uses integer shorthand | Verification still succeeds; it compares typed values, not text. |
| The suggestion first appears at a candidate prefix | Reject; only observations beginning within the committed base count. |
| Two observations share a key but differ in actions | Each export verifies against its own observation; either acknowledges the same issue. |
| The graph or suite changes after export | The stale `plan_id` rejects that export and its siblings. |
| Evaluation time or an implementation changes and the suggestion no longer reproduces | Reject with a distinct error; the user re-exports. |
| The proposal fails checking, or publication fails before replacement | Neither graph changes nor acknowledgment persist. |
| Another writer edits dismissals between export and apply | Reload under the lock and preserve their records. |
| The service restarts between export and apply | Verification replays; nothing is persisted per export. |
| The suggestion is empty, or its finding is already dismissed or restored | Graph revision unchanged; no inference of prior application or one-use behavior. |
