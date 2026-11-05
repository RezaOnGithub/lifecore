import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  AnalyzerAdded,
  AnalyzerConfig,
  EntryId,
  ErrorKind,
  ErrorObject,
  Method,
  NodeRecord,
  Observation,
  ProposalReport,
} from "../contract.ts";
import { entryKey } from "../model/labels.ts";
import {
  type Call,
  HttpRejectionError,
  RpcError,
  TransportError,
} from "../rpc.ts";
import {
  AnalyzerAdds,
  addCounts,
  boundaryWords,
  classifyAdd,
  positionsOf,
  type Sent,
  suiteHolds,
} from "./analyzerAdd.ts";

interface Pending {
  method: Method;
  params: unknown;
  resolve: (value: unknown) => void;
  reject: (failure: unknown) => void;
}

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
const typecheck: AnalyzerConfig = { name: "Type_Check", configuration: "{}" };
const dag: AnalyzerConfig = {
  name: "DAG_Check",
  configuration: '{"on_cycle":"warning"}',
};

function sent(suite: AnalyzerConfig[] = [typecheck]): Sent {
  return { database: "db", params: { ...dag }, suite };
}

function observation(
  kind: "inconsistency" | "warning",
  analyzer: AnalyzerConfig,
  focus: EntryId,
): Observation {
  return {
    key: {
      database: "db",
      analyzer: { ...analyzer, version: "0.1.0" },
      kind,
      analyzer_certificate: `${kind}:${focus.position}`,
    },
    evaluated_view: { kind: "initialization" },
    diagnostic: {
      kind,
      subjects: [focus, id("9")],
      focus,
      message: kind,
      sticky: false,
    },
  };
}

function report(observations: Observation[] = []): ProposalReport {
  const view = { database: "db", end_exclusive: "10", head: null };
  return {
    database: "db",
    base_revision: "r",
    base: view,
    operations: [],
    analysis: {
      view,
      inputs: { evaluation_time: null },
      implementations: [],
      observations,
    },
  };
}

const added: AnalyzerAdded = {
  graph_revision: "r2",
  suite: [typecheck, dag],
  report: report(),
};

function failure(kind: ErrorKind, extra: object = {}): RpcError {
  return new RpcError("analyzers.add", {
    code: 1,
    message: `${kind} text`,
    data: { kind, ...extra },
  } as unknown as ErrorObject);
}

test("an add failure is uncertain unless known to come before publication", () => {
  assert.deepEqual(classifyAdd(failure("rejected", { report: report() })), {
    kind: "rejected",
    report: report(),
  });
  const execution = {
    analyzer: { ...dag, version: "0.1.0" },
    inputs: { evaluation_time: null },
    evaluated_view: { kind: "initialization" },
    message: "boom",
  };
  assert.deepEqual(
    classifyAdd(failure("analyzer_failure", { failure: execution })),
    {
      kind: "analyzer_failure",
      message: "analyzer_failure text",
      failure: execution,
    },
  );
  // A missing or misconfigured analyzer carries no execution failure.
  assert.deepEqual(classifyAdd(failure("analyzer_failure")), {
    kind: "analyzer_failure",
    message: "analyzer_failure text",
    failure: null,
  });
  assert.deepEqual(classifyAdd(failure("busy", { lock: "l" })), {
    kind: "busy",
    message: "busy text",
  });
  for (const kind of [
    "invalid_params",
    "local_time_unknown",
    "lock_failed",
    "database_missing",
    "database_unreadable",
    "publication_failed",
  ] as ErrorKind[]) {
    assert.equal(classifyAdd(failure(kind)).kind, "failed", kind);
  }
  for (const kind of [
    "backend_failure",
    "internal",
    "stale_base",
    "entry_not_found",
    "something_new",
  ] as ErrorKind[]) {
    assert.deepEqual(
      classifyAdd(failure(kind)),
      {
        kind: "uncertain",
        error: kind,
        message: `${kind} text`,
        outcome: null,
      },
      kind,
    );
  }
  // An uncertain publication of this call reports what it would have
  // returned; another kind of outcome is not this call's.
  const uncertain = classifyAdd(
    failure("publication_uncertain", {
      outcome: { kind: "analyzer_added", ...added },
    }),
  );
  assert.deepEqual(uncertain, {
    kind: "uncertain",
    error: "publication_uncertain",
    message: "publication_uncertain text",
    outcome: added,
  });
  const other = classifyAdd(
    failure("publication_uncertain", {
      outcome: { kind: "dismissed", dismissals: [] },
    }),
  );
  assert.equal(other.kind === "uncertain" && other.outcome, null);
});

