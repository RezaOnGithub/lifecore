import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  IncidentEdge,
  NodeRecord,
  ProposalEntry,
  ProposalReport,
} from "../contract.ts";
import {
  applicable,
  type DraftState,
  initialDraft,
  isUnset,
  requestOf,
  type Uncertain,
} from "./model.ts";
import { type DraftAction, draftReducer } from "./reducer.ts";

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

const node = (position: string, ty_name = "Task"): NodeRecord => ({
  kind: "node",
  id: id(position),
  ty_name,
  payload: {},
  current: true,
});

function incident(
  position: string,
  direction: "outgoing" | "incoming",
  self: NodeRecord,
  other: NodeRecord,
): IncidentEdge {
  return {
    edge: {
      kind: "edge",
      id: id(position),
      ty_name: "DependsOn",
      from: direction === "outgoing" ? self.id : other.id,
      to: direction === "outgoing" ? other.id : self.id,
      current: true,
    },
    direction,
    other,
  };
}

const names = (s: DraftState) => s.proposal.entries.map((e) => e.name);

test("adding uniquifies the name, inserts after a named operation, and selects", () => {
  const added = draftReducer(state([task, edge]), {
    kind: "add",
    entry: task,
    after: "task_1",
  });
  assert.deepEqual(names(added), ["task_1", "task_1_2", "dependson_1"]);
  assert.equal(added.selected, "task_1_2");
  assert.equal(
    draftReducer(state([task], { selected: "task_1" }), {
      kind: "add",
      entry: edge,
      select: false,
    }).selected,
    "task_1",
  );
});

test("a rename follows references, kept provenance, and checked names; a clash is refused", () => {
  const checked = draftReducer(
    state([task, revision, edge], {
      kept: {
        dependson_1: {
          revision: "task_2",
          predecessor: id("1"),
          edge: id("2"),
        },
      },
    }),
    {
      kind: "checked",
      result: { accepted: true, report },
      request: requestOf(state([task, revision, edge])),
      evaluation: { kind: "none" },
    },
  );
  const renamed = draftReducer(checked, {
    kind: "rename",
    name: "task_2",
    to: "fixed",
  });
  assert.deepEqual(names(renamed), ["task_1", "fixed", "dependson_1"]);
  const e = renamed.proposal.entries[2];
  assert.ok(e?.kind === "edge");
  assert.deepEqual(e.from, { added: "fixed" });
  assert.equal(renamed.kept.dependson_1?.revision, "fixed");
  assert.deepEqual(renamed.outcome?.names, ["task_1", "fixed", "dependson_1"]);
  const clash = draftReducer(state([task, revision]), {
    kind: "replace",
    name: "task_2",
    entry: { ...revision, name: "task_1", payload: { title: "c" } },
  });
  assert.deepEqual(names(clash), ["task_1", "task_2"]);
  const kept = clash.proposal.entries[1];
  assert.ok(kept?.kind === "node");
  assert.deepEqual(kept.payload, { title: "c" });
  assert.match(clash.notices[0]?.text ?? "", /already used/);
});

test("a move that puts a reference before its target says so", () => {
  const down = draftReducer(state([task, revision, edge]), {
    kind: "move",
    name: "task_1",
    by: 1,
  });
  assert.deepEqual(names(down), ["task_2", "task_1", "dependson_1"]);
  assert.match(down.notices[0]?.text ?? "", /task_2 → task_1/);
  const harmless = draftReducer(state([task, edge, revision]), {
    kind: "move",
    name: "dependson_1",
    by: 1,
  });
  assert.equal(harmless.notices.length, 0);
  const top = state([task, revision]);
  assert.equal(
    draftReducer(top, { kind: "move", name: "task_1", by: -1 }),
    top,
  );
  const copied = draftReducer(state([task, revision]), {
    kind: "duplicate",
    name: "task_1",
  });
  assert.deepEqual(names(copied), ["task_1", "task_1_2", "task_2"]);
});

