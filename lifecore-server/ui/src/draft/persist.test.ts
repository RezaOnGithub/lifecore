import assert from "node:assert/strict";
import { test } from "node:test";
import type { Proposal } from "../contract.ts";
import { loadDraft, readProposal, saveDraft, storageKey } from "./persist.ts";

function memory(initial: Record<string, string> = {}) {
  const items = new Map(Object.entries(initial));
  return {
    items,
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => {
      items.set(key, value);
    },
    removeItem: (key: string) => {
      items.delete(key);
    },
  };
}

const proposal: Proposal = {
  plan_id: "lc1:a:b:c",
  entries: [
    { name: "t", kind: "node", ty_name: "Task", payload: { title: "x" } },
    {
      name: "e",
      kind: "edge",
      ty_name: "DependsOn",
      from: { added: "t" },
      to: { existing: { origin: "o", position: "1" } },
    },
  ],
};

test("a saved draft is restored under the database's own key only", () => {
  const store = memory();
  saveDraft(store, "db1", proposal);
  assert.ok(store.items.has(storageKey("db1")));
  assert.deepEqual(loadDraft(store, "db1"), proposal);
  assert.equal(loadDraft(store, "db2"), null);
  saveDraft(store, "db1", { entries: [] });
  assert.equal(store.items.has(storageKey("db1")), false);
});

test("garbage, wrong shapes, and duplicate names are ignored", () => {
  const store = memory({
    [storageKey("junk")]: "{not json",
    [storageKey("shape")]: JSON.stringify({ entries: [{ name: "x" }] }),
    [storageKey("dupe")]: JSON.stringify({
      entries: [proposal.entries[0], proposal.entries[0]],
    }),
    [storageKey("plan")]: JSON.stringify({ plan_id: 5, entries: [] }),
    [storageKey("empty")]: JSON.stringify({ entries: [] }),
  });
  assert.equal(loadDraft(store, "junk"), null);
  assert.equal(loadDraft(store, "shape"), null);
  assert.equal(loadDraft(store, "dupe"), null);
  assert.equal(loadDraft(store, "plan"), null);
  assert.deepEqual(loadDraft(store, "empty"), { entries: [] });
  assert.equal(readProposal(null), null);
  assert.equal(readProposal([]), null);
  assert.equal(
    readProposal({
      entries: [{ name: "n", kind: "node", ty_name: "T", payload: [] }],
    }),
    null,
  );
  assert.equal(
    readProposal({
      entries: [
        { name: "n", kind: "node", ty_name: "T", payload: {}, intent: "x" },
      ],
    }),
    null,
  );
  assert.equal(
    readProposal({
      entries: [
        { name: "e", kind: "edge", ty_name: "T", from: { added: 1 }, to: {} },
      ],
    }),
    null,
  );
  // A draft saved mid-rename keeps the name as typed.
  assert.deepEqual(
    readProposal({
      entries: [{ name: "1x", kind: "node", ty_name: "T", payload: {} }],
    }),
    { entries: [{ name: "1x", kind: "node", ty_name: "T", payload: {} }] },
  );
});

test("a throwing store neither loads nor breaks a save", () => {
  const broken = {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("quota");
    },
    removeItem: () => {
      throw new Error("denied");
    },
  };
  assert.equal(loadDraft(broken, "db"), null);
  assert.doesNotThrow(() => saveDraft(broken, "db", proposal));
  assert.doesNotThrow(() => saveDraft(broken, "db", { entries: [] }));
});
