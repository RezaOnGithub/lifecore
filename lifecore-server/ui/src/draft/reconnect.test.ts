import assert from "node:assert/strict";
import { test } from "node:test";
import type { IncidentEdge, NodeRecord, ProposalEntry } from "../contract.ts";
import {
  keepsEdge,
  keptBy,
  keptEdgeName,
  otherEnd,
  reconnectable,
  reconnection,
  stillRevises,
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
    other: other.id,
  };
}

const revision: ProposalEntry = {
  name: "task_1",
  kind: "node",
  ty_name: "Task",
  predecessor: { existing: id("1") },
  intent: "revise",
  payload: {},
};
const outgoing = incident("5", "outgoing", node("2"));
const incoming = incident("6", "incoming", node("3"));
const historical = incident("7", "outgoing", node("4", false), false);
const loop = incident("8", "outgoing", node("1"));

test("a kept edge keeps its type and other end, with the operation at this end", () => {
  assert.deepEqual(reconnection([revision], "task_1", outgoing), {
    name: "task_1_keep_5",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_1" },
    to: { existing: id("2") },
  });
  assert.deepEqual(reconnection([revision], "task_1", incoming), {
    name: "task_1_keep_6",
    kind: "edge",
    ty_name: "DependsOn",
    from: { existing: id("3") },
    to: { added: "task_1" },
  });
  assert.equal(keptEdgeName("x", historical), "x_keep_7");
});

test("a self-loop is kept on the revision at both ends, once", () => {
  assert.deepEqual(reconnection([revision], "task_1", loop), {
    name: "task_1_keep_8",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_1" },
    to: { added: "task_1" },
  });
  const twice = { ...loop, direction: "incoming" as const };
  assert.deepEqual(
    reconnectable([loop, twice]).map((e) => e.edge.id.position),
    ["8"],
  );
});

test("only current edges can be kept", () => {
  assert.deepEqual(
    reconnectable([outgoing, historical, incoming]).map(
      (e) => e.edge.id.position,
    ),
    ["5", "6"],
  );
});

test("an other end revised earlier in the draft is kept on its successor", () => {
  const otherRevision: ProposalEntry = {
    ...revision,
    name: "other_1",
    predecessor: { existing: id("2") },
  };
  const entries = [otherRevision, revision];
  assert.deepEqual(otherEnd(entries, 1, "task_1", outgoing), {
    added: "other_1",
  });
  // A later revision of the other end does not count: the edge lands first.
  assert.deepEqual(otherEnd([revision, otherRevision], 0, "task_1", outgoing), {
    existing: id("2"),
  });
  const kept = reconnection(entries, "task_1", outgoing);
  assert.ok(kept.kind === "edge");
  assert.deepEqual(kept.to, { added: "other_1" });
  // Recognized either way: on the successor, or on the original node.
  assert.equal(keepsEdge(entries, kept, "task_1", outgoing), true);
  assert.equal(
    keepsEdge(
      entries,
      { ...kept, to: { existing: id("2") } },
      "task_1",
      outgoing,
    ),
    true,
  );
});

test("kept edges are recognized structurally, whatever their names", () => {
  const kept = reconnection([revision], "task_1", incoming);
  assert.ok(kept.kind === "edge");
  const renamed = { ...kept, name: "link" };
  const entries: ProposalEntry[] = [revision, renamed];
  assert.equal(keepsEdge(entries, renamed, "task_1", incoming), true);
  assert.equal(keepsEdge(entries, renamed, "task_1", outgoing), false);
  assert.equal(keepsEdge(entries, renamed, "task_2", incoming), false);
  assert.equal(
    keepsEdge(entries, { ...renamed, ty_name: "Mentions" }, "task_1", incoming),
    false,
  );
  const flipped: ProposalEntry = {
    ...renamed,
    from: { added: "task_1" },
    to: { existing: id("3") },
  };
  assert.equal(keepsEdge(entries, flipped, "task_1", incoming), false);
  assert.equal(keptBy(entries, "task_1", incoming), renamed);
  assert.equal(keptBy(entries, "task_1", outgoing), null);
});

test("an operation still revises a node only through that exact reference", () => {
  assert.equal(stillRevises(revision, id("1")), true);
  assert.equal(stillRevises(revision, id("2")), false);
  assert.equal(stillRevises(undefined, id("1")), false);
  assert.equal(
    stillRevises({ ...revision, predecessor: { added: "x" } }, id("1")),
    false,
  );
});
