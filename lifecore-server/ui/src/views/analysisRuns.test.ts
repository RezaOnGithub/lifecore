import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  AnalysisOutput,
  AnalysisRun,
  Dismissal,
  EntryId,
  ErrorKind,
  ErrorObject,
  FindingKey,
  Method,
  NodeRecord,
} from "../contract.ts";
import { HttpRejectionError, RpcError, TransportError } from "../rpc.ts";
import {
  AnalysisRuns,
  type Call,
  classifyEdit,
  type Ran,
  readLabels,
  shownRun,
} from "./analysisRuns.ts";

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

function key(certificate: string): FindingKey {
  return {
    database: "db",
    analyzer: { name: "A", version: "1", configuration: "{}" },
    kind: "warning",
    analyzer_certificate: certificate,
  };
}

const record = (
  certificate: string,
  note: string | null = null,
): Dismissal => ({
  key: key(certificate),
  reason: "acknowledged",
  note,
});

function analysis(end: string): AnalysisOutput {
  return {
    view: { database: "db", end_exclusive: end, head: null },
    inputs: { evaluation_time: null },
    implementations: [],
    observations: [],
  };
}

const answer = (end: string, dismissals: Dismissal[] = []): AnalysisRun => ({
  analysis: analysis(end),
  dismissals,
});

function rpcFailure(kind: ErrorKind, extra: object = {}): RpcError {
  return new RpcError("findings.dismiss", {
    code: 1,
    message: kind,
    data: { kind, ...extra },
  } as unknown as ErrorObject);
}

function ready(store: AnalysisRuns, key: string): Ran {
  const state = store.get(key);
  assert.equal(state?.status, "ready");
  const ran = shownRun(state);
  assert.ok(ran !== null);
  return ran;
}

// The pending call of `method`, the last one by default.
function pending(calls: Pending[], method: Method): Pending {
  const found = calls.filter((c) => c.method === method).at(-1);
  assert.ok(found, `a ${method} call`);
  return found;
}

test("an edit's records apply to runs requested before it answered", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const before = store.run({}, 0);
  pending(calls, "analysis.run").resolve(answer("3", [record("a")]));
  await settle();
  const early = ready(store, before);
  assert.deepEqual(store.dismissals(early), [record("a")]);
  const edit = store.dismiss(key("b"), "acknowledged", "");
  pending(calls, "findings.dismiss").resolve({
    dismissals: [record("a"), record("b")],
  });
  assert.equal(await edit, "stored");
  assert.deepEqual(store.dismissals(early), [record("a"), record("b")]);
  // A run requested after the edit reads the stored records itself.
  const after = store.run({ as_of: "3" }, 0);
  pending(calls, "analysis.run").resolve(answer("3", [record("b")]));
  await settle();
  assert.deepEqual(store.dismissals(ready(store, after)), [record("b")]);
  assert.deepEqual(store.dismissals(early), [record("a"), record("b")]);
});

test("edits go one at a time and a failure is not retried", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const first = store.dismiss(key("a"), "acknowledged", "  spaced  ");
  assert.equal(store.busy(), true);
  assert.equal(await store.restore(key("b")), "refused");
  assert.equal(calls.length, 1);
  // The note goes as typed; an empty one is left out.
  assert.deepEqual(calls[0]?.params, {
    key: key("a"),
    reason: "acknowledged",
    note: "  spaced  ",
  });
  calls[0]?.reject(rpcFailure("busy", { lock: "l" }));
  assert.equal(await first, "failed");
  assert.equal(store.busy(), false);
  assert.equal(store.editFailure(key("a"))?.kind, "failed");
  await settle();
  assert.equal(calls.length, 1);
  void store.dismiss(key("a"), "false_positive", "");
  assert.deepEqual(calls[1]?.params, {
    key: key("a"),
    reason: "false_positive",
  });
  // A new edit of the key clears its failure.
  assert.equal(store.editFailure(key("a")), null);
});

test("a failed run stays failed until run again", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const key = store.run({}, 0);
  pending(calls, "analysis.run").reject(new TransportError("lost"));
  await settle();
  assert.equal(store.get(key)?.status, "failed");
  assert.equal(store.run({}, 0), key);
  assert.equal(calls.length, 1);
  store.run({}, 0, true);
  assert.equal(calls.length, 2);
  assert.equal(store.get(key)?.status, "running");
});

test("the answer to a replaced run is dropped", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const key = store.run({}, 0);
  const replaced = pending(calls, "analysis.run");
  // Four other runs evict it while it runs; asked for again, it restarts.
  for (const refresh of [1, 2, 3, 4]) {
    store.run({}, refresh);
  }
  assert.equal(store.get(key), undefined);
  store.run({}, 0);
  const current = pending(calls, "analysis.run");
  replaced.resolve(answer("1"));
  await settle();
  assert.equal(store.get(key)?.status, "running");
  current.resolve(answer("2"));
  await settle();
  assert.equal(ready(store, key).run.analysis.view.end_exclusive, "2");
});

test("only a few runs are kept, never evicting the screen's", async () => {
  const { call } = fake();
  const store = new AnalysisRuns(call);
  const pinned = store.run({}, 0);
  store.setScreen({ last: { key: pinned, asOf: null, refresh: 0 } });
  const others = [1, 2, 3, 4, 5].map((refresh) => store.run({}, refresh));
  assert.notEqual(store.get(pinned), undefined);
  assert.equal(store.get(others[0] ?? ""), undefined);
  assert.equal(store.get(others[1] ?? ""), undefined);
  for (const kept of others.slice(2)) {
    assert.notEqual(store.get(kept), undefined);
  }
});

