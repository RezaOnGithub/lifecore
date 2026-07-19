# A Productivity-Focused Analyzer For LifeCore

This document proposes a `Productivity` analyzer for personal task and project
management. The analyzer is implemented by `lifecore-prod` and operates on the
typed, versioned graph model already provided by LifeCore. It does not add a
second task database or treat LifeCore as an event-sourcing framework.

The proposal introduces three node types, one edge name, schema-bootstrap
suggestions, and diagnostics derived from the fresh graph in each historical
analysis window.

## Terminology And Scope

- A **current node** or **current edge** is fresh in the bounded `EventContext`
  being analyzed.
- A **timebox** is a non-negative integer number of minutes.
- A TODO or Project is **unfinished** when its `status` is not `Done`.
- A node **contributes to** another node when a current `Contributes_To` edge
  points from the contributor to the recipient.
- A Cycler is **covered** when a current TODO whose status is `Done` currently
  contributes to it.

Deadlines, calendar scheduling, recurrence intervals, automatic status
transitions, and mutation suggestions other than type-declaration bootstrap are
outside the scope of this analyzer.

## Graph Schema

The Rust crate should export typed bodies named `Todo`, `Project`, and `Cycler`,
a shared `CompletionStatus` enum, `ProductivityConfig`, and
`ProductivityFactory`. Their persisted type names are `TODO`, `Project`, and
`Cycler`, respectively. `CompletionStatus` has exactly the Rust variants `Done`,
`InProgress`, and `Dormant`, which map to the textual values specified below.

Status strings and graph type names are case-sensitive. Although `status` is
semantically a completion field, its persisted field name remains `status`.

### TODO

A `TODO` is a specific actionable item with these fields:

| Field                          | Payload type | Policy    |
| ------------------------------ | ------------ | --------- |
| `description`                  | `Text`       | Mandatory |
| `status`                       | `Text`       | Mandatory |
| `estimated_completion_minutes` | `Number`     | Optional  |

The allowed statuses are `Done`, `In-Progress`, and `Dormant`. There is no
separate ready or pending state: every unfinished TODO is explicitly either in
progress or dormant.

`TODO` uses `NodeTypeClass::Spike`, permits outgoing `Contributes_To` edges, and
permits no incoming edge names.

### Project

A `Project` groups work under a description and a required timebox:

| Field                          | Payload type | Policy    |
| ------------------------------ | ------------ | --------- |
| `description`                  | `Text`       | Mandatory |
| `status`                       | `Text`       | Mandatory |
| `estimated_completion_minutes` | `Number`     | Mandatory |

Project uses the same statuses as TODO: `Done`, `In-Progress`, and `Dormant`. In
particular, `TODO` is not a valid Project status. Consequently, Projects have
the same deliberate lack of a ready-but-not-started state as TODO items.

`Project` uses `NodeTypeClass::Regular` and permits `Contributes_To` as both an
incoming and outgoing edge name.

### Cycler

A `Cycler` represents an area that should periodically receive completed work:

| Field         | Payload type | Policy    |
| ------------- | ------------ | --------- |
| `description` | `Text`       | Mandatory |

`Cycler` uses `NodeTypeClass::Hub`, permits incoming `Contributes_To` edges, and
permits no outgoing edge names.

All three canonical declarations allow `Tombstone` as their sole additional
override type. Field order follows the tables above. Declaration comparison for
bootstrap uses full `NodeType` equality, including class, edge-name vectors,
additional overrides, field order, payload types, and field policies.

### Contributes_To

The analyzer accepts exactly these endpoint pairs for a `Contributes_To` edge:

| Contributor | Recipient |
| ----------- | --------- |
| `TODO`      | `Cycler`  |
| `TODO`      | `Project` |
| `Project`   | `Project` |

TypeCheck can verify that an endpoint declaration permits the edge name, but it
cannot constrain the type at the opposite endpoint. `Productivity` therefore
owns validation of these pairs. A `Contributes_To` edge with any other endpoint
pair is an Inconsistency.

Projects may be nested, but the current Project-to-Project contribution graph
must be acyclic. A self-loop is a cycle. Cycles are Inconsistencies because
recursive time accounting would otherwise be undefined.

## Type Declaration Bootstrap

On initialization, the analyzer examines the active `Node_Type` declarations for
`TODO`, `Project`, and `Cycler`. For each declaration that is absent or does not
structurally equal the canonical declaration above, it emits a separate
Suggestion containing one `SuggestedAction::AddNode`:

