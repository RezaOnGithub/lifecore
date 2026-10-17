import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  Method,
  Params,
  ProposalEntry,
  ProposalReport,
  Result,
} from "../contract.ts";
import { type Call, DraftActions } from "./actions.ts";
import {
  applicable,
  applyingElsewhere,
  frozen,
  type InFlight,
  requestOf,
} from "./model.ts";
import { loadDraft, storageKey, storedOf } from "./persist.ts";
import { SharedDraft } from "./shared.ts";

// Two tabs of one database share a store, as two pages of one origin share
// localStorage; `sync` stands for the `storage` event a write raises in the
// other tab, which may come late.

function memory() {
  const items = new Map<string, string>();
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

const task = (name: string): ProposalEntry => ({
  name,
  kind: "node",
  ty_name: "Task",
  payload: { title: name },
});

const report: ProposalReport = {
  database: "db",
  base_revision: "r",
  base: { database: "db", end_exclusive: "4", head: null },
  operations: [],
  analysis: {
    view: { database: "db", end_exclusive: "4", head: null },
    inputs: { evaluation_time: null },
    implementations: [],
    observations: [],
  },
};

// A tab whose draft holds one checked operation with a plan ID.
function checkedTab(store: ReturnType<typeof memory>): SharedDraft {
  const tab = new SharedDraft(store, "db");
  tab.dispatch({ kind: "add", entry: task("t") });
  tab.dispatch({ kind: "set_plan_id", plan_id: "lc1:p" });
  tab.dispatch({
    kind: "checked",
    result: { accepted: true, report },
    request: requestOf(tab.state),
    evaluation: { kind: "none" },
  });
  return tab;
}

function flightOf(tab: SharedDraft): InFlight {
  return {
    plan_id: tab.state.proposal.plan_id ?? null,
    request: requestOf(tab.state),
    base_revision: "r",
    base_end: "4",
  };
}

test("another tab's edits are adopted, keeping what this tab still applies to", () => {
  const store = memory();
  const a = new SharedDraft(store, "db");
  const b = new SharedDraft(store, "db");
  a.dispatch({ kind: "add", entry: task("t") });
  b.dispatch({ kind: "select", name: null });
  assert.equal(b.sync(), true);
  assert.deepEqual(b.state.proposal, a.state.proposal);
  b.dispatch({ kind: "select", name: "t" });
  b.dispatch({ kind: "add", entry: task("u"), select: false });
  assert.equal(a.sync(), true);
  assert.deepEqual(
    a.state.proposal.entries.map((e) => e.name),
    ["t", "u"],
  );
  // Nothing new to follow; this tab's own write raises no change either.
  assert.equal(a.sync(), false);
  assert.equal(b.sync(), false);
  // A removed operation is no longer selected in the tab that adopts it.
  a.dispatch({ kind: "remove", names: ["t"] });
  assert.equal(b.sync(), true);
  assert.equal(b.state.selected, null);
  // An emptied draft leaves no record, and the other tab follows.
  a.dispatch({ kind: "clear" });
  assert.equal(store.items.size, 0);
  assert.equal(b.sync(), true);
  assert.deepEqual(b.state.proposal, { entries: [] });
});

test("another tab's apply freezes this one, and closing that tab restores the draft as uncertain", () => {
  const store = memory();
  const a = checkedTab(store);
  const b = new SharedDraft(store, "db");
  const flight = flightOf(a);
  a.dispatch({ kind: "apply_started", flight });
  assert.equal(b.sync(), true);
  assert.deepEqual(b.state.inFlight, flight);
  assert.equal(applyingElsewhere(b.state), true);
  assert.equal(frozen(b.state), true);
  // No edit lands, so nothing overwrites the marker.
  const before = store.items.get(storageKey("db"));
  b.dispatch({ kind: "add", entry: task("u") });
  b.dispatch({ kind: "clear" });
  b.dispatch({ kind: "remove", names: ["t"] });
  assert.equal(store.items.get(storageKey("db")), before);
  // Tab A goes away before the answer: the next tab to load the draft sees
  // an apply of uncertain fate.
  const c = new SharedDraft(store, "db");
  assert.equal(c.state.uncertain?.error, "interrupted");
  assert.equal(c.state.uncertain?.request, flight.request);
  assert.equal(applicable(c.state), false);
});

test("an edit racing past another tab's marker is refused and that tab's record adopted", () => {
  const store = memory();
  const a = checkedTab(store);
  const b = new SharedDraft(store, "db");
  assert.equal(b.sync(), false);
  const flight = flightOf(a);
  a.dispatch({ kind: "apply_started", flight });
  // B edits before its storage event arrives.
  const marked = store.items.get(storageKey("db"));
  b.dispatch({ kind: "add", entry: task("u") });
  assert.equal(store.items.get(storageKey("db")), marked);
  assert.deepEqual(b.state.inFlight, flight);
  assert.deepEqual(
    b.state.proposal.entries.map((e) => e.name),
    ["t"],
  );
  assert.match(b.state.notices.at(-1)?.text ?? "", /was not kept/);
  // The event that arrives afterwards changes nothing more.
  assert.equal(b.sync(), false);
});

test("the applying tab writes its marker again over a write that raced past it", () => {
  const store = memory();
  const a = checkedTab(store);
  const flight = flightOf(a);
  const racing = { ...storedOf(a.state), proposal: { entries: [task("x")] } };
  a.dispatch({ kind: "apply_started", flight });
  store.setItem(storageKey("db"), JSON.stringify(racing));
  assert.equal(a.sync(), false);
  assert.deepEqual(loadDraft(store, "db")?.in_flight, flight);
  assert.deepEqual(loadDraft(store, "db")?.proposal, a.state.proposal);
  // Its answer is then saved as usual.
  a.dispatch({
    kind: "applied",
    result: {
      graph_revision: "r2",
      report: {
        ...report,
        operations: [{ name: "t", entry: { origin: "o", position: "4" } }],
      },
    },
    request: flight.request,
  });
  assert.equal(store.items.size, 0);
});

test("the end of another tab's apply unfreezes the draft with a note", () => {
  const store = memory();
  const a = checkedTab(store);
  a.dispatch({ kind: "add", entry: task("later"), select: false });
  a.dispatch({
    kind: "checked",
    result: { accepted: true, report },
    request: requestOf(a.state),
    evaluation: { kind: "none" },
  });
  const b = new SharedDraft(store, "db");
  const flight = flightOf(a);
  a.dispatch({ kind: "apply_started", flight });
  b.sync();
  a.dispatch({
    kind: "applied",
    result: {
      graph_revision: "r2",
      report: {
        ...report,
        operations: [
          { name: "t", entry: { origin: "o", position: "4" } },
          { name: "later", entry: { origin: "o", position: "5" } },
        ],
      },
    },
    request: flight.request,
  });
  assert.equal(b.sync(), true);
  assert.equal(frozen(b.state), false);
  assert.deepEqual(b.state.proposal, { entries: [] });
  assert.match(
    b.state.notices.at(-1)?.text ?? "",
    /apply in another tab ended/,
  );
});

test("an uncertain record spends the other tab's check until a decision there ends it everywhere", () => {
  const store = memory();
  const a = checkedTab(store);
  // B holds its own accepted check of the very same content.
  const b = new SharedDraft(store, "db");
  b.dispatch({
    kind: "checked",
    result: { accepted: true, report },
    request: requestOf(a.state),
    evaluation: { kind: "none" },
  });
  assert.equal(applicable(b.state), true);
  const flight = flightOf(a);
  a.dispatch({ kind: "apply_started", flight });
  a.dispatch({
    kind: "uncertain",
    uncertain: {
      ...flight,
      error: "transport",
      message: "lost",
      outcome: null,
      verdict: null,
    },
  });
  assert.equal(b.sync(), true);
  assert.equal(b.state.uncertain?.error, "transport");
  assert.equal(b.state.outcome?.spent, true);
  assert.equal(applicable(b.state), false);
  // A verdict is this tab's own; the stored record stays the same.
  b.dispatch({ kind: "verdict", verdict: { kind: "absent" } });
  assert.equal(a.sync(), false);
  // The user decides in B, which was shown the record.
  b.dispatch({ kind: "decide", present: false });
  assert.equal(loadDraft(store, "db")?.uncertain, null);
  assert.equal(a.sync(), true);
  assert.equal(a.state.uncertain, null);
});

test("an Apply whose marker cannot be saved over another tab's sends nothing", async () => {
  const store = memory();
  const a = checkedTab(store);
  const b = new SharedDraft(store, "db");
  b.dispatch({
    kind: "checked",
    result: { accepted: true, report },
    request: requestOf(a.state),
    evaluation: { kind: "none" },
  });
  const calls: Method[] = [];
  const call: Call = async <M extends Method>(method: M, _: Params<M>) => {
    calls.push(method);
    return { graph_revision: "r2", report } as unknown as Result<M>;
  };
  const actions = new DraftActions(
    { call },
    (action) => {
      b.dispatch(action);
    },
    () => b.state,
  );
  a.dispatch({ kind: "apply_started", flight: flightOf(a) });
  await actions.apply();
  assert.deepEqual(calls, []);
  assert.equal(applyingElsewhere(b.state), true);
});

test("an unreadable record from another tab is replaced by the next save", () => {
  const store = memory();
  const a = new SharedDraft(store, "db");
  a.dispatch({ kind: "add", entry: task("t") });
  store.setItem(storageKey("db"), "{not json");
  assert.equal(a.sync(), false);
  assert.deepEqual(
    a.state.proposal.entries.map((e) => e.name),
    ["t"],
  );
  a.dispatch({ kind: "add", entry: task("u") });
  assert.deepEqual(
    loadDraft(store, "db")?.proposal.entries.map((e) => e.name),
    ["t", "u"],
  );
});

test("before the database is known nothing is read, stored, or edited", () => {
  const store = memory();
  const tab = new SharedDraft(store, null);
  assert.equal(tab.state.detached, true);
  assert.equal(frozen(tab.state), true);
  tab.dispatch({ kind: "add", entry: task("t") });
  tab.dispatch({
    kind: "import",
    mode: "append",
    proposal: { entries: [task("u")] },
  });
  assert.deepEqual(tab.state.proposal, { entries: [] });
  assert.equal(tab.sync(), false);
  assert.equal(store.items.size, 0);
});
