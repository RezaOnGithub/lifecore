import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProposalEntry, ProposalReport } from "../contract.ts";
import {
  appendOperations,
  applicable,
  type DraftState,
  defaultName,
  draftReducer,
  incomplete,
  initialDraft,
  isUnset,
  proposalOf,
  referenceIssue,
  removeOperation,
  requestOf,
  uniqueName,
  validName,
} from "./model.ts";

const id = (position: string) => ({ origin: "o", position });

const task: ProposalEntry = {
  name: "task_1",
  kind: "node",
  ty_name: "Task",
  payload: { title: "a" },
};
const revision: ProposalEntry = {
  name: "task_2",
  kind: "node",
  ty_name: "Task",
  predecessor: { added: "task_1" },
  intent: "revise",
  payload: { title: "b" },
};
const edge: ProposalEntry = {
  name: "dependson_1",
  kind: "edge",
  ty_name: "DependsOn",
  from: { added: "task_2" },
  to: { existing: id("3") },
};

function state(entries: ProposalEntry[], more: Partial<DraftState> = {}) {
  return { ...initialDraft, proposal: { entries }, ...more };
}

const report: ProposalReport = {
  database: "db",
  base_revision: "r",
  base: { database: "db", end_exclusive: "4", head: null },
  operations: [],
  analysis: {
    view: { database: "db", end_exclusive: "4", head: null },
    inputs: { evaluation_time: null },
    implementations: [],
    observations: [],
  },
};

test("names validate against the proposal grammar", () => {
  for (const ok of ["a", "_x", "task_1", "A-b_9"]) {
    assert.equal(validName(ok), true, ok);
  }
  for (const bad of ["", "1a", "-a", "a b", "a.b", "ä"]) {
    assert.equal(validName(bad), false, bad);
  }
});

test("default names lower the type name and take the first free number", () => {
  assert.equal(defaultName([], "Task"), "task_1");
  assert.equal(defaultName([task], "Task"), "task_2");
  assert.equal(
    defaultName([task, { ...task, name: "task_3" }], "Task"),
    "task_2",
  );
  assert.equal(defaultName([], "Depends On"), "depends_on_1");
  assert.equal(defaultName([], "3D"), "_3d_1");
  assert.equal(defaultName([], ""), "op_1");
  assert.equal(uniqueName([task], "task_1"), "task_1_2");
  assert.equal(uniqueName([task], "other"), "other");
});

test("adding without selection keeps the selected operation", () => {
  const added = draftReducer(state([task], { selected: "task_1" }), {
    kind: "add",
    entry: edge,
    select: false,
  });
  assert.equal(added.selected, "task_1");
});

test("adding uniquifies the name, inserts after a named operation, and selects", () => {
  const added = draftReducer(state([task, edge]), {
    kind: "add",
    entry: { ...task, name: "task_1" },
    after: "task_1",
  });
  assert.deepEqual(
    added.proposal.entries.map((e) => e.name),
    ["task_1", "task_1_2", "dependson_1"],
  );
  assert.equal(added.selected, "task_1_2");
  const appended = draftReducer(state([task]), {
    kind: "add",
    entry: edge,
    after: "nobody",
  });
  assert.deepEqual(
    appended.proposal.entries.map((e) => e.name),
    ["task_1", "dependson_1"],
  );
});