- `ty_name` is `Node_Type`;
- `payload` is the canonical declaration;
- `parent` is absent when the declaration is missing; and
- `parent` points to the active declaration when replacing a mismatch.

Separate suggestions let the CLI apply and analyze each graph addition through
its normal transaction workflow. They are advisory rather than Inconsistencies.
Replacing a declaration can still be rejected by TypeCheck if existing fresh
nodes do not satisfy the replacement schema; the productivity analyzer must not
bypass that protection.

## Configuration

`ProductivityFactory` accepts either an empty config string or a strict JSON
object:

```json
{
  "oldest_cyclers": 3
}
```

An empty string is equivalent to the object above. `oldest_cyclers` is a
non-negative integer and is the number of uncovered Cyclers selected for
warnings. Zero disables only the Cycler warning. Unknown fields, malformed JSON,
fractional values, and negative values reject analyzer construction.

To report this cleanly, LifeCore should add an error variant equivalent to:

```rust
InvalidAnalyzerConfig {
    analyzer: String,
    message: String,
}
```

The error message should identify the analyzer and explain the invalid config
without embedding unstable parser debug output.

The crate should expose a serializable `ProductivityConfig` and use strict
unknown-field rejection so programmatic and string configuration have one
schema.

## Analysis Model

`AnalysisRunner` invokes analyzers once for every valid historical prefix.
`Productivity::analyze_inner` must interpret only the bounded window it is
given. It must not use `EventContext::universe()` to make decisions about that
window.

Every rule below operates on current nodes and current edges in that window.
Node kinds are selected using the stored `NodeExt.ty_name`, not the potentially
historically shifted resolved `NodeExt.ty`. Pointed contexts passed to LifeCore
queries must be rebased into the window when necessary.

Traversal and diagnostic construction must be deterministic. Collections used
for traversal, cycle detection, relevance, and output should be ordered by
absolute event identity before conversion to public messages.

### Semantic Inconsistencies

The analyzer emits Inconsistencies for:

- a TODO or Project with an unknown or incorrectly-cased status;
- a TODO or Project with a negative estimate;
- a productivity node whose required fields cannot be interpreted;
- a `Contributes_To` edge whose endpoints are not one of the allowed pairs; and
- a cycle in the current Project contribution graph.

TypeCheck may also diagnose malformed field shapes. The productivity analyzer
must nevertheless handle them without panicking when TypeCheck is absent or
ordered differently. A malformed value is excluded from calculations that depend
on it after its Inconsistency has been recorded.

Cycle reporting should be deterministic and finite. Emit one Inconsistency for
each cyclic strongly connected component, locate it at the newest internal
`Contributes_To` edge, and include every Project and internal edge in sorted
order in the inner diagnostic data. Do not calculate recursive budget warnings
for a Project whose reachable subgraph contains a reported cycle.

### Urgent Non-Time-Boxed TODOs

An unfinished TODO is urgent when:

- it has no `estimated_completion_minutes` field; and
- it directly contributes to at least one Project.

Emit one Warning per urgent TODO, not one per edge or recipient Project. Locate
the warning at the TODO and include the TODO, all direct recipient Projects, and
their connecting edges in relevance. A non-time-boxed TODO contributing only to
a Cycler is not urgent under this rule. A Done TODO never produces this warning.

### Project Timebox Overage

For each Project, recursively follow incoming `Contributes_To` edges through
contributing Projects and collect reachable TODOs. The consumed estimate is the
sum of `estimated_completion_minutes` from the unique current TODO nodes in that
transitive subgraph.

- Count a TODO once even when multiple contribution paths reach it.
- Include TODO estimates regardless of TODO status, including `Done`.
- Exclude TODOs without estimates; their urgency is handled separately when they
  directly contribute to a Project.
- Do not add a nested Project's timebox to its parent's consumed estimate. Every
  Project timebox is a budget checked independently against its own recursively
  collected TODO work.

Emit one Warning for a Project only when consumed minutes are strictly greater
than `estimated_completion_minutes`. Equality is within budget. Locate the
warning at the Project and include the target Project, traversed Projects and
edges, and counted TODOs in deterministic relevance order.

### Dormant Projects

Emit one Warning for every current Project whose status is `Dormant`. Locate it
at that Project and include the Project in relevance. The warning applies even
when the Project is within its timebox or has no contributors.

