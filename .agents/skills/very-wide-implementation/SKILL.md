---
name: very-wide-implementation
description: Design and drive very broad software implementations as cumulative, shallow, self-contained stages across fresh-agent sessions. Use only when work spans many subsystems or stages and needs a durable implementation blueprint, explicit invariants, verification evidence, reproducible fixtures, and either reviewed or continuous hand-offs. Do not use for ordinary task breakdowns, one-off features, or deep single-stage changes.
---

# Very Wide Implementation

Create a source-grounded implementation blueprint that becomes the durable
context for a sequence of fresh agents. Invest heavily in the overarching
design, then keep each implementation stage bounded, independently verifiable,
and shallow enough to execute without recovering earlier conversation.

## Select one workflow

Record exactly one workflow near the beginning of the blueprint:

```markdown
Workflow: reviewed
```

or:

```markdown
Workflow: continuous
```

- Use `reviewed` when a human reviewer must accept the blueprint and each stage
  before work proceeds.
- Use `continuous` when fresh agents should proceed from verified implementation
  evidence without waiting for human acceptance.
- Treat reviewer gates as optional at the workflow level. When `reviewed` is
  selected, every reviewer gate is mandatory; when `continuous` is selected,
  omit reviewer gates entirely.
- Choose once from the user's direction or clear working context. Ask if the
  choice is material and cannot be inferred. Do not mix workflows stage by
  stage unless the user explicitly changes the blueprint.

## Establish the design before implementation

Read repository instructions, governing specifications, design notes, tests,
and relevant existing code before writing stages. Settle and record:

- The overarching architecture, dependency routes, and responsibility
  boundaries.
- Source-grounded behavioral contracts and acceptance criteria.
- Cross-stage invariants, failure atomicity, rollback, and persistence rules.
- Stage ordering and the exact foundation each later stage may rely on.
- Deferred behavior, explicit failures, and documented internal no-ops.
- Verification strategy, manual review surfaces, and reproducible fixture
  generators.
- The durable evidence and state each stage must hand to its successor.
- Open design decisions that must be resolved before the affected stage.

Finish and save the complete blueprint before changing implementation. Do not
push architectural decisions into a later stage merely because they are
difficult. A stage may refine local details, but it must not invent an
unrecorded cross-stage contract.

Apply this restartability test before implementation:

> Could a fresh agent implement the next eligible stage using only the
> repository and this blueprint, without recovering prior conversation or
> making an unrecorded architectural decision?

If not, improve the blueprint first.

## Use only the initial reviewer gate

Before Stage 1, include exactly one gate in the `reviewed` workflow:

```markdown
## Blueprint acceptance

- [ ] **Initial reviewer gate (reviewer only):** the overarching architecture,
      invariants, stage boundaries, verification strategy, and deferred
      behavior are accepted; Stage 1 may begin.
```

In the `continuous` workflow, omit this section. Never add an author,
implementor, readiness, progression, or hand-off gate before Stage 1.

## Preface the blueprint

State these execution rules:

- Complete stages in order and implement only the first eligible unfinished
  stage.
- Check a box only when acting in the role named by that box and after obtaining
  its required evidence.
- Preserve repository-specific test, formatting, comment, and dirty-tree rules.
- Keep each completed stage buildable and testable.
- Preserve shared-service boundaries; leaf features must not duplicate shared
  lifecycle logic.
- Keep deferred user-facing behavior explicitly unavailable. An unimplemented
  mutation must fail visibly and leave durable state unchanged.
- Treat notable signatures as interface sketches unless marked normative.
- Amend the blueprint before materially changing an accepted responsibility,
  invariant, or stage boundary.

## Use this stage format

Keep one authoritative gate set per stage. Do not duplicate summary gates near
the stage heading.

```markdown
## Stage N — Outcome-oriented name

### Description

Describe the observable outcome, why it belongs here, the boundary it
establishes, and what can be exercised afterward.

### Included scope

- List behavior implemented in this stage.

### Deferred behavior

- List scaffolding, unavailable behavior, internal no-ops, and later work.

### Types

| Type | Responsibility | Ownership/lifetime |
| ---- | -------------- | ------------------ |
| `TypeName` | Protected concept or state | Owning module and lifetime |

### Invariants

- State enforceable conditions and identify their owning modules.

### Notable members

| Member | Purpose |
| ------ | ------- |
| `Type::member(...)` | Responsibility and important behavior |

### Implementation checklist

- [ ] Implement coherent work packages through the established boundaries.
- [ ] Keep deferred paths explicitly unavailable or documented no-ops.
- [ ] Add focused tests in the repository-approved location.

### Verification

#### Automated checks

- [ ] Run focused tests and applicable format, lint, and broader checks.

#### Manual walkthrough

- [ ] Exercise the successful workflow.
- [ ] Exercise representative failure and no-change paths.

### Hand-off record

- Implemented outcome:
- Verification evidence:
- Files and interfaces materially changed:
- Blueprint amendments affecting later stages:
- Known limitations or deferred work:
- State or fixture needed by the next stage:

### Review record

- Reviewer feedback requiring implementation work:
- Agreed deferrals:

### Stage gates

- [ ] **Implementor gate (current-stage implementor only):** required behavior
      and verification are complete, and the hand-off record is current.
- [ ] **Reviewer gate (reviewer only; reviewed workflow only):** the stage is
      accepted, with feedback incorporated or explicitly deferred.
- [ ] **Hand-off received gate (next-stage implementor only):** I inspected the
      completed stage, its evidence, the current working tree, and known
      deviations; no unresolved issue prevents Stage N+1 from beginning.
```