test("edit failures are classified by what they say about the records", () => {
  const before: ErrorKind[] = [
    "busy",
    "lock_failed",
    "publication_failed",
    "database_missing",
    "database_unreadable",
    "invalid_params",
    "foreign_database",
    "cannot_dismiss_inconsistency",
  ];
  for (const kind of before) {
    assert.equal(classifyEdit(rpcFailure(kind)).kind, "failed", kind);
  }
  const envelope = new RpcError("findings.dismiss", {
    code: -32600,
    message: "invalid request",
  });
  assert.equal(classifyEdit(envelope).kind, "failed");
  assert.equal(
    classifyEdit(
      new HttpRejectionError(403, { kind: "token", message: "no" }, ""),
    ).kind,
    "failed",
  );
  const written = classifyEdit(
    rpcFailure("publication_uncertain", {
      outcome: { kind: "restored", dismissals: [record("a")], restored: true },
    }),
  );
  assert.deepEqual(written.kind === "written" ? written.dismissals : null, [
    record("a"),
  ]);
  const uncertain = [
    rpcFailure("publication_uncertain", {
      outcome: { kind: "initialized", path: "p" },
    }),
    rpcFailure("backend_failure"),
    rpcFailure("internal"),
    rpcFailure("entry_not_found", { id: id("1") }),
    rpcFailure("something_new" as ErrorKind),
    new HttpRejectionError(502, null, "bad gateway"),
    new TransportError("lost"),
    new Error("unreadable"),
  ];
  for (const failure of uncertain) {
    assert.equal(classifyEdit(failure).kind, "uncertain", String(failure));
  }
});

test("an edit of unknown outcome marks earlier runs until an edit answers", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const early = store.run({}, 0);
  pending(calls, "analysis.run").resolve(answer("3", [record("a")]));
  await settle();
  const ran = ready(store, early);
  const lost = store.dismiss(key("b"), "acknowledged", "");
  pending(calls, "findings.dismiss").reject(new TransportError("lost"));
  assert.equal(await lost, "uncertain");
  assert.equal(store.uncertain(ran), true);
  assert.equal(store.editFailure(key("b"))?.kind, "uncertain");
  // The records shown stay the last known ones.
  assert.deepEqual(store.dismissals(ran), [record("a")]);
  // A run requested after the failure reads the stored records.
  const later = store.run({}, 1);
  pending(calls, "analysis.run").resolve(answer("3", [record("a")]));
  await settle();
  assert.equal(store.uncertain(ready(store, later)), false);
  assert.equal(store.uncertain(ran), true);
  // The next answered edit lists every stored record.
  const next = store.restore(key("a"));
  pending(calls, "findings.restore").resolve({
    dismissals: [record("b")],
    restored: true,
  });
  assert.equal(await next, "stored");
  assert.equal(store.uncertain(ran), false);
  assert.deepEqual(store.dismissals(ran), [record("b")]);
});

test("a publication of unconfirmed durability is adopted and labeled", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const early = store.run({}, 0);
  pending(calls, "analysis.run").resolve(answer("3"));
  await settle();
  const ran = ready(store, early);
  const edit = store.dismiss(key("a"), "acknowledged", "n");
  pending(calls, "findings.dismiss").reject(
    rpcFailure("publication_uncertain", {
      outcome: { kind: "dismissed", dismissals: [record("a", "n")] },
    }),
  );
  assert.equal(await edit, "written");
  assert.deepEqual(store.dismissals(ran), [record("a", "n")]);
  assert.equal(store.written(ran, key("a")), true);
  assert.equal(store.written(ran, key("b")), false);
  assert.equal(store.uncertain(ran), false);
  assert.equal(store.editFailure(key("a")), null);
  const confirmed = store.dismiss(key("b"), "acknowledged", "");
  pending(calls, "findings.dismiss").resolve({
    dismissals: [record("a", "n"), record("b")],
  });
  await confirmed;
  assert.equal(store.written(ran, key("a")), false);
});

test("dismiss all edits one key at a time, past known failures, until stopped", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const keys = ["a", "b", "c", "d"].map(key);
  const all = store.dismissAll(
    { id: "cluster", label: "m" },
    keys,
    "no_longer_relevant",
    "",
  );
  assert.equal(calls.length, 1);
  // A single edit waits for the whole run.
  assert.equal(await store.dismiss(key("z"), "acknowledged", ""), "refused");
  calls[0]?.resolve({ dismissals: [record("a")] });
  await settle();
  assert.equal(calls.length, 2);
  calls[1]?.reject(rpcFailure("lock_failed", { path: "p" }));
  await settle();
  assert.equal(calls.length, 3);
  store.stopBulk();
  calls[2]?.resolve({ dismissals: [record("a"), record("c")] });
  await all;
  assert.equal(calls.length, 3);
  assert.deepEqual(store.bulk(), {
    cluster: "cluster",
    label: "m",
    total: 4,
    stored: 2,
    failed: 1,
    running: false,
    stopping: true,
    stopped: "stopped",
  });
  assert.equal(store.editFailure(key("b"))?.kind, "failed");
});

test("dismiss all ends at an edit of unknown outcome", async () => {
  const { calls, call } = fake();
  const store = new AnalysisRuns(call);
  const all = store.dismissAll(
    { id: "c", label: "m" },
    [key("a"), key("b")],
    "acknowledged",
    "",
  );
  calls[0]?.reject(new TransportError("lost"));
  await all;
  assert.equal(calls.length, 1);
  assert.equal(store.bulk()?.stopped, "an edit's outcome is unknown");
  assert.equal(store.busy(), false);
});

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