test("removing names the affected operations and offers to remove orphaned edges", () => {
  const removed = draftReducer(
    state([task, revision, edge], {
      selected: "task_2",
      picking: { operation: "task_2", role: "predecessor" },
    }),
    { kind: "remove", names: ["task_2"] },
  );
  assert.equal(removed.selected, null);
  assert.equal(removed.picking, null);
  const [notice] = removed.notices;
  assert.equal(
    notice?.text,
    "removed task_2; dependson_1 referred to it and must be picked again",
  );
  assert.deepEqual(notice?.offer, {
    label: "Remove that edge too",
    remove: ["dependson_1"],
  });
  // Notices stay until dismissed, through later edits.
  const later = draftReducer(removed, {
    kind: "add",
    entry: { ...task, name: "x" },
  });
  assert.equal(later.notices.length, 1);
  assert.equal(
    draftReducer(later, { kind: "dismiss_notice", id: notice?.id ?? 0 }).notices
      .length,
    0,
  );
  const root = draftReducer(state([task, revision]), {
    kind: "remove",
    names: ["task_1"],
  });
  const left = root.proposal.entries[0];
  assert.ok(left?.kind === "node" && left.predecessor !== undefined);
  assert.equal(isUnset(left.predecessor), true);
  assert.equal(root.notices[0]?.offer, null);
});

test("import replace renames duplicates and never keeps the old plan ID", () => {
  const old = {
    ...state([task]),
    proposal: { plan_id: "lc1:mine", entries: [task] },
    stale: { expected: "a", actual: "b" },
  };
  const replaced = draftReducer(old, {
    kind: "import",
    mode: "replace",
    proposal: { entries: [task, { ...task, payload: { title: "b" } }] },
  });
  assert.deepEqual(names(replaced), ["task_1", "task_1_2"]);
  assert.equal(replaced.proposal.plan_id, undefined);
  assert.equal(replaced.stale, null);
  assert.match(replaced.notices[0]?.text ?? "", /task_1 → task_1_2/);
  const withPlan = draftReducer(old, {
    kind: "import",
    mode: "replace",
    proposal: { plan_id: "lc1:x", entries: [revision] },
  });
  assert.equal(withPlan.proposal.plan_id, "lc1:x");
  const appended = draftReducer(old, {
    kind: "import",
    mode: "append",
    proposal: { plan_id: "lc1:x", entries: [task] },
  });
  assert.deepEqual(names(appended), ["task_1", "task_1_2"]);
  assert.equal(appended.proposal.plan_id, "lc1:mine");
  const acknowledging = draftReducer(old, {
    kind: "import",
    mode: "replace",
    proposal: {
      entries: [revision],
      acknowledges: {
        database: "db",
        analyzer: { name: "A", version: "1", configuration: "" },
        kind: "suggestion",
        analyzer_certificate: "c",
      },
    },
  });
  assert.deepEqual(acknowledging.proposal, old.proposal);
  assert.match(acknowledging.notices[0]?.text ?? "", /not supported/);
});

test("nothing edits the draft while an apply is in flight", () => {
  const flight = {
    plan_id: "p",
    request: requestOf(state([task])),
    base_revision: "r",
    base_end: "4",
  };
  const applying = draftReducer(state([task]), {
    kind: "apply_started",
    flight,
  });
  assert.equal(applying.pending, "apply");
  const edits: DraftAction[] = [
    { kind: "add", entry: edge },
    { kind: "replace", name: "task_1", entry: { ...task, payload: {} } },
    { kind: "rename", name: "task_1", to: "x" },
    { kind: "duplicate", name: "task_1" },
    { kind: "remove", names: ["task_1"] },
    { kind: "clear" },
    { kind: "import", mode: "replace", proposal: { entries: [] } },
    { kind: "replan", plan_id: "q" },
    { kind: "pick", picking: { operation: "task_1", role: "predecessor" } },
  ];
  for (const action of edits) {
    assert.equal(draftReducer(applying, action), applying, action.kind);
  }
  // A late answer for kept edges is noted, not added.
  const late = draftReducer(applying, {
    kind: "keep",
    operation: "task_1",
    predecessor: id("1"),
    edges: [incident("4", "outgoing", node("1"), node("2"))],
  });
  assert.equal(late.proposal, applying.proposal);
  assert.match(late.notices[0]?.text ?? "", /arrived while applying/);
});