Omit the reviewer line in the `continuous` workflow. Omit the hand-off line from
the final stage unless the blueprint names a distinct final receiver. Omit the
review record in the `continuous` workflow. Adapt checklist items and
verification commands to the stage instead of copying irrelevant boilerplate.

## Enforce gate ownership

- The current-stage implementor checks implementation tasks, verification
  tasks, and the implementor gate only.
- The current-stage implementor must not check the reviewer gate or its own
  hand-off received gate.
- The reviewer checks only reviewer-owned boxes.
- When review requires implementation changes, leave the reviewer gate
  unchecked and record the feedback. A current-stage implementor reopens the
  implementor gate, addresses the feedback, reruns affected verification, and
  refreshes the hand-off record before checking it again.
- The next-stage implementor checks the preceding stage's hand-off received
  gate after inspecting the repository and evidence, before beginning new
  implementation.
- Treat hand-off receipt as continuity confirmation, not retrospective code
  approval.
- If receipt exposes a blocker or a mismatch with the blueprint, leave the
  hand-off gate unchecked and report the defect.

## Execute a stage from fresh context

For every implementation iteration:

1. Read repository instructions, the governing specification, the entire
   blueprint, and current working-tree state.
2. Locate the earliest incomplete stage. In the `reviewed` workflow, a stage
   with recorded reviewer feedback and an unchecked reviewer gate remains
   incomplete even when its implementor gate was previously checked. Never jump
   to a later stage.
3. For Stage 1, confirm the initial reviewer gate when using the `reviewed`
   workflow.
4. For later stages, confirm the predecessor's implementor gate and, in the
   `reviewed` workflow, reviewer gate. Inspect its hand-off record and evidence.
5. If the predecessor's hand-off received gate is unchecked, check it only
   after completing that inspection. If it is already checked, treat the
   current stage as previously received and resume it.
6. If returning to recorded review feedback, reopen the implementor gate before
   changing implementation. Otherwise, implement only the current stage. Reuse
   shared staging, validation, rollback, and output paths rather than creating
   stage-local substitutes.
7. Run the required checks and walkthroughs, update the hand-off record, and
   check only implementor-owned boxes supported by evidence.
8. Stop after the implementor gate. In the `reviewed` workflow, wait for review;
   in the `continuous` workflow, leave progression to the next fresh agent.

## Stop on blueprint defects

Stop execution when source reality contradicts the blueprint, a required
cross-stage decision is missing, or the stage cannot satisfy an invariant
without changing accepted architecture. Record the evidence and propose a
blueprint amendment. Do not silently reinterpret the contract or both invent
and approve a new direction.

## Preserve reproducible review state

Add named stage-local fixture generators when accepted prior-stage state is too
small for useful verification. Allow multiple generators per stage.

Keep each generator:

- Narrowly focused on representative behavior for its stage.
- Self-contained and easy to run and inspect.
- Built only from base-project behavior and the current or accepted prior
  stages.
- Independent of behavior assigned to a later stage.
- A review aid rather than a production abstraction.

Prefer generation through the available implementation over preconstructed
state. Record the exact recipe and produced artifact in the hand-off record.
Retain reproducible artifacts and recipes by default. Prompt before removing an
artifact only when neither base-project behavior nor a recorded generator can
recreate it. Freeze accepted generator recipes as historical evidence; add a
new named recipe for later needs instead of silently rewriting an accepted one.

## Calibrate detail

- Use **Types** for meaningful state and responsibility boundaries.
- Use **Invariants** for properties later stages may safely rely on.
- Use **Notable members** for narrow interfaces between modules.
- Use **Implementation checklist** for coherent work packages.
- Use **Verification** for observable evidence, including byte comparisons when
  no-change behavior matters.
- Include focused review questions only in the `reviewed` workflow and only for
  decisions requiring human judgment.
- Add **Resulting architecture** only when a stage establishes a dependency
  route that later work must not bypass.

## Check blueprint quality

Before saving, confirm:

- The workflow is explicit and its gate set is consistent.
- The overarching design is settled deeply enough for shallow execution stages.
- Every stage is cumulative, bounded, and independently verifiable.
- Every non-final stage produces a sufficient hand-off record for fresh context.
- Types and notable members support the declared outcome.
- Invariants have verification evidence or a clear enforcement owner.
- Deferred work cannot accidentally appear successful.
- Reproducible fixture recipes are named and retained.
- The final stages cover integration, determinism, failure atomicity, and all
  governing acceptance criteria.
- The restartability test passes for every stage.
