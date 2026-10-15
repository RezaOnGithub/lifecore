import assert from "node:assert/strict";
import { test } from "node:test";
import type { EdgeEntry, NodeEntry } from "../contract.ts";
import { entryKey } from "./labels.ts";
import {
  countByType,
  countWords,
  directionAt,
  edgeWords,
  groupByRelation,
  plural,
  relationCells,
  relationKey,
  relationLabel,
  typeTags,
} from "./relations.ts";

const id = (position: string) => ({ origin: "o", position });
function node(position: string, ty_name = "Task"): NodeEntry {
  return { kind: "node", id: id(position), ty_name, payload: {} };
}
function edge(
  position: string,
  ty_name: string,
  from: string,
  to: string,
): EdgeEntry {
  return {
    kind: "edge",
    id: id(position),
    ty_name,
    from: id(from),
    to: id(to),
  };
}

test("relations read with an arrow and order outgoing first, then by name", () => {
  const groups = groupByRelation(
    [
      { ty_name: "DependsOn", direction: "incoming" as const, n: 1 },
      { ty_name: "PartOf", direction: "outgoing" as const, n: 2 },
      { ty_name: "DependsOn", direction: "outgoing" as const, n: 3 },
      { ty_name: "DependsOn", direction: "incoming" as const, n: 4 },
      { ty_name: "", direction: "outgoing" as const, n: 5 },
    ],
    (item) => item,
  );
  assert.deepEqual(
    groups.map((group) => [
      relationLabel(group.relation),
      group.items.map((item) => item.n),
    ]),
    [
      ["→ (unnamed)", [5]],
      ["→ DependsOn", [3]],
      ["→ PartOf", [2]],
      ["← DependsOn", [1, 4]],
    ],
  );
});

test("cells list each row's neighbours per relation in edge order", () => {
  const nodes = [node("1"), node("2"), node("3", "Project"), node("4", "Note")];
  const byKey = new Map(nodes.map((n) => [entryKey(n.id), n]));
  const rows = [nodes[0], nodes[1]].filter((n) => n !== undefined);
  const edges = [
    edge("10", "PartOf", "1", "3"),
    edge("11", "DependsOn", "2", "1"),
    edge("12", "About", "4", "1"),
    edge("13", "PartOf", "2", "3"),
    // Between two non-rows: not a cell of any row.
    edge("14", "About", "4", "3"),
    // Its other end is outside the nodes: skipped.
    edge("15", "PartOf", "1", "99"),
    // A self-loop is both outgoing and incoming.
    edge("16", "Blocks", "2", "2"),
  ];
  const { relations, cells } = relationCells(rows, edges, byKey);
  assert.deepEqual(relations.map(relationLabel), [
    "→ Blocks",
    "→ DependsOn",
    "→ PartOf",
    "← About",
    "← Blocks",
    "← DependsOn",
  ]);
  const cell = (row: string, label: string) => {
    const relation = relations.find((r) => relationLabel(r) === label);
    return relation === undefined
      ? undefined
      : cells
          .get(`o:${row}`)
          ?.get(relationKey(relation))
          ?.map(({ node, edge }) => [node.id.position, edge.id.position]);
  };
  assert.deepEqual(cell("1", "→ PartOf"), [["3", "10"]]);
  assert.deepEqual(cell("1", "← DependsOn"), [["2", "11"]]);
  assert.deepEqual(cell("1", "← About"), [["4", "12"]]);
  assert.deepEqual(cell("2", "→ DependsOn"), [["1", "11"]]);
  assert.deepEqual(cell("2", "→ Blocks"), [["2", "16"]]);
  assert.deepEqual(cell("2", "← Blocks"), [["2", "16"]]);
  assert.equal(cell("1", "→ DependsOn"), undefined);
  assert.equal(cells.has("o:4"), false);
});

test("neighbours count by type, most first, in words and as tags", () => {
  const counts = countByType([
    node("1", "Note"),
    node("2"),
    node("3"),
    node("4", "Address"),
    node("5"),
    node("6", "Story"),
  ]);
  assert.deepEqual(counts, [
    { ty_name: "Task", count: 3 },
    { ty_name: "Address", count: 1 },
    { ty_name: "Note", count: 1 },
    { ty_name: "Story", count: 1 },
  ]);
  assert.equal(countWords(counts), "3 Tasks, 1 Address, 1 Note, 1 Story");
  assert.equal(
    countWords([
      { ty_name: "Address", count: 2 },
      { ty_name: "Story", count: 4 },
    ]),
    "2 Addresses, 4 Stories",
  );
  assert.equal(plural("Day", 2), "Days");
  assert.equal(typeTags([{ ty_name: "Task", count: 460 }]), "Task");
  assert.equal(typeTags(counts.slice(0, 2)), "3 Task, 1 Address");
  assert.equal(typeTags([{ ty_name: "", count: 1 }]), "(unnamed)");
});

test("one arrow notation, read from the node in context", () => {
  const within = edge("9", "Within", "1", "2");
  assert.equal(directionAt(within, id("1")), "outgoing");
  assert.equal(directionAt(within, id("2")), "incoming");
  assert.equal(
    relationLabel({
      ty_name: "Within",
      direction: directionAt(within, id("1")),
    }),
    "→ Within",
  );
  assert.equal(
    relationLabel({
      ty_name: "Within",
      direction: directionAt(within, id("2")),
    }),
    "← Within",
  );
  assert.equal(
    edgeWords("Wash dishes", "Within", "Kitchen"),
    "Wash dishes —Within→ Kitchen",
  );
});
