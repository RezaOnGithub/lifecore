import assert from "node:assert/strict";
import { test } from "node:test";
import type { EntryRecord, ProposalEntry } from "../contract.ts";
import { isUnset } from "./model.ts";
import {
  appendOperations,
  canonicalJson,
  defaultName,
  endpointWarnings,
  findApplied,
  forwardReferences,
  incomplete,
  keptWarning,
  pathOperation,
  referenceIssue,
  removeOperations,
  reviseSuccessor,
  uniqueName,
  withoutApplied,
} from "./operations.ts";

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

test("removing a predecessor leaves it unset with its intent, not a root", () => {
  const removal = removeOperations([task, revision, edge], ["task_1"]);
  assert.deepEqual(removal.affected, ["task_2"]);
  assert.deepEqual(removal.edges, []);
  const kept = removal.entries[0];
  assert.ok(kept?.kind === "node" && kept.predecessor !== undefined);
  assert.equal(isUnset(kept.predecessor), true);
  assert.equal(kept.intent, "revise");
  assert.deepEqual(incomplete(removal.entries), {
    operation: "task_2",
    message: "node task_2 has no predecessor picked yet",
  });
  const edges = removeOperations([task, revision, edge], ["task_2"]);
  assert.deepEqual(edges.affected, ["dependson_1"]);
  assert.deepEqual(edges.edges, ["dependson_1"]);
  const [, left] = edges.entries;
  assert.ok(left?.kind === "edge");
  assert.equal(isUnset(left.from), true);
  // Removing both ends of a reference together leaves nothing to re-pick.
  assert.deepEqual(removeOperations([task, revision], ["task_1", "task_2"]), {
    entries: [],
    affected: [],
    edges: [],
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
  assert.deepEqual(forwardReferences([task, revision, edge]), []);
  assert.deepEqual(forwardReferences([revision, task, edge]), [
    "task_2 → task_1",
  ]);
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
  const own = entries[1];
  assert.ok(own?.kind === "edge");
  assert.deepEqual(own.from, { added: "task_2" });
});

test("an imported reference follows its own batch, duplicates included", () => {
  const imported: ProposalEntry[] = [
    { ...task, name: "a" },
    { ...task, name: "a", payload: { title: "second" } },
    { ...edge, name: "e", from: { added: "a" }, to: { existing: id("1") } },
  ];
  const { entries, renamed } = appendOperations([], imported);
  assert.deepEqual(renamed, [["a", "a_2"]]);
  assert.deepEqual(
    entries.map((e) => e.name),
    ["a", "a_2", "e"],
  );
  // `a` in the edge means the nearest earlier `a`, as the server would read
  // the file in order.
  const e = entries[2];
  assert.ok(e?.kind === "edge");
  assert.deepEqual(e.from, { added: "a_2" });
});

test("successors inside the draft: chains of revisions, branches warned", () => {
  const other = { existing: id("9") };
  const reviseOther: ProposalEntry = {
    name: "other_2",
    kind: "node",
    ty_name: "Task",
    predecessor: other,
    intent: "revise",
    payload: {},
  };
  const reviseAgain: ProposalEntry = {
    ...reviseOther,
    name: "other_3",
    predecessor: { added: "other_2" },
  };
  const entries = [reviseOther, reviseAgain, task];
  assert.equal(reviseSuccessor(entries, 2, other), "other_3");
  assert.equal(reviseSuccessor(entries, 1, other), "other_2");
  assert.equal(reviseSuccessor(entries, 0, other), null);
  const branch: ProposalEntry = { ...reviseOther, intent: "branch" };
  assert.equal(reviseSuccessor([branch, task], 1, other), null);
  // An edge to a node the draft supersedes: before or after it lands.
  const toOther: ProposalEntry = {
    name: "link",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_1" },
    to: other,
  };
  assert.deepEqual(endpointWarnings([task, toOther, reviseOther], 1), [
    "the to node is revised later in this draft by other_2; this edge will be historical",
  ]);
  assert.deepEqual(endpointWarnings([branch, task, toOther], 2), [
    "the to node is branched earlier in this draft by other_2; this edge would be historical when added",
  ]);
  assert.deepEqual(endpointWarnings([task, toOther], 1), []);
});

test("a kept edge warns once its revision stops revising its node", () => {
  const kept = { revision: "task_2", predecessor: id("5"), edge: id("6") };
  const current: ProposalEntry = {
    ...revision,
    predecessor: { existing: id("5") },
  };
  assert.equal(keptWarning([current], kept), null);
  assert.equal(
    keptWarning([{ ...current, predecessor: { existing: id("7") } }], kept),
    "kept from @5, which task_2 no longer revises",
  );
  const { predecessor: _p, intent: _i, ...root } = current;
  assert.equal(
    keptWarning([root], kept),
    "kept from @5, which task_2 no longer revises",
  );
  assert.equal(
    keptWarning([], kept),
    "kept for task_2, which is no longer in the draft",
  );
  assert.equal(keptWarning([current], undefined), null);
});

test("canonical JSON ignores key order", () => {
  assert.equal(
    canonicalJson({ b: 1, a: [{ d: 2, c: 3 }] }),
    canonicalJson({ a: [{ c: 3, d: 2 }], b: 1 }),
  );
  assert.notEqual(canonicalJson({ a: "1" }), canonicalJson({ a: 1 }));
});

test("an applied request leaves the other operations, with references made existing", () => {
  const extra: ProposalEntry = {
    name: "late",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_1" },
    to: { existing: id("3") },
  };
  const changed: ProposalEntry = { ...task, payload: { title: "edited" } };
  const sent = [task];
  const { entries, removed } = withoutApplied([task, extra], sent, [
    { name: "task_1", entry: id("4") },
  ]);
  assert.deepEqual([...removed], ["task_1"]);
  assert.deepEqual(entries, [{ ...extra, from: { existing: id("4") } }]);
  // Same name, other content: not what was sent, so it stays.
  assert.deepEqual(withoutApplied([changed], sent, null).entries, [changed]);
  // Without the ids, a reference to an applied operation must be re-picked.
  const unknown = withoutApplied([task, extra], sent, null).entries[0];
  assert.ok(unknown?.kind === "edge");
  assert.equal(isUnset(unknown.from), true);
});

test("an applied request is recognized at the positions it would have taken", () => {
  const record = (position: string, entry: object) =>
    ({ id: { origin: "x", position }, current: true, ...entry }) as EntryRecord;
  const sent: ProposalEntry[] = [
    { ...task, payload: { b: "2", a: "1" } },
    {
      name: "e",
      kind: "edge",
      ty_name: "DependsOn",
      from: { added: "task_1" },
      to: { existing: id("1") },
    },
  ];
  const records = [
    record("4", { kind: "node", ty_name: "Task", payload: { a: "1", b: "2" } }),
    record("5", {
      kind: "edge",
      ty_name: "DependsOn",
      from: { origin: "x", position: "4" },
      to: id("1"),
    }),
  ];
  assert.deepEqual(findApplied(sent, "4", records), [
    { name: "task_1", entry: { origin: "x", position: "4" } },
    { name: "e", entry: { origin: "x", position: "5" } },
  ]);
  assert.equal(findApplied(sent, "3", records), null);
  assert.equal(findApplied(sent, "4", records.slice(0, 1)), null);
  const other = [
    record("4", { kind: "node", ty_name: "Task", payload: { a: "1" } }),
    records[1] as EntryRecord,
  ];
  assert.equal(findApplied(sent, "4", other), null);
});

test("a decode path names the operation it points into", () => {
  assert.equal(pathOperation("entries[1].payload", ["a", "b"]), "b");
  assert.equal(pathOperation("entries[5]", ["a"]), null);
  assert.equal(pathOperation("plan_id", ["a"]), null);
  assert.equal(pathOperation(undefined, ["a"]), null);
});