test("applied removes only the operations of the request, by name and content", () => {
  const sent = state([task, revision]);
  const request = requestOf(sent);
  // An operation the request did not carry, as if added from elsewhere.
  const extra: ProposalEntry = {
    name: "extra",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_2" },
    to: { existing: id("3") },
  };
  const applied = draftReducer(
    {
      ...state([task, revision, extra]),
      proposal: { plan_id: "p", entries: [task, revision, extra] },
    },
    {
      kind: "applied",
      request,
      result: {
        graph_revision: "r2",
        report: {
          ...report,
          operations: [
            { name: "task_1", entry: id("4") },
            { name: "task_2", entry: id("5") },
          ],
        },
      },
    },
  );
  assert.deepEqual(applied.proposal, {
    entries: [{ ...extra, from: { existing: id("5") } }],
  });
  assert.equal(applied.applied?.graph_revision, "r2");
  assert.match(applied.notices[0]?.text ?? "", /kept 1 operation/);
  const all = draftReducer(sent, {
    kind: "applied",
    request,
    result: { graph_revision: "r2", report },
  });
  assert.deepEqual(all.proposal, { entries: [] });
  assert.equal(all.notices.length, 0);
});

test("an uncertain apply spends the check and waits for the user's decision", () => {
  const request = requestOf(state([task]));
  const ok = draftReducer(state([task]), {
    kind: "checked",
    result: { accepted: true, report },
    request,
    evaluation: { kind: "none" },
  });
  const uncertain: Uncertain = {
    plan_id: "p",
    request,
    base_revision: "r",
    base_end: "4",
    error: "transport",
    message: "lost",
    outcome: null,
    verdict: null,
  };
  const waiting = draftReducer(ok, { kind: "uncertain", uncertain });
  assert.equal(waiting.outcome?.spent, true);
  assert.equal(applicable(waiting), false);
  // Nothing but a decision clears it: not a replan, an edit, or a problem.
  assert.equal(
    draftReducer(waiting, { kind: "replan", plan_id: "q" }).proposal.plan_id,
    undefined,
  );
  assert.notEqual(
    draftReducer(waiting, { kind: "add", entry: edge }).uncertain,
    null,
  );
  const notApplied = draftReducer(waiting, { kind: "decide", present: false });
  assert.equal(notApplied.uncertain, null);
  assert.deepEqual(notApplied.proposal.entries, [task]);
  // Still no Apply: the check was spent.
  assert.equal(applicable(notApplied), false);
  const recheck = draftReducer(notApplied, {
    kind: "checked",
    result: { accepted: true, report },
    request,
    evaluation: { kind: "none" },
  });
  assert.equal(applicable(recheck), true);
  // Entries present: the request's operations go, with the verdict's ids.
  const seen = draftReducer(waiting, {
    kind: "verdict",
    verdict: {
      kind: "present",
      operations: [{ name: "task_1", entry: id("4") }],
    },
  });
  const present = draftReducer(seen, { kind: "decide", present: true });
  assert.deepEqual(present.proposal.entries, []);
  assert.equal(present.uncertain, null);
  assert.deepEqual(present.applied?.operations, [
    { name: "task_1", entry: id("4") },
  ]);
});

test("kept edges land only while the operation still revises the node", () => {
  const n = node("1");
  const fresh: ProposalEntry = {
    name: "task_9",
    kind: "node",
    ty_name: "Task",
    predecessor: { existing: n.id },
    intent: "revise",
    payload: {},
  };
  const edges = [
    incident("4", "outgoing", n, node("2")),
    incident("5", "incoming", n, node("3")),
  ];
  const kept = draftReducer(state([fresh, task]), {
    kind: "keep",
    operation: "task_9",
    predecessor: n.id,
    intent: "revise",
    edges,
  });
  assert.deepEqual(names(kept), [
    "task_9",
    "task_9_keep_4",
    "task_9_keep_5",
    "task_1",
  ]);
  assert.deepEqual(kept.kept.task_9_keep_4, {
    revision: "task_9",
    predecessor: n.id,
    edge: id("4"),
  });
  // A second answer adds nothing twice.
  assert.equal(
    draftReducer(kept, {
      kind: "keep",
      operation: "task_9",
      predecessor: n.id,
      edges,
    }),
    kept,
  );
  // The same name now revises another node: the answer is dropped.
  const other = draftReducer(
    state([{ ...fresh, predecessor: { existing: id("7") } }]),
    {
      kind: "keep",
      operation: "task_9",
      predecessor: n.id,
      intent: "revise",
      edges,
    },
  );
  assert.deepEqual(names(other), ["task_9"]);
  assert.match(other.notices[0]?.text ?? "", /removed or changed/);
  // Switched to branch before the answer: nothing is kept by default.
  const branched = draftReducer(state([{ ...fresh, intent: "branch" }]), {
    kind: "keep",
    operation: "task_9",
    predecessor: n.id,
    intent: "revise",
    edges,
  });
  assert.deepEqual(names(branched), ["task_9"]);
  // Gone altogether.
  const gone = draftReducer(state([task]), {
    kind: "keep",
    operation: "task_9",
    predecessor: n.id,
    edges,
  });
  assert.deepEqual(names(gone), ["task_1"]);
  assert.equal(gone.notices.length, 1);
});

