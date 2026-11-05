# RPC methods and errors

Status: provisional reference for the envelope of
[INTERFACES §3](INTERFACES.md#3-shared-rpc-envelope). The UI stages reshape
these names and shapes as the screens need. `ui/src/contract.ts` declares the
exact shapes, guarded by fixtures recorded from Rust
([CLIENT §2](CLIENT.md#2-contract-and-fixtures)).

## 1. Conventions

Unknown or repeated request members are rejected (`-32600`), as are
unknown parameters (`-32602`). Parameters never accept `null`: omit an optional
parameter instead, as for optional proposal fields. Results may carry `null`
for absent optional values, such as `view.head` or `analysis.inputs.evaluation_time`.
Evaluation-dependent methods take optional `evaluation_time` (a Jiff timestamp
string) or `now: true`, not both; the host captures `now` once and the result's
`analysis.inputs` reports the instant used.

A `proposal` parameter is decoded on its own, exactly like a change file, so a
malformed one fails with `malformed_proposal` (2001) and a `data.decode.path`
inside it, such as `entries[0].payload.n`; failures of other parameters are
`-32602`.

Read methods take an optional `as_of`, a decimal position: they answer from
the prefix `[0, as_of)`, whose boundary the result's `view` names, and default
to the whole history. A boundary past the visible end fails with
`beyond_boundary` (4002).

Each entry record is the stored entry of
[FILES §2.5](FILES.md#25-entry-shaped-types) with `current` beside its members.
List and query results are complete. The prototype applies no result limits or
paging; they are [backlogged](PLAN.md#3-backlog).

A structurally valid proposal whose replay emits an inconsistency is still a
`check` result, with `accepted: false` and the complete report, including
advisories. `apply` refuses it with the `rejected` error, whose `data.report` is
that report; `analyzers.add` rejects the same way, with no operations.

## 2. Methods

| Method | Parameters | Result |
| --- | --- | --- |
| `database.describe` | none | `database`, `graph_revision`, current `view`, ordered `suite`, and `registered`: the sorted names of the analyzer factories this host registers, the only names `analyzers.add` can append |
| `entries.list` | optional `kind` (`node`/`edge`), `ty_name`, `currentness` (`current` default, `historical`, `all`), `text` (node text search), `as_of` | `database`, `graph_revision`, `view`, `entries` in append order |
| `entries.get` | `id` (exact `EntryId`), optional `as_of` | `view` and `entry` |
| `entries.inspect` | `id`, optional `as_of` | tagged by `kind`: a node with its `type` resolution, its `lineage` (the whole revision tree, root first), its incident `edges` of any currentness in append order (each with `direction` and the id of the `other` endpoint), and `neighbours`, the record of each other endpoint once, in append order; or an edge with `from` and `to` |
| `types.list` | optional `as_of` | `view` and `types`: one summary per name the view knows (the built-in `Type`, every name a current declaration claims, and every name a node uses), sorted by name, with its `resolution` ([SPEC §6.2](SPEC.md#62-name-resolution)): `builtin`; `declared` with the declaration and its structured schema; `missing`; `malformed` with the declaration and its structured `error`; or `ambiguous` with every claimant, `parsed` with its schema or `malformed` with its error; and node counts by currentness |
| `views.query` | `seeds` (exact node `ids`, each a node of the view, else `invalid_seeds`; or a node selection), ordered `expand` steps (`hops`, `direction`, `ty_names`, `currentness`), an `edges` filter, optional `as_of` | `view`, `nodes` with their `provenance` (`seed`, or `reached` at a hop over an edge from a node), and the matching `edges` among them |
| `views.render` | `query` (the `views.query` parameters) and `format` (`dot`/`svg`) | `format` and `text`: the queried subgraph as deterministic DOT (`digraph lifecore`, labeled with the database and boundary; nodes `n<position>`, names local to the view, with the row label on one line and cut at 60 characters, `@position`, and type, superseded ones gray; dashed blue `revision` links only between result nodes; solid edges labeled `@position <ty_name>`, historical ones gray; the empty type name reads `(unnamed)`; every node and edge has `id`, an `href` of `#inspect/<origin>/<position>`, and the uncut label as `tooltip`; payload text reads literally, with control characters shown as spaces), or that DOT rendered by Graphviz's `dot -Tsvg`, run synchronously with a 10 s deadline and a 16 MiB output cap |
| `plan_id.mint` | none | `plan_id` |
| `proposal.check` | `proposal`, evaluation inputs | `accepted` and `report`: `database`, `base_revision`, `base` view, `operations` (name to provisional entry), complete `analysis` |
| `proposal.apply` | `proposal`, evaluation inputs | new `graph_revision` and the report with actual entry IDs |
| `proposal.decode` | raw `text`, `format` (`json`/`toml`) | canonical `Proposal` |
| `proposal.encode` | `proposal`, `format` | `text` |
| `analyzers.add` | `name`, `configuration` (default `{}`), evaluation inputs | new `graph_revision`, `suite`, and the report |
| `analysis.run` | optional `as_of`, evaluation inputs | `analysis`: the selected prefix replayed with fresh analyzers under the stored ordered suite, which history does not version ([STORAGE §3](STORAGE.md#3-graph-revision)); `dismissals`: the stored records (`key`, `reason`, `note`), from the same snapshot read, whose complete key equals that of a returned observation, so no record reveals a finding first emitted past the boundary; the analysis never applies them ([INTERFACES §5](INTERFACES.md#5-analysis-lifetime-and-dismissal-persistence)) |
| `findings.dismiss` | `key` (complete finding key of the selected database, else `foreign_database`), `reason` (`acknowledged`, `false_positive`, `no_longer_relevant`), optional `note` | `dismissals`: every stored record after the edit; an existing record of the key is updated in place, and one with the same reason and note publishes nothing |
| `findings.restore` | `key` | `dismissals` after the edit and `restored`: whether a record of the key existed; nothing is published otherwise |

## 3. Errors

Application errors use codes outside the reserved JSON-RPC range and a stable
`data.kind`:

| Code | `data.kind` | Meaning |
| --- | --- | --- |
| 1001 | `state_path_empty`, `working_directory`, `directory_missing` | The state path cannot be selected, or `init`'s containing directory does not exist. |
| 1002 | `database_missing` | No database at the selected path. |
| 1003 | `database_unreadable` | The snapshot cannot be read or is invalid. |
| 1004 | `database_exists` | `init` refuses to replace an existing path. |
| 1005 | `lock_failed` | The lock file cannot be opened or locked. |
| 1006 | `busy` | Another writer held the lock throughout the retry. |
| 1007 | `publication_failed` | Failure before replacement; stored state unchanged. |
| 1008 | `publication_uncertain` | Synchronization failed after replacement; `data.outcome` is the result the call would have returned, such as the new `graph_revision` and report, tagged by `kind` (`initialized`, `applied`, `analyzer_added`, `dismissed`, or `restored`), so the caller can inspect the stored state. |
| 2001 | `malformed_proposal` | Decoding failed; `data.decode` has `format` and `message`, plus `path`, `line`, and `column` where known. A JSON `message` is serde_json's own text, which names a position; for a `proposal` parameter that position counts from the start of the member. |
| 2002 | `invalid_proposal` | Names, references, intent, or structure; `data.location` names the entry and field where known. |
| 2003 | `acknowledgment_unsupported` | `acknowledges` is present but not yet verified ([PLAN §2.2](PLAN.md#22-suggestions-and-acknowledgment)). |
| 2004 | `foreign_database` | The plan ID, or the key `findings.dismiss` was given, names another database; `data.expected` is that database and `data.actual` the selected one. |
| 2005 | `stale_base` | The plan ID names an earlier graph revision. |
| 2006 | `rejected` | Inconsistencies; `data.report` has the complete report. For `init`, whose base was never published, the report omits `base_revision`. |
| 3001 | `analyzer_failure` | Missing analyzer, invalid configuration, or execution failure. |
| 4001 | `entry_not_found` | The identity is not present in the selected view. |
| 4002 | `beyond_boundary` | `as_of` exceeds the visible end; `data` carries `requested` and `visible` as decimal strings. |
| 4003 | `cannot_dismiss_inconsistency` | `findings.dismiss` named an inconsistency key, emitted or not; `data.key` is that key. Refused before the writer lock; nothing is stored. |
| 4004 | `graphviz_unavailable` | `views.render` with `format: "svg"` could not start the `dot` program, whether missing, not executable, or refused for another reason; `data.message` is the operating system's text. |
| 4005 | `graphviz_failed` | `dot` started but failed. `data.cause`, tagged by `kind`, says why: `deadline` or `oversized`, past the deadline (`milliseconds`) or the output cap (`bytes`), and killed at once; `exited` unsuccessfully, with the platform's `status` text; `input_unread` on exit, with the write failure's `message`, or null while the write was still blocked; `output_open` `milliseconds` after the exit; `invalid_utf8`; or the operating system's `message` from `pipe_thread`, `unwaitable`, `output_unreadable`, or `stderr_unreadable`. `data.stderr` is what `dot` wrote there, trimmed and cut at 64 KiB, empty when nothing was read. |
| 4006 | `invalid_seeds` | `views.query` or `views.render` named `ids` seeds that are not nodes of the selected view; `data.refused` lists each once, in request order, as `id` and `reason`: `absent` from the view, or an `edge`. Nothing is expanded. |
| 5001 | `backend_failure` | The host task running the call ended without a response, for example by panicking. A write it was making may or may not have been published; inspect the stored state. Never a rollback. |

Invalid parameters (`-32602`, `invalid_params`) and internal errors (`-32603`,
`internal`) also carry a `data.kind`; envelope errors (`-32700`, `-32600`,
`-32601`) carry no `data`.