test("envelope errors and access rejections precede dispatch; lost answers do not", () => {
  const envelope = new RpcError("analyzers.add", {
    code: -32600,
    message: "invalid request",
  });
  assert.equal(classifyAdd(envelope).kind, "failed");
  const refused = new HttpRejectionError(
    401,
    { kind: "token", message: "no token" },
    "",
  );
  assert.equal(classifyAdd(refused).kind, "failed");
  const unstructured = classifyAdd(new HttpRejectionError(502, null, "gw"));
  assert.equal(
    unstructured.kind === "uncertain" && unstructured.error,
    "http_502",
  );
  const lost = classifyAdd(new TransportError("cannot reach the server"));
  assert.equal(lost.kind === "uncertain" && lost.error, "transport");
  const unreadable = classifyAdd(new SyntaxError("bad body"));
  assert.equal(
    unreadable.kind === "uncertain" && unreadable.error,
    "unreadable",
  );
});

test("a suite holds the added analyzer when it has one more copy than before", () => {
  // Compared by name and verbatim configuration: whitespace is identity.
  const spaced = { ...dag, configuration: '{ "on_cycle": "warning" }' };
  assert.deepEqual(positionsOf([typecheck, spaced, dag], sent()), [3]);
  assert.equal(suiteHolds([typecheck], sent()), false);
  assert.equal(suiteHolds([typecheck, spaced], sent()), false);
  assert.equal(suiteHolds([typecheck, dag], sent()), true);
  // Another writer appended in between: still found, by count.
  assert.equal(suiteHolds([typecheck, spaced, dag], sent()), true);
  // An identical analyzer was already configured: only a second copy counts.
  assert.equal(suiteHolds([typecheck, dag], sent([typecheck, dag])), false);
  assert.equal(suiteHolds([typecheck, dag, dag], sent([typecheck, dag])), true);
});

test("counts split the added analyzer's findings from the whole replay's", () => {
  const counts = addCounts(
    report([
      observation("warning", typecheck, id("1")),
      observation("warning", dag, id("2")),
      observation("inconsistency", { ...dag, configuration: "{}" }, id("3")),
    ]),
    sent(),
  );
  assert.deepEqual(counts.total, {
    inconsistency: 1,
    warning: 2,
    suggestion: 0,
  });
  assert.deepEqual(counts.added, {
    inconsistency: 0,
    warning: 1,
    suggestion: 0,
  });
  const view = (end: string) => ({
    database: "db",
    end_exclusive: end,
    head: null,
  });
  assert.equal(
    boundaryWords({ kind: "prefixes", first: view("3"), last: view("3") }),
    "at the prefix boundary 3",
  );
  assert.equal(
    boundaryWords({ kind: "prefixes", first: view("3"), last: view("5") }),
    "at the prefix boundaries 3–5",
  );
});

test("a confirmed add is sent once and its success reloads the screens", async () => {
  const { calls, call } = fake();
  const store = new AnalyzerAdds(call);
  let reloads = 0;
  const first = store.add(sent(), () => {
    reloads += 1;
  });
  assert.equal(store.outcome()?.kind, "sending");
  // A second request while one is in flight is not sent, and the outcome
  // cannot be dismissed from under it.
  await store.add(sent(), () => {
    reloads += 10;
  });
  store.dismiss();
  assert.equal(calls.length, 1);
  assert.equal(calls[0]?.method, "analyzers.add");
  assert.deepEqual(calls[0]?.params, dag);
  assert.equal(store.outcome()?.kind, "sending");
  calls[0]?.resolve(added);
  await first;
  const outcome = store.outcome();
  assert.equal(outcome?.kind, "added");
  assert.deepEqual(outcome?.kind === "added" && outcome.added, added);
  assert.equal(reloads, 1);
  store.dismiss();
  assert.equal(store.outcome(), null);
});