test("a kept edge whose other end the draft revised earlier uses the successor", () => {
  const n = node("1");
  const o = node("2");
  const otherRevision: ProposalEntry = {
    name: "other_2",
    kind: "node",
    ty_name: "Task",
    predecessor: { existing: o.id },
    intent: "revise",
    payload: {},
  };
  const fresh: ProposalEntry = {
    name: "task_9",
    kind: "node",
    ty_name: "Task",
    predecessor: { existing: n.id },
    intent: "revise",
    payload: {},
  };
  const loop = {
    ...incident("6", "outgoing", n, n),
  };
  const kept = draftReducer(state([otherRevision, fresh]), {
    kind: "keep",
    operation: "task_9",
    predecessor: n.id,
    edges: [incident("4", "outgoing", n, o), loop],
  });
  const [, , toOther, self] = kept.proposal.entries;
  assert.ok(toOther?.kind === "edge" && self?.kind === "edge");
  assert.deepEqual(toOther.from, { added: "task_9" });
  assert.deepEqual(toOther.to, { added: "other_2" });
  assert.deepEqual(self.from, { added: "task_9" });
  assert.deepEqual(self.to, { added: "task_9" });
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

test("a rejected apply keeps its report; content problems clear on edit, others stay", () => {
  const rejected = draftReducer(state([task]), {
    kind: "rejected",
    report,
    request: requestOf(state([task])),
    evaluation: { kind: "now" },
  });
  assert.equal(rejected.outcome?.source, "apply");
  assert.equal(applicable(rejected), false);
  const invalid = draftReducer(rejected, {
    kind: "problem",
    problem: {
      kind: "invalid_proposal",
      message: "bad",
      location: { operation: 0, name: "task_1" },
    },
  });
  assert.equal(
    draftReducer(invalid, { kind: "add", entry: edge }).problem,
    null,
  );
  const busy = draftReducer(invalid, {
    kind: "problem",
    problem: { kind: "busy", message: "busy", action: "apply" },
  });
  assert.equal(
    draftReducer(busy, { kind: "add", entry: edge }).problem?.kind,
    "busy",
  );
});

test("clear empties the draft, kept provenance, notices, and an uncertain record", () => {
  const cleared = draftReducer(
    state([task], {
      kept: { x: { revision: "a", predecessor: id("1"), edge: id("2") } },
      notices: [{ id: 1, text: "n", offer: null }],
      uncertain: {
        plan_id: null,
        request: "{}",
        base_revision: null,
        base_end: "1",
        error: "transport",
        message: "",
        outcome: null,
        verdict: null,
      },
    }),
    { kind: "clear" },
  );
  assert.deepEqual(cleared.proposal, { entries: [] });
  assert.deepEqual(cleared.kept, {});
  assert.deepEqual(cleared.notices, []);
  assert.equal(cleared.uncertain, null);
});

test("a removal notice names a few operations and counts the rest", () => {
  const edges: ProposalEntry[] = ["a", "b", "c", "d", "e"].map((name) => ({
    name,
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_1" },
    to: { existing: id("3") },
  }));
  const removed = draftReducer(state([task, ...edges]), {
    kind: "remove",
    names: ["task_1"],
  });
  assert.equal(
    removed.notices[0]?.text,
    "removed task_1; a, b, c, and 2 more referred to it and must be picked again",
  );
  assert.deepEqual(removed.notices[0]?.offer, {
    label: "Remove those 5 edges too",
    remove: ["a", "b", "c", "d", "e"],
  });
});