test("replacing follows a valid rename through references and refuses a clash", () => {
  const renamed = draftReducer(state([task, revision, edge]), {
    kind: "replace",
    name: "task_2",
    entry: { ...revision, name: "fixed" },
  });
  const names = renamed.proposal.entries.map((e) => e.name);
  assert.deepEqual(names, ["task_1", "fixed", "dependson_1"]);
  const e = renamed.proposal.entries[2];
  assert.ok(e?.kind === "edge");
  assert.deepEqual(e.from, { added: "fixed" });
  const clash = draftReducer(state([task, revision]), {
    kind: "replace",
    name: "task_2",
    entry: { ...revision, name: "task_1", payload: { title: "c" } },
  });
  assert.deepEqual(
    clash.proposal.entries.map((e) => e.name),
    ["task_1", "task_2"],
  );
  const kept = clash.proposal.entries[1];
  assert.ok(kept?.kind === "node");
  assert.deepEqual(kept.payload, { title: "c" });
  assert.match(clash.notice ?? "", /already used/);
  // A name outside the grammar is kept mid-edit, references and all; the
  // editor flags it and the server judges it.
  const typing = draftReducer(state([task, revision]), {
    kind: "rename",
    name: "task_1",
    to: "",
  });
  assert.deepEqual(
    typing.proposal.entries.map((e) => e.name),
    ["", "task_2"],
  );
  assert.equal(typing.notice, null);
  const followed = typing.proposal.entries[1];
  assert.ok(followed?.kind === "node");
  assert.deepEqual(followed.predecessor, { added: "" });
});

test("move stays in bounds and duplicate lands right after the original", () => {
  const down = draftReducer(state([task, revision, edge]), {
    kind: "move",
    name: "task_1",
    by: 1,
  });
  assert.deepEqual(
    down.proposal.entries.map((e) => e.name),
    ["task_2", "task_1", "dependson_1"],
  );
  const top = state([task, revision]);
  assert.equal(
    draftReducer(top, { kind: "move", name: "task_1", by: -1 }),
    top,
  );
  const copied = draftReducer(state([task, revision]), {
    kind: "duplicate",
    name: "task_1",
  });
  assert.deepEqual(
    copied.proposal.entries.map((e) => e.name),
    ["task_1", "task_1_2", "task_2"],
  );
  const copy = copied.proposal.entries[1];
  assert.ok(copy?.kind === "node");
  assert.deepEqual(copy.payload, { title: "a" });
  assert.equal(copied.selected, "task_1_2");
});

test("removing clears references to the operation and counts them", () => {
  const { entries, cleared } = removeOperation(
    [task, revision, edge],
    "task_2",
  );
  assert.equal(cleared, 1);
  const [, kept] = entries;
  assert.ok(kept?.kind === "edge");
  assert.equal(isUnset(kept.from), true);
  const root = removeOperation([task, revision], "task_1");
  assert.equal(root.cleared, 1);
  assert.deepEqual(root.entries, [
    { name: "task_2", kind: "node", ty_name: "Task", payload: { title: "b" } },
  ]);
  const removed = draftReducer(
    state([task, revision, edge], {
      selected: "task_2",
      picking: { operation: "task_2", role: "predecessor" },
    }),
    { kind: "remove", name: "task_2" },
  );
  assert.equal(removed.selected, null);
  assert.equal(removed.picking, null);
  assert.equal(removed.notice, "removed task_2 and cleared 1 reference to it");
  assert.deepEqual(incomplete(removed.proposal.entries), {
    operation: "dependson_1",
    message: "edge dependson_1 has no from node yet",
  });
});

test("reference issues name missing and later targets", () => {
  assert.equal(referenceIssue([task, revision], 1, { added: "task_1" }), null);
  assert.equal(
    referenceIssue([revision, task], 0, { added: "task_1" }),
    "later",
  );
  assert.equal(referenceIssue([task], 0, { added: "ghost" }), "missing");
  assert.equal(referenceIssue([task], 0, { existing: id("1") }), null);
});

test("appending an import renames collisions and follows them inside the batch", () => {
  const imported: ProposalEntry[] = [
    { ...task, name: "task_1", payload: { title: "imported" } },
    { ...edge, name: "link", from: { added: "task_1" } },
  ];
  const { entries, renamed } = appendOperations([task, edge], imported);
  assert.deepEqual(renamed, [["task_1", "task_1_2"]]);
  assert.deepEqual(
    entries.map((e) => e.name),
    ["task_1", "dependson_1", "task_1_2", "link"],
  );
  const link = entries[3];
  assert.ok(link?.kind === "edge");
  assert.deepEqual(link.from, { added: "task_1_2" });
  // The draft's own edge still points at the draft's task.
  const own = entries[1];
  assert.ok(own?.kind === "edge");
  assert.deepEqual(own.from, { added: "task_2" });
});

