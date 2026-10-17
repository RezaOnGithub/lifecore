import assert from "node:assert/strict";
import { test } from "node:test";
import type { EntryId, Method, NodeRecord } from "../contract.ts";
import { type Call, RpcError } from "../rpc.ts";
import { readLabels } from "./entryLabels.ts";

interface Pending {
  method: Method;
  params: unknown;
  resolve: (value: unknown) => void;
  reject: (failure: unknown) => void;
}

// A call whose answers the test gives, in any order.
function fake() {
  const calls: Pending[] = [];
  const call = ((method: Method, params: unknown) =>
    new Promise<unknown>((resolve, reject) => {
      calls.push({ method, params, resolve, reject });
    })) as Call;
  return { calls, call };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

const id = (position: string): EntryId => ({ origin: "o", position });

function node(position: string): NodeRecord {
  return {
    kind: "node",
    id: id(position),
    ty_name: "Task",
    payload: { title: `t${position}` },
    current: true,
  } as NodeRecord;
}

test("a few labels are read entry by entry, nodes only", async () => {
  const { calls, call } = fake();
  const read = readLabels(call, [id("1"), id("2"), id("1")], "9");
  await settle();
  assert.deepEqual(
    calls.map((c) => [c.method, c.params]),
    [
      ["entries.get", { id: id("1"), as_of: "9" }],
      ["entries.get", { id: id("2"), as_of: "9" }],
    ],
  );
  calls[0]?.resolve({ entry: node("1") });
  calls[1]?.resolve({ entry: { kind: "edge", id: id("2") } });
  const { nodes, failure } = await read;
  assert.deepEqual([...nodes.keys()], ["o:1"]);
  assert.equal(failure, null);
});

// A `views.query` refusal of the seeds at these positions.
function refusal(edges: string[], absent: string[] = []): RpcError {
  return new RpcError("views.query", {
    code: 4006,
    message: "refused",
    data: {
      kind: "invalid_seeds",
      refused: [
        ...edges.map((position) => ({
          id: id(position),
          reason: "edge" as const,
        })),
        ...absent.map((position) => ({
          id: id(position),
          reason: "absent" as const,
        })),
      ],
    },
  });
}

const seeded = (c: Pending | undefined) =>
  (c?.params as { seeds: { ids: EntryId[] }[] } | undefined)?.seeds[0]?.ids;

test("many labels come from one query, dropping every refused id at once", async () => {
  const many = Array.from({ length: 25 }, (_, i) => id(String(i)));
  const { calls, call } = fake();
  const read = readLabels(call, many, "30");
  await settle();
  assert.equal(calls[0]?.method, "views.query");
  assert.equal(seeded(calls[0])?.length, 25);
  calls[0]?.reject(refusal(["3", "8"], ["24"]));
  await settle();
  assert.equal(calls[1]?.method, "views.query");
  assert.deepEqual(
    seeded(calls[1])?.map(({ position }) => position),
    many
      .map(({ position }) => position)
      .filter((position) => !["3", "8", "24"].includes(position)),
  );
  calls[1]?.resolve({ nodes: [{ node: node("4") }], edges: [] });
  const { nodes, failure } = await read;
  assert.deepEqual([...nodes.keys()], ["o:4"]);
  assert.equal(failure, null);
  assert.equal(calls.length, 2);
});

test("labels are read with one retry at most, and none when all are refused", async () => {
  const many = Array.from({ length: 25 }, (_, i) => id(String(i)));
  const again = fake();
  const read = readLabels(again.call, many, "30");
  await settle();
  again.calls[0]?.reject(refusal(["3"]));
  await settle();
  const second = refusal(["4"]);
  again.calls[1]?.reject(second);
  const { nodes, failure } = await read;
  assert.equal(nodes.size, 0);
  assert.equal(failure, second);
  assert.equal(again.calls.length, 2);
  const none = fake();
  const refused = readLabels(none.call, many, "30");
  await settle();
  none.calls[0]?.reject(
    refusal(
      many.slice(0, 20).map(({ position }) => position),
      ["20", "21", "22", "23", "24"],
    ),
  );
  const result = await refused;
  assert.equal(result.nodes.size, 0);
  assert.equal(result.failure, null);
  assert.equal(none.calls.length, 1);
});
