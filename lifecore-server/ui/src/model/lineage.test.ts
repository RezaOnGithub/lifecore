import assert from "node:assert/strict";
import { test } from "node:test";
import type { NodeEntry } from "../contract.ts";
import { lineageRows } from "./lineage.ts";

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