test("an imported reference follows its own batch, not a later rename", () => {
  // The batch's `task_1_2` is renamed too, so `task_1` → `task_1_2` must not
  // be renamed again when the batch's own `task_1_2` moves to `task_1_2_2`.
  const imported: ProposalEntry[] = [
    { ...task, name: "task_1" },
    { ...task, name: "task_1_2" },
    {
      ...edge,
      name: "a",
      from: { added: "task_1" },
      to: { added: "task_1_2" },
    },
    {
      ...edge,
      name: "b",
      from: { added: "outside" },
      to: { existing: id("1") },
    },
  ];
  const { entries, renamed } = appendOperations(
    [task, { ...task, name: "task_1_2" }],
    imported,
  );
  assert.deepEqual(renamed, [
    ["task_1", "task_1_3"],
    ["task_1_2", "task_1_2_2"],
  ]);
  const a = entries[4];
  assert.ok(a?.kind === "edge");
  assert.deepEqual(a.from, { added: "task_1_3" });
  assert.deepEqual(a.to, { added: "task_1_2_2" });
  const b = entries[5];
  assert.ok(b?.kind === "edge");
  assert.deepEqual(b.from, { added: "outside" });
  assert.equal(new Set(entries.map((e) => e.name)).size, entries.length);
});

test("import replace keeps the file's plan ID, else the draft's", () => {
  const withPlan = draftReducer(
    state([task], { stale: { expected: "a", actual: "b" } }),
    {
      kind: "import",
      mode: "replace",
      proposal: { plan_id: "lc1:x", entries: [revision] },
    },
  );
  assert.deepEqual(withPlan.proposal, {
    plan_id: "lc1:x",
    entries: [revision],
  });
  assert.equal(withPlan.stale, null);
  const without = draftReducer(
    { ...state([task]), proposal: { plan_id: "lc1:mine", entries: [task] } },
    { kind: "import", mode: "replace", proposal: { entries: [revision] } },
  );
  assert.equal(without.proposal.plan_id, "lc1:mine");
});

test("the plan ID lands only on a non-empty draft without one", () => {
  const empty = draftReducer(initialDraft, {
    kind: "set_plan_id",
    plan_id: "p",
  });
  assert.equal(empty.proposal.plan_id, undefined);
  const set = draftReducer(state([task]), {
    kind: "set_plan_id",
    plan_id: "p",
  });
  assert.equal(set.proposal.plan_id, "p");
  const kept = draftReducer(set, { kind: "set_plan_id", plan_id: "q" });
  assert.equal(kept.proposal.plan_id, "p");
  const replanned = draftReducer(
    { ...set, stale: { expected: "a", actual: "b" } },
    { kind: "replan", plan_id: "q" },
  );
  assert.equal(replanned.proposal.plan_id, "q");
  assert.equal(replanned.stale, null);
  assert.equal(replanned.outcome, null);
});

test("proposalOf strips client state and omits an absent plan ID", () => {
  assert.deepEqual(proposalOf(state([task], { selected: "task_1" })), {
    entries: [task],
  });
  assert.equal(requestOf(state([task])), JSON.stringify({ entries: [task] }));
  assert.deepEqual(
    Object.keys(
      proposalOf({
        ...state([task]),
        proposal: { plan_id: "p", entries: [task] },
      }),
    ),
    ["plan_id", "entries"],
  );
});

