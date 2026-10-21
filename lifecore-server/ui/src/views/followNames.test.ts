import assert from "node:assert/strict";
import { test } from "node:test";
import type { EntryId, Method, NodeRecord, Params } from "../contract.ts";
import type { Alias } from "../model/aliases.ts";
import { type Call, RpcError } from "../rpc.ts";
import { followNames } from "./followNames.ts";

const id = (position: string): EntryId => ({ origin: "o", position });

const record = (
  position: string,
  current: boolean,
  predecessor?: string,
): NodeRecord => ({
  kind: "node",
  id: id(position),
  ty_name: "Room",
  payload: {},
  current,
  ...(predecessor === undefined ? {} : { predecessor: id(predecessor) }),
});

const alias = (name: string, position: string, kind = "node"): Alias => ({
  name,
  id: id(position),
  kind: kind as Alias["kind"],
  ty_name: "Room",
});

// 1 ← 2 ← 3 (current); 4 ← 5 and 4 ← 6 (both current); 7 and 10 to 29
// current; 30 superseded, its lineage unreadable.
const kitchen = [
  record("1", false),
  record("2", false, "1"),
  record("3", true, "2"),
];
const lineages: Record<string, NodeRecord[]> = {
  "1": kitchen,
  "2": kitchen,
  "4": [record("4", false), record("5", true, "4"), record("6", true, "4")],
};
const records: Record<string, NodeRecord> = {
  "1": record("1", false),
  "2": record("2", false, "1"),
  "4": record("4", false),
  "7": record("7", true),
  "30": record("30", false),
};
for (let n = 10; n < 30; n += 1) {
  records[String(n)] = record(String(n), true);
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

// Answers from `records` and `lineages`; @9 cannot be read, nor a lineage
// that is not listed, and a query refuses the seeds it does not know as
// absent. Inspections take a turn of
// the event loop, so ones made together would overlap.
function fake() {
  const calls: { method: Method; params: unknown }[] = [];
  let inspecting = 0;
  let overlapping = 0;
  const call = (async <M extends Method>(method: M, params: Params<M>) => {
    calls.push({ method, params });
    if (method === "views.query") {
      const { seeds } = params as Params<"views.query">;
      const ids = seeds.flatMap((seed) =>
        seed.kind === "ids" ? seed.ids : [],
      );
      const absent = ids.filter(({ position }) => !(position in records));
      if (absent.length > 0) {
        throw new RpcError("views.query", {
          code: 4006,
          message: "refused",
          data: {
            kind: "invalid_seeds",
            refused: absent.map((seed) => ({
              id: seed,
              reason: "absent" as const,
            })),
          },
        });
      }
      return {
        view: {},
        nodes: ids.map(({ position }) => ({
          node: records[position],
          provenance: { kind: "seed" },
        })),
        edges: [],
      };
    }
    const { position } = (params as { id: EntryId }).id;
    if (position === "9") {
      throw new Error("lost");
    }
    if (method === "entries.get") {
      return { view: {}, entry: records[position] };
    }
    inspecting += 1;
    overlapping = Math.max(overlapping, inspecting);
    await settle();
    inspecting -= 1;
    if (lineages[position] === undefined) {
      throw new Error("lost");
    }
    const entry = records[position];
    return { kind: "node", entry, lineage: lineages[position] };
  }) as Call;
  const count = (method: Method) =>
    calls.filter((c) => c.method === method).length;
  return { calls, call, count, overlapping: () => overlapping };
}

test("names of superseded nodes move to a sole current descendant, read without as_of", async () => {
  const { calls, call, count, overlapping } = fake();
  const followed = await followNames(call, [
    alias("attic", "30"),
    alias("cellar", "9"),
    alias("galley", "1"),
    alias("hall", "7"),
    alias("kitchen", "1"),
    alias("scullery", "2"),
    alias("supplies", "8", "edge"),
    alias("wing", "4"),
  ]);
  assert.deepEqual(followed, {
    moves: [
      {
        from: id("1"),
        to: { id: id("3"), kind: "node", ty_name: "Room" },
        names: ["galley", "kitchen"],
      },
      {
        from: id("2"),
        to: { id: id("3"), kind: "node", ty_name: "Room" },
        names: ["scullery"],
      },
    ],
    split: [{ id: id("4"), names: ["wing"], count: 2 }],
    unread: ["attic", "cellar"],
  });
  // Every read is of the current state, and a node with two names is read
  // once. Edges and current nodes are not inspected, a tree is inspected
  // once for all its named nodes, and one inspection at a time.
  assert.ok(calls.every(({ params }) => !("as_of" in (params as object))));
  assert.equal(count("entries.get"), 6);
  assert.deepEqual(
    calls
      .filter(({ method }) => method === "entries.inspect")
      .map(({ params }) => (params as { id: EntryId }).id.position),
    ["30", "1", "4"],
  );
  assert.equal(overlapping(), 1);
});

test("many names are read with one query, and a refused node is unread", async () => {
  // 20 current nodes beside the superseded @1, then the same with @9, which
  // the query refuses as absent.
  const many = [
    alias("galley", "1"),
    ...Array.from({ length: 20 }, (_, i) => alias(`room_${i}`, String(10 + i))),
  ];
  const moved = {
    moves: [
      {
        from: id("1"),
        to: { id: id("3"), kind: "node", ty_name: "Room" },
        names: ["galley"],
      },
    ],
    split: [],
  };
  const once = fake();
  assert.deepEqual(await followNames(once.call, many), {
    ...moved,
    unread: [],
  });
  assert.deepEqual(
    once.calls.map(({ method }) => method),
    ["views.query", "entries.inspect"],
  );
  const query = once.calls[0]?.params as Params<"views.query">;
  assert.deepEqual(query.seeds, [
    { kind: "ids", ids: many.map(({ id }) => id) },
  ]);
  assert.ok(!("as_of" in query));
  const refused = fake();
  assert.deepEqual(
    await followNames(refused.call, [alias("cellar", "9"), ...many]),
    { ...moved, unread: ["cellar"] },
  );
  assert.equal(refused.count("views.query"), 2);
  assert.equal(refused.count("entries.get"), 0);
  assert.equal(refused.count("entries.inspect"), 1);
});
