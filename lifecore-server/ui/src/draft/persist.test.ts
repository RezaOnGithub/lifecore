import assert from "node:assert/strict";
import { test } from "node:test";
import type { Proposal } from "../contract.ts";
import { applicable, type DraftState, initialDraft } from "./model.ts";
import {
  loadDraft,
  readProposal,
  readStored,
  restoredState,
  type Stored,
  saveDraft,
  storageKey,
  storedChanged,
  storedOf,
} from "./persist.ts";
import { draftReducer } from "./reducer.ts";

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

const stored = (more: Partial<Stored> = {}): Stored => ({
  proposal,
  kept: {},
  uncertain: null,
  in_flight: null,
  ...more,
});

const flight = {
  plan_id: "lc1:a:b:c",
  request: JSON.stringify(proposal),
  base_revision: "b",
  base_end: "4",
};

test("a saved draft is restored under the database's own key only", () => {
  const store = memory();
  saveDraft(store, "db1", stored());
  assert.ok(store.items.has(storageKey("db1")));
  assert.deepEqual(loadDraft(store, "db1"), stored());
  assert.equal(loadDraft(store, "db2"), null);
  saveDraft(store, "db1", stored({ proposal: { entries: [] } }));
  assert.equal(store.items.has(storageKey("db1")), false);
});

test("a restored draft never has Apply on: check results are not stored", () => {
  const state = restoredState(stored());
  assert.deepEqual(state.proposal, proposal);
  assert.equal(state.outcome, null);
  assert.equal(state.uncertain, null);
  assert.equal(applicable(state), false);
  assert.equal(restoredState(null), initialDraft);
});

test("an apply in flight when the page went away comes back uncertain", () => {
  const store = memory();
  saveDraft(store, "db", stored({ in_flight: flight }));
  const state = restoredState(loadDraft(store, "db"));
  assert.deepEqual(state.uncertain, {
    ...flight,
    error: "interrupted",
    message:
      "the page was closed or reloaded while this draft was being applied",
    outcome: null,
    verdict: null,
  });
  assert.equal(state.inFlight, null);
  assert.equal(applicable(state), false);
  // The uncertain record survives a reload as such, without its verdict.
  const uncertain = draftReducer(state, {
    kind: "verdict",
    verdict: { kind: "absent" },
  });
  saveDraft(store, "db", storedOf(uncertain));
  const again = restoredState(loadDraft(store, "db"));
  assert.equal(again.uncertain?.error, "interrupted");
  assert.equal(again.uncertain?.verdict, null);
  // Only the decision clears it, and an empty draft then leaves nothing.
  const decided = draftReducer(draftReducer(again, { kind: "clear" }), {
    kind: "decide",
    present: false,
  });
  saveDraft(store, "db", storedOf(decided));
  assert.equal(store.items.has(storageKey("db")), false);
});

test("the in-flight marker is part of what a dispatch saves", () => {
  const before: DraftState = { ...initialDraft, proposal };
  const after = draftReducer(before, { kind: "apply_started", flight });
  assert.equal(storedChanged(before, after), true);
  assert.deepEqual(storedOf(after).in_flight, flight);
  const selected = draftReducer(after, { kind: "select", name: "t" });
  assert.equal(storedChanged(after, selected), false);
});

test("garbage, wrong shapes, and duplicate names are ignored", () => {
  const store = memory({
    [storageKey("junk")]: "{not json",
    [storageKey("bare")]: JSON.stringify(proposal),
    [storageKey("dupe")]: JSON.stringify(
      stored({
        proposal: { entries: [proposal.entries[0], proposal.entries[0]] },
      } as Partial<Stored>),
    ),
    [storageKey("flight")]: JSON.stringify({
      ...stored(),
      in_flight: { plan_id: 5 },
    }),
  });
  assert.equal(loadDraft(store, "junk"), null);
  assert.equal(loadDraft(store, "bare"), null);
  assert.equal(loadDraft(store, "dupe"), null);
  assert.equal(loadDraft(store, "flight"), null);
  assert.equal(readStored(null), null);
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
  assert.equal(
    readStored({ ...stored(), kept: { e: { revision: "t" } } }),
    null,
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
  assert.doesNotThrow(() => saveDraft(broken, "db", stored()));
  assert.doesNotThrow(() =>
    saveDraft(broken, "db", stored({ proposal: { entries: [] } })),
  );
});