test("Apply is gated on an accepted check of identical content", () => {
  const checked = draftReducer(state([task]), {
    kind: "checked",
    result: { accepted: true, report },
    request: requestOf(state([task])),
    evaluation: { kind: "none" },
  });
  assert.equal(applicable(checked), true);
  const edited = draftReducer(checked, {
    kind: "replace",
    name: "task_1",
    entry: { ...task, payload: { title: "changed" } },
  });
  assert.equal(applicable(edited), false);
  assert.equal(edited.outcome, checked.outcome);
  const reordered = draftReducer(
    draftReducer(checked, { kind: "add", entry: edge }),
    { kind: "remove", name: "dependson_1" },
  );
  assert.equal(applicable(reordered), true);
  const rejected = draftReducer(checked, {
    kind: "checked",
    result: { accepted: false, report },
    request: requestOf(checked),
    evaluation: { kind: "none" },
  });
  assert.equal(applicable(rejected), false);
  assert.equal(
    applicable(
      draftReducer(checked, { kind: "stale", expected: "a", actual: "b" }),
    ),
    false,
  );
  assert.equal(applicable({ ...checked, pending: "check" }), false);
});

test("a successful apply clears the draft and keeps the ids until dismissed", () => {
  const applied = draftReducer(
    state([task], {
      selected: "task_1",
      stale: { expected: "a", actual: "b" },
    }),
    {
      kind: "applied",
      result: {
        graph_revision: "r2",
        report: { ...report, operations: [{ name: "task_1", entry: id("4") }] },
      },
    },
  );
  assert.deepEqual(applied.proposal, { entries: [] });
  assert.equal(applied.selected, null);
  assert.equal(applied.stale, null);
  assert.deepEqual(applied.applied, {
    graph_revision: "r2",
    operations: [{ name: "task_1", entry: id("4") }],
  });
  const next = draftReducer(applied, { kind: "add", entry: task });
  assert.notEqual(next.applied, null);
  assert.equal(draftReducer(next, { kind: "dismiss_applied" }).applied, null);
});

test("a rejected apply keeps the report and content problems clear on edit", () => {
  const rejected = draftReducer(state([task]), {
    kind: "rejected",
    report,
    request: requestOf(state([task])),
    evaluation: { kind: "now" },
  });
  assert.equal(rejected.outcome?.source, "apply");
  assert.equal(rejected.outcome?.accepted, false);
  const invalid = draftReducer(rejected, {
    kind: "problem",
    problem: {
      kind: "invalid_proposal",
      message: "bad",
      location: { operation: 0, name: "task_1" },
    },
  });
  const edited = draftReducer(invalid, { kind: "move", name: "task_1", by: 1 });
  assert.equal(edited.problem?.kind, "invalid_proposal");
  const moved = draftReducer(
    draftReducer(invalid, { kind: "add", entry: edge }),
    { kind: "move", name: "task_1", by: 1 },
  );
  assert.equal(moved.problem, null);
  const uncertain = draftReducer(invalid, {
    kind: "problem",
    problem: { kind: "uncertain", message: "lost", outcome: null },
  });
  assert.equal(
    draftReducer(uncertain, { kind: "add", entry: edge }).problem?.kind,
    "uncertain",
  );
});

test("resolving a pick sets the role's reference and clears the pick", () => {
  const fresh: ProposalEntry = {
    name: "edge_1",
    kind: "edge",
    ty_name: "",
    from: { existing: id("1") },
    to: { existing: { origin: "", position: "" } },
  };
  const picked = draftReducer(
    state([fresh, task], { picking: { operation: "edge_1", role: "to" } }),
    { kind: "resolve_pick", id: id("7") },
  );
  const e = picked.proposal.entries[0];
  assert.ok(e?.kind === "edge");
  assert.deepEqual(e.to, { existing: id("7") });
  assert.equal(picked.picking, null);
  assert.equal(picked.selected, "edge_1");
  const predecessor = draftReducer(
    state([task], { picking: { operation: "task_1", role: "predecessor" } }),
    { kind: "resolve_pick", id: id("2") },
  );
  const n = predecessor.proposal.entries[0];
  assert.ok(n?.kind === "node");
  assert.deepEqual(n.predecessor, { existing: id("2") });
  assert.equal(n.intent, "revise");
  const mismatch = draftReducer(
    state([task], { picking: { operation: "task_1", role: "to" } }),
    { kind: "resolve_pick", id: id("2") },
  );
  assert.equal(mismatch.picking, null);
  assert.deepEqual(mismatch.proposal.entries, [task]);
});
