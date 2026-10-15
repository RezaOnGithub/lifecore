import assert from "node:assert/strict";
import { test } from "node:test";
import type { IncidentEdge, NodeRecord, ProposalEntry } from "../contract.ts";
import {
  keepsEdge,
  keptBy,
  keptEdgeName,
  reconnectable,
  reconnection,
  reconnections,
} from "./reconnect.ts";

const id = (position: string) => ({ origin: "o", position });
const node = (position: string, current = true): NodeRecord => ({
  kind: "node",
  id: id(position),
  ty_name: "Task",
  payload: { title: position },
  current,
});

function incident(
  position: string,
  direction: "outgoing" | "incoming",
  other: NodeRecord,
  current = true,
): IncidentEdge {
  const self = id("1");
  return {
    edge: {
      kind: "edge",
      id: id(position),
      ty_name: "DependsOn",
      from: direction === "outgoing" ? self : other.id,
      to: direction === "outgoing" ? other.id : self,
      current,
    },
    direction,
    other,
  };
}

const outgoing = incident("5", "outgoing", node("2"));
const incoming = incident("6", "incoming", node("3"));
const historical = incident("7", "outgoing", node("4", false), false);

test("a kept edge keeps its type and other end, with the operation at this end", () => {
  assert.deepEqual(reconnection("task_1", outgoing), {
    name: "task_1_keep_5",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_1" },
    to: { existing: id("2") },
  });
  assert.deepEqual(reconnection("task_1", incoming), {
    name: "task_1_keep_6",
    kind: "edge",
    ty_name: "DependsOn",
    from: { existing: id("3") },
    to: { added: "task_1" },
  });
  assert.equal(keptEdgeName("x", historical), "x_keep_7");
});

test("revise keeps every current edge by default, branch none", () => {
  const edges = [outgoing, historical, incoming];
  assert.deepEqual(
    reconnectable(edges).map((e) => e.edge.id.position),
    ["5", "6"],
  );
  assert.deepEqual(
    reconnections("task_1", edges, "revise").map((e) => e.name),
    ["task_1_keep_5", "task_1_keep_6"],
  );
  assert.deepEqual(reconnections("task_1", edges, "branch"), []);
});

test("kept edges are recognized structurally, whatever their names", () => {
  const kept = reconnection("task_1", incoming);
  assert.ok(kept.kind === "edge");
  const renamed = { ...kept, name: "link" };
  assert.equal(keepsEdge(renamed, "task_1", incoming), true);
  assert.equal(keepsEdge(renamed, "task_1", outgoing), false);
  assert.equal(keepsEdge(renamed, "task_2", incoming), false);
  assert.equal(
    keepsEdge({ ...renamed, ty_name: "Mentions" }, "task_1", incoming),
    false,
  );
  const flipped: ProposalEntry = {
    ...renamed,
    from: { added: "task_1" },
    to: { existing: id("3") },
  };
  assert.equal(keepsEdge(flipped, "task_1", incoming), false);
  const entries: ProposalEntry[] = [
    { name: "task_1", kind: "node", ty_name: "Task", payload: {} },
    renamed,
  ];
  assert.equal(keptBy(entries, "task_1", incoming), renamed);
  assert.equal(keptBy(entries, "task_1", outgoing), null);
});