### Concurrent In-Progress Work

TODO and Project concurrency are independent rules:

- if more than one current TODO is `In-Progress`, emit one aggregate Warning;
- if more than one current Project is `In-Progress`, emit a separate aggregate
  Warning.

Do not emit one warning per excess node. For each category, locate the warning
at the newest involved node and include all involved nodes in event order in
relevance.

### Oldest Uncovered Cyclers

A current Cycler is covered only when a current TODO with status `Done` has a
current `Contributes_To` edge to it. A historical edge, an edge made stale by an
endpoint replacement, or a Done successor without a recreated edge does not
cover the Cycler.

Sort uncovered Cyclers by the event identity of their current fresh version,
oldest first, and select at most `oldest_cyclers`. Replacing a Cycler therefore
resets its age. Emit one Warning per selected Cycler, located at and relevant to
that Cycler. Equal event ages are impossible, but absolute event identity is the
deterministic tie-breaker wherever ordering helpers require one.

## Diagnostic Conversion

The crate should define a structured, hashable inner message enum rather than
constructing text inside graph traversal. Each variant maps to exactly one
`MessageType`. Conversion to public diagnostics sets:

- `system_name` to `Productivity`;
- `system_config` to the original config string supplied to the factory;
- per-node and per-edge locations as described above; and
- stable messages containing type names, event identities where helpful, and
  numeric budget totals for overage warnings.

Warnings are non-fatal under the CLI transaction rules. Every semantic
Inconsistency is fatal. Suggested type declarations remain non-fatal and use the
existing suggestion application workflow.

The runner intentionally preserves diagnostics from historical windows.
`Productivity` should not suppress a valid historical diagnostic merely because
a later window resolves it; relevance and the CLI barklist remain the
presentation-layer mechanism for managing non-fatal historical messages.

## CLI And Workspace Integration

`lifecore-prod` should implement and export `ProductivityFactory` for arbitrary
`LifeCore` implementations. The CLI should depend on `lifecore-prod` and
register this factory alongside the other compiled-in factories, allowing a
database to add `Productivity` through the existing `analyzer add` workflow.

The analyzer is not added to the default suite during `init`. A user opts into
the model explicitly by adding its analyzer/config pair and then applying its
schema suggestions.

## Acceptance Tests

The eventual implementation should cover at least these cases:

1. Typed-body conversions round-trip all fields and produce the exact canonical
   `NodeType` declarations, classes, and edge permissions.
2. TODO and Project accept the same three statuses; `TODO`, different casing,
   and arbitrary strings are rejected as statuses.
3. Estimates accept zero and positive integers, reject negative values, and
   remain optional only for TODO.
4. Empty and valid JSON configs construct the analyzer; zero disables Cycler
   warnings; malformed, fractional, negative, and unknown-field configs fail.
5. Initialization suggests every absent declaration and correctly parents a
   replacement for a mismatched active declaration.
6. All three permitted `Contributes_To` endpoint pairs pass, while reversed or
   unrelated pairs produce Inconsistencies.
7. Project self-loops and multi-Project cycles are detected once per cyclic
   component with deterministic locations.
8. A direct unfinished TODO without an estimate warns once across multiple
   recipient Projects; Done, estimated, and Cycler-only TODOs do not warn.
9. Direct, nested, branching, and duplicate-path Project graphs calculate unique
   TODO estimates correctly. Equality does not warn and strict overage does.
10. Nested Project timeboxes are not double-counted, and completed TODO work
    remains part of the consumed estimate.
11. Every Dormant Project warns independently.
12. TODO and Project In-Progress limits warn independently and produce one
    aggregate diagnostic per exceeded category.
13. Cycler selection respects the configured limit, current-version age, Done
    status, stale endpoints, and recreated edges.
14. Historical-prefix tests prove later replacements and status changes do not
    alter earlier analysis results.
15. Runner and CLI integration tests prove Warnings remain non-fatal,
    Inconsistencies block commits, suggestions use normal staging, and output is
    deterministic across repeated runs.

## Consistency Notes

Removing `TODO` from the Project status vocabulary introduces no contradiction
with the analyzer rules. It makes Project status exactly consistent with TODO
status and leaves the Dormant and In-Progress warnings well-defined.

The resulting model deliberately has no pending state for either node type. This
limits expressiveness—an unfinished item must be `In-Progress` or `Dormant`—but
it is a shared, explicit domain choice rather than an accidental schema
inconsistency.
