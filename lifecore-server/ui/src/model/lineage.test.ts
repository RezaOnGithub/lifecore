import assert from "node:assert/strict";
import { test } from "node:test";
import type { NodeEntry } from "../contract.ts";
import { currentDescendants, lineageRows } from "./lineage.ts";

function node(position: string, predecessor?: string): NodeEntry {
  const entry: NodeEntry = {
    kind: "node",
    id: { origin: "o", position },
    ty_name: "Task",
    payload: {},
  };
  if (predecessor !== undefined) {
    entry.predecessor = { origin: "o", position: predecessor };
  }
  return entry;
}

test("a revision tree indents successors under their predecessor", () => {
  const rows = lineageRows([
    node("1"),
    node("2", "1"),
    node("3", "1"),
    node("4", "2"),
  ]);
  assert.deepEqual(
    rows.map((row) => [row.node.id.position, row.depth]),
    [
      ["1", 0],
      ["2", 1],
      ["4", 2],
      ["3", 1],
    ],
  );
});

test("an empty lineage has no rows", () => {
  assert.deepEqual(lineageRows([]), []);
});

test("a node whose predecessor is not listed is shown as a root", () => {
  const rows = lineageRows([node("5", "4"), node("6", "5")]);
  assert.deepEqual(
    rows.map((row) => [row.node.id.position, row.depth]),
    [
      ["5", 0],
      ["6", 1],
    ],
  );
});

test("current descendants follow chains and splits below the node only", () => {
  const record = (position: string, predecessor?: string) => {
    const entry = node(position, predecessor);
    return { ...entry, current: true };
  };
  // 1 ← 2 ← 4, 2 ← 5 (a split), and 1 ← 3, whose subtree is not below 2.
  const lineage = [
    { ...record("1"), current: false },
    { ...record("2", "1"), current: false },
    { ...record("3", "1"), current: false },
    record("4", "2"),
    record("5", "2"),
    record("6", "3"),
  ];
  const positions = (start: string) =>
    currentDescendants(lineage, { origin: "o", position: start }).map(
      (n) => n.id.position,
    );
  assert.deepEqual(positions("1"), ["4", "5", "6"]);
  assert.deepEqual(positions("2"), ["4", "5"]);
  assert.deepEqual(positions("3"), ["6"]);
  assert.deepEqual(positions("4"), []);
});
