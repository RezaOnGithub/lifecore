import assert from "node:assert/strict";
import { test } from "node:test";
import type { EdgeEntry, NodeEntry } from "../contract.ts";
import { buildThread, preferredDirection, stripeMark } from "./thread.ts";

function node(position: string): NodeEntry {
  return {
    kind: "node",
    id: { origin: "o", position },
    ty_name: "Task",
    payload: {},
  };
}
function edge(
  position: string,
  from: string,
  to: string,
  ty_name = "DependsOn",
): EdgeEntry {
  return {
    kind: "edge",
    id: { origin: "o", position },
    ty_name,
    from: { origin: "o", position: from },
    to: { origin: "o", position: to },
  };
}
const rows = (thread: ReturnType<typeof buildThread>) =>
  thread.rows.map((row) =>
    row.kind === "repeat"
      ? [row.kind, row.key, row.depth, row.under]
      : [row.kind, row.key, row.depth],
  );

test("children follow parents in append order; a diamond repeats once", () => {
  // 1 → 2, 1 → 3, 2 → 4, 3 → 4: node 4 has two parents.
  const nodes = ["1", "2", "3", "4"].map(node);
  const edges = [
    edge("10", "1", "3"),
    edge("11", "1", "2"),
    edge("12", "2", "4"),
    edge("13", "3", "4"),
  ];
  const thread = buildThread(nodes, edges, {
    ty_name: "DependsOn",
    direction: "outgoing",
  });
  assert.deepEqual(rows(thread), [
    ["node", "o:1", 0],
    ["node", "o:2", 1],
    ["node", "o:4", 2],
    ["node", "o:3", 1],
    ["repeat", "o:4", 2, "o:2"],
  ]);
  assert.deepEqual([...thread.stripes], [["o:4", 0]]);
});

test("incoming direction reverses parents and other edge types are ignored", () => {
  const nodes = ["1", "2", "3"].map(node);
  const edges = [edge("10", "2", "1"), edge("11", "3", "1", "Mentions")];
  const thread = buildThread(nodes, edges, {
    ty_name: "DependsOn",
    direction: "incoming",
  });
  assert.deepEqual(rows(thread), [
    ["node", "o:1", 0],
    ["node", "o:2", 1],
    ["node", "o:3", 0],
  ]);
  assert.equal(thread.stripes.size, 0);
});

test("a cycle is cut at the first revisit on the path", () => {
  // 1 → 2 → 3 → 2 and a rootless 5 ⇄ 6.
  const nodes = ["1", "2", "3", "5", "6"].map(node);
  const edges = [
    edge("10", "1", "2"),
    edge("11", "2", "3"),
    edge("12", "3", "2"),
    edge("13", "5", "6"),
    edge("14", "6", "5"),
  ];
  const thread = buildThread(nodes, edges, {
    ty_name: "DependsOn",
    direction: "outgoing",
  });
  assert.deepEqual(rows(thread), [
    ["node", "o:1", 0],
    ["node", "o:2", 1],
    ["node", "o:3", 2],
    ["cycle", "o:2", 3],
    ["node", "o:5", 0],
    ["node", "o:6", 1],
    ["cycle", "o:5", 2],
  ]);
  assert.deepEqual(
    [...thread.stripes],
    [
      ["o:2", 0],
      ["o:5", 1],
    ],
  );
});

test("an edge with an endpoint outside the result is ignored", () => {
  const nodes = ["1", "2"].map(node);
  const edges = [edge("10", "1", "9"), edge("11", "9", "2")];
  const thread = buildThread(nodes, edges, {
    ty_name: "DependsOn",
    direction: "outgoing",
  });
  assert.deepEqual(rows(thread), [
    ["node", "o:1", 0],
    ["node", "o:2", 0],
  ]);
  assert.equal(thread.stripes.size, 0);
});

test("duplicate edges between a pair nest the child once", () => {
  const nodes = ["1", "2"].map(node);
  const edges = [edge("10", "1", "2"), edge("11", "1", "2")];
  const thread = buildThread(nodes, edges, {
    ty_name: "DependsOn",
    direction: "outgoing",
  });
  assert.deepEqual(rows(thread), [
    ["node", "o:1", 0],
    ["node", "o:2", 1],
  ]);
  assert.equal(thread.stripes.size, 0);
});

test("without edges every node is a root in append order", () => {
  const thread = buildThread(["2", "1"].map(node), [], {
    ty_name: "",
    direction: "outgoing",
  });
  assert.deepEqual(rows(thread), [
    ["node", "o:2", 0],
    ["node", "o:1", 0],
  ]);
  assert.equal(thread.stripes.size, 0);
});

test("every edge type nests children and names the types joining them", () => {
  // Tasks 2 and 3 are part of project 1; task 3 depends on task 2 and is
  // also about it.
  const nodes = ["1", "2", "3"].map(node);
  const edges = [
    edge("10", "2", "1", "PartOf"),
    edge("11", "3", "1", "PartOf"),
    edge("12", "3", "2", "DependsOn"),
    edge("13", "3", "2", "About"),
  ];
  const incoming = buildThread(nodes, edges, {
    ty_name: null,
    direction: "incoming",
  });
  assert.deepEqual(
    incoming.rows.map((row) => [row.kind, row.key, row.depth, row.via]),
    [
      ["node", "o:1", 0, []],
      ["node", "o:2", 1, ["PartOf"]],
      ["node", "o:3", 2, ["DependsOn", "About"]],
      ["repeat", "o:3", 1, ["PartOf"]],
    ],
  );
  // One type at a time ignores the others, as before.
  const partOf = buildThread(nodes, edges, {
    ty_name: "PartOf",
    direction: "incoming",
  });
  assert.deepEqual(
    partOf.rows.map((row) => [row.key, row.depth, row.via]),
    [
      ["o:1", 0, []],
      ["o:2", 1, ["PartOf"]],
      ["o:3", 1, ["PartOf"]],
    ],
  );
});

test("the preferred direction leaves fewer roots, outgoing on a tie", () => {
  // Three tasks part of one project: read to → from, the project is the
  // only root.
  const nodes = ["1", "2", "3", "4"].map(node);
  const partOf = [
    edge("10", "2", "1", "PartOf"),
    edge("11", "3", "1", "PartOf"),
    edge("12", "4", "1", "PartOf"),
  ];
  assert.equal(preferredDirection(nodes, partOf, null), "incoming");
  assert.equal(preferredDirection(nodes, partOf, "Other"), "outgoing");
  // A chain has one root either way.
  const chain = [edge("10", "1", "2"), edge("11", "2", "3")];
  assert.equal(
    preferredDirection(nodes.slice(0, 3), chain, "DependsOn"),
    "outgoing",
  );
});

test("stripes cycle through the palette with a suffix afterwards", () => {
  assert.deepEqual(stripeMark(0), { color: 0, suffix: "" });
  assert.deepEqual(stripeMark(7), { color: 7, suffix: "" });
  assert.deepEqual(stripeMark(8), { color: 0, suffix: "2" });
  assert.deepEqual(stripeMark(17), { color: 1, suffix: "3" });
});