test("a failure is never retried and a lost answer stays uncertain", async () => {
  const { calls, call } = fake();
  const store = new AnalyzerAdds(call);
  let reloads = 0;
  const done = store.add(sent(), () => {
    reloads += 1;
  });
  calls[0]?.reject(new TransportError("cannot reach the server"));
  await done;
  await settle();
  assert.equal(calls.length, 1);
  assert.equal(reloads, 0);
  assert.equal(store.outcome()?.kind, "uncertain");
  // Until the suite is read again, Add waits and the outcome stays: a
  // second add could append the analyzer twice.
  assert.equal(store.waiting(), true);
  await store.add(sent(), () => {});
  store.dismiss();
  assert.equal(calls.length, 1);
  assert.equal(store.outcome()?.kind, "uncertain");
  // A failed read keeps it waiting.
  const failed = store.reread();
  calls[1]?.reject(new TransportError("still down"));
  await failed;
  const unread = store.outcome();
  assert.equal(unread?.kind === "uncertain" && unread.reread?.status, "failed");
  assert.equal(store.waiting(), true);
  // The store reads the suite itself, once at a time.
  const reading = store.reread();
  void store.reread();
  assert.equal(calls.length, 3);
  assert.equal(calls[2]?.method, "database.describe");
  calls[2]?.resolve({
    database: "db",
    graph_revision: "r2",
    view: { database: "db", end_exclusive: "10", head: null },
    suite: [typecheck, dag],
    registered: ["DAG_Check", "Type_Check"],
  });
  await reading;
  const read = store.outcome();
  assert.ok(read?.kind === "uncertain" && read.reread?.status === "read");
  assert.equal(suiteHolds(read.reread.suite, read.sent), true);
  assert.equal(store.waiting(), false);
  store.dismiss();
  assert.equal(store.outcome(), null);
});

test("a rejection lists its inconsistencies with labels read at its boundary", async () => {
  const { calls, call } = fake();
  const store = new AnalyzerAdds(call);
  const rejected = report([
    observation("warning", typecheck, id("1")),
    observation("inconsistency", dag, id("4")),
  ]);
  const done = store.add(sent(), () => {});
  calls[0]?.reject(failure("rejected", { report: rejected }));
  await settle();
  const before = store.outcome();
  assert.equal(before?.kind, "rejected");
  assert.equal(before?.kind === "rejected" && before.nodes, undefined);
  // Only the inconsistency's focus and subjects are read, at the replay's
  // boundary; the warning's focus is not.
  const reads = calls.filter((c) => c.method === "entries.get");
  assert.deepEqual(
    reads.map((c) => c.params),
    [
      { id: id("4"), as_of: "10" },
      { id: id("9"), as_of: "10" },
    ],
  );
  const node: NodeRecord = {
    kind: "node",
    id: id("4"),
    ty_name: "Ring",
    payload: { title: "Ring A" },
    current: true,
  };
  reads[0]?.resolve({ view: rejected.analysis.view, entry: node });
  reads[1]?.reject(failure("entry_not_found", { id: id("9") }));
  await done;
  const after = store.outcome();
  assert.ok(after?.kind === "rejected");
  // A read that failed leaves its entry a position.
  assert.deepEqual([...(after.nodes ?? [])], [[entryKey(id("4")), node]]);
});

test("labels that answer after the outcome was dismissed are dropped", async () => {
  const { calls, call } = fake();
  const store = new AnalyzerAdds(call);
  const done = store.add(sent(), () => {});
  calls[0]?.reject(
    failure("rejected", {
      report: report([observation("inconsistency", dag, id("4"))]),
    }),
  );
  await settle();
  store.dismiss();
  for (const read of calls.filter((c) => c.method === "entries.get")) {
    read.reject(new TransportError("gone"));
  }
  await done;
  assert.equal(store.outcome(), null);
});
