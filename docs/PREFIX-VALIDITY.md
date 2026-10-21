# Transaction granularity and prefix validity

Status: design rationale. [SPEC §2](SPEC.md#2-history-entries-and-transactions)
and [§7](SPEC.md#7-mandatory-validation) define the selected contract: atomic
plans and validation at every prefix. This document records the alternatives
and the costs of that choice.

## 1. Validation and atomic persistence are separate choices

| Model                              | Full validation                                    | Persistence             | Status                      |
| ---------------------------------- | -------------------------------------------------- | ----------------------- | --------------------------- |
| Single-entry transactions          | Every entry prefix                                 | One entry at a time     | Not selected                |
| Batches with prefix validation     | Every entry prefix                                 | Entire batch or nothing | Selected                    |
| Batches with final-view validation | Only the final view, plus structural append checks | Entire batch or nothing | Excluded from this contract |

The first two permit the same entry sequences when evaluated with the same
analyzers, configuration, and explicit inputs. They differ in what is saved if a
later append fails. Prefix-valid batches expose every entry boundary
for historical inspection, even if those entries were persisted together.

Final-view validation permits additional sequences with invalid intermediate
states. That is a semantic change, not just a storage optimization.

## 2. Why final-view validation was rejected

The earlier draft allowed a transaction to append an invalid revision `B` and
then append a valid successor `C`. At the transaction boundary only `C` was
current, so `B` could remain in history without ever passing payload validation.
Similarly, an edge with an impermissible label could evade type checking if a
later replacement made it historical before the final check.

Checking every newly introduced object separately would require selecting a
schema context for each object and deciding which intermediate changes count.
Every-prefix validation answers those questions directly: each entry must be
valid in the view immediately following its append, and predecessor-type
permission is checked in the preceding view.

This gives every stored revision an inspectable view in which it was current and
validated. A later repair cannot legitimize an invalid earlier append.

## 3. Costs for graph operations and analyzer design

Some apparently reasonable invariants admit no valid single-entry construction
sequence. Suppose an analyzer blocks any current Task without a current outgoing
`Supports` edge:

- Appending the Task first fails because the edge is absent.
- Appending the edge first fails because the Task does not yet exist.
- Revising a connected Task fails because replacement makes its old edges
  historical before new edges can be appended.

Such a rule must allow an explicit incomplete state, remain advisory, or be
reformulated to admit the intended construction sequence. Permission to use an
edge label is different from requiring that an edge already exist; ordinary
payload validation and allowed-label checks do not create this construction
problem.

Splits and connection redistribution likewise proceed through valid partial
states. Analyzer authors must consider those states. Atomic persistence with
prefix validation would not remove this requirement.

## 4. Schema evolution uses field policies

`encouraged` and `deprecated` are intentional tools for gradual migration, not
merely presentation annotations. They let the type checker direct users toward a
new schema while keeping every prefix valid:

```text
Introduce: undeclared -> encouraged -> mandatory
Retire:    declared   -> deprecated -> undeclared
```

Data revisions occur between the declaration changes. Present values always
remain typechecked. Encouraged warns about missing data; deprecated warns about
data that should be removed or replaced. Optional accepts absence without that
pressure. [SPEC §10.3](SPEC.md#103-gradual-schema-migration) gives examples
and the role of additional-field policies.

This incurs extra declaration and node revisions, and node revisions require
explicit reconnection where relationships should remain current. It provides
visible migration progress without admitting invalid history. It does not make
every possible schema change constructible; other constraints must also permit
the migration path.

## 5. Failure handling and cost

A plan stages the entire sequence, validates every prefix, and persists none of
it if any check fails. Atomic publication does not repair the construction
problem in section 3.

Validating after every append may cost more than checking one final graph. The
prototype should favor the simpler invariant; caching or incremental checks must
preserve the result of full prefix validation.

Prefix validity concerns structural and contextual type rules. Analyzer
acceptance additionally depends on the configured suite and explicit evaluation
inputs used for a check. Introducing a new analyzer or advancing evaluation time
does not imply that historical commits satisfied that different evaluation.

The decision can be revisited if a concrete essential workflow has no acceptable
valid-prefix formulation. Any proposal to allow invalid intermediate states must
explicitly replace this contract and settle historical inspection, schema
context, and transition-checking semantics.
