import assert from "node:assert/strict";
import { test } from "node:test";
import type { EntryId, Method, NodeRecord, Params } from "../contract.ts";
import type { Alias } from "../model/aliases.ts";
import type { Call } from "../rpc.ts";
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

// 1 ← 2 ← 3 (current); 4 ← 5 and 4 ← 6 (both current); 7 current.
const lineages: Record<string, NodeRecord[]> = {
  "1": [record("1", false), record("2", false, "1"), record("3", true, "2")],
  "4": [record("4", false), record("5", true, "4"), record("6", true, "4")],
};
const records: Record<string, NodeRecord> = {
  "1": record("1", false),
  "4": record("4", false),
  "7": record("7", true),
};

test("names of superseded nodes move to a sole current descendant, read without as_of", async () => {
  const calls: { method: Method; params: unknown }[] = [];
  const call = (async <M extends Method>(method: M, params: Params<M>) => {
    calls.push({ method, params });
    const { position } = (params as { id: EntryId }).id;
    if (position === "9") {
      throw new Error("lost");
    }
    if (method === "entries.get") {
      return { view: {}, entry: records[position] };
    }
    const entry = records[position];
    return { kind: "node", entry, lineage: lineages[position] };
  }) as Call;
  const followed = await followNames(call, [
    alias("cellar", "9"),
    alias("galley", "1"),
    alias("hall", "7"),
    alias("kitchen", "1"),
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
    ],
    split: [{ id: id("4"), names: ["wing"], count: 2 }],
    unread: ["cellar"],
  });
  // Every read is of the current state; edges and current nodes are not
  // inspected, and a node with two names is read once.
  assert.ok(calls.every(({ params }) => !("as_of" in (params as object))));
  assert.deepEqual(
    calls
      .filter(({ method }) => method === "entries.inspect")
      .map(({ params }) => (params as { id: EntryId }).id.position)
      .sort(),
    ["1", "4"],
  );
  assert.equal(
    calls.filter(({ method }) => method === "entries.get").length,
    4,
  );
});
