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
  kept: {},
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

test("the marker and the uncertain record keep the provenance of the kept edges sent", () => {
  const kept = {
    e: {
      revision: "t",
      predecessor: { origin: "o", position: "1" },
      edge: { origin: "o", position: "2" },
    },
  };
  const sending = { ...flight, kept };
  const store = memory();
  const saved = saveDraft(store, "db", stored({ in_flight: sending }));
  assert.deepEqual(loadDraft(store, "db")?.in_flight, sending);
  // Read back, the record is the same text, so a tab knows its own marker.
  assert.deepEqual(saved, {
    kind: "written",
    text: JSON.stringify(loadDraft(store, "db")),
  });
  const restored = restoredState(loadDraft(store, "db"));
  assert.deepEqual(restored.uncertain?.kept, kept);
  saveDraft(store, "db", storedOf(restored));
  assert.deepEqual(loadDraft(store, "db")?.uncertain?.kept, kept);
  // Malformed provenance drops the draft, as any malformed record does.
  const load = (value: unknown) =>
    loadDraft(memory({ [storageKey("db")]: JSON.stringify(value) }), "db");
  for (const bad of [undefined, null, [], { e: { revision: "t" } }]) {
    const record = { ...flight, kept: bad };
    assert.equal(load({ ...stored(), in_flight: record }), null);
    assert.equal(
      load({
        ...stored(),
        uncertain: {
          ...record,
          error: "transport",
          message: "m",
          outcome: null,
        },
      }),
      null,
    );
  }
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

test("an uncertain record whose applied outcome lacks a usable report is dropped", () => {
  const view = { database: "db", end_exclusive: "4", head: null };
  const report = {
    database: "db",
    base_revision: "b",
    base: view,
    operations: [{ name: "t", entry: { origin: "o", position: "4" } }],
    analysis: {
      view,
      inputs: { evaluation_time: null },
      implementations: [],
      observations: [],
    },
  };
  const uncertain = (outcome: unknown) => ({
    ...stored(),
    uncertain: {
      ...flight,
      error: "publication_uncertain",
      message: "m",
      outcome,
    },
  });
  const load = (value: unknown) =>
    loadDraft(memory({ [storageKey("db")]: JSON.stringify(value) }), "db");
  const applied = { kind: "applied", graph_revision: "r2", report };
  assert.deepEqual(load(uncertain(applied))?.uncertain?.outcome, applied);
  assert.equal(load(uncertain(null))?.uncertain?.outcome, null);
  // Another kind is shown by name only.
  assert.ok(load(uncertain({ kind: "restored", other: 1 })) !== null);
  for (const outcome of [
    { kind: "applied" },
    { kind: "applied", report },
    { ...applied, report: { ...report, operations: undefined } },
    { ...applied, report: { ...report, operations: [{ name: "t" }] } },
    { ...applied, report: { ...report, base: {} } },
    { ...applied, report: { ...report, analysis: { view } } },
    { report },
    "applied",
  ]) {
    assert.equal(load(uncertain(outcome)), null, JSON.stringify(outcome));
  }
  // The decision that the entries are present reads what was validated.
  const state = restoredState(load(uncertain(applied)));
  const decided = draftReducer(state, { kind: "decide", present: true });
  assert.deepEqual(decided.applied?.operations, report.operations);
  assert.deepEqual(decided.proposal.entries, []);
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
