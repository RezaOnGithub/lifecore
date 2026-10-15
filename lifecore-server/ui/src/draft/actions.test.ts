import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  EntryRecord,
  ErrorKind,
  ErrorObject,
  IncidentEdge,
  Method,
  NodeRecord,
  Params,
  ProposalReport,
  Result,
} from "../contract.ts";
import { RpcError, TransportError } from "../rpc.ts";
import { type Call, DraftActions } from "./actions.ts";
import { applicable, type DraftState, initialDraft } from "./model.ts";
import { type Stored, storedChanged, storedOf } from "./persist.ts";
import { type DraftAction, draftReducer } from "./reducer.ts";

const id = (position: string) => ({ origin: "o", position });

function report(evaluation_time: string | null = null): ProposalReport {
  return {
    database: "db",
    base_revision: "r",
    base: { database: "db", end_exclusive: "1", head: null },
    operations: [],
    analysis: {
      view: { database: "db", end_exclusive: "1", head: null },
      inputs: { evaluation_time },
      implementations: [],
      observations: [],
    },
  };
}

// Lets the plan ID mint that adding an operation starts answer.
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function rpc(error: ErrorObject): RpcError {
  return new RpcError("proposal.apply", error);
}

function failure(kind: ErrorKind): RpcError {
  return rpc({
    code: 5001,
    message: kind,
    data: { kind },
  } as unknown as ErrorObject);
}

// A reducer-backed harness: the actions see the state their dispatches
// produced, as under React, and every change of the stored parts is
// recorded as the provider would save it.
function harness(
  answer: <M extends Method>(
    method: M,
    params: Params<M>,
  ) => Promise<Result<M>> | Result<M>,
) {
  let state: DraftState = initialDraft;
  const calls: { method: Method; params: unknown; state: DraftState }[] = [];
  const saved: Stored[] = [];
  const call: Call = async (method, params) => {
    calls.push({ method, params, state });
    return answer(method, params);
  };
  const dispatch = (action: DraftAction) => {
    const next = draftReducer(state, action);
    if (storedChanged(state, next)) {
      saved.push(storedOf(next));
    }
    state = next;
  };
  let refreshed = 0;
  const actions = new DraftActions(
    { call },
    dispatch,
    () => state,
    () => {
      refreshed += 1;
    },
  );
  return {
    actions,
    calls,
    saved,
    dispatch,
    state: () => state,
    refreshed: () => refreshed,
    count: (method: Method) => calls.filter((c) => c.method === method).length,
  };
}

function answers(
  overrides: Partial<{
    [M in Method]: (params: Params<M>) => Promise<Result<M>> | Result<M>;
  }>,
) {
  return <M extends Method>(method: M, params: Params<M>) => {
    const handler = overrides[method] as
      | ((p: Params<M>) => Promise<Result<M>> | Result<M>)
      | undefined;
    if (handler === undefined) {
      throw new Error(`unexpected ${method}`);
    }
    return handler(params);
  };
}

const minted = { "plan_id.mint": () => ({ plan_id: "lc1:p" }) };

test("the first operation mints a plan ID once, and check sends the draft with it", async () => {
  let mints = 0;
  const h = harness(
    answers({
      "plan_id.mint": () => {
        mints += 1;
        return { plan_id: `lc1:${mints}` };
      },
      "proposal.check": () => ({ accepted: true, report: report() }),
    }),
  );
  assert.equal(h.actions.addNode("Task"), "task_1");
  h.actions.addNode("Task");
  await settle();
  assert.equal(mints, 1);
  assert.equal(h.state().proposal.plan_id, "lc1:1");
  await h.actions.check();
  const check = h.calls.find((c) => c.method === "proposal.check");
  assert.deepEqual(check?.params, {
    proposal: { plan_id: "lc1:1", entries: h.state().proposal.entries },
  });
  assert.equal(h.state().outcome?.accepted, true);
  assert.equal(h.state().pending, null);
});

test("check with an unset endpoint is refused locally and apply records the ids", async () => {
  const h = harness(
    answers({
      ...minted,
      "proposal.check": () => ({ accepted: true, report: report() }),
      "proposal.apply": () => ({
        graph_revision: "r2",
        report: {
          ...report(),
          operations: [{ name: "edge_1", entry: id("9") }],
        },
      }),
    }),
  );
  h.actions.connect(id("1"));
  assert.deepEqual(h.state().picking, { operation: "edge_1", role: "to" });
  await settle();
  await h.actions.check();
  assert.equal(h.state().problem?.kind, "incomplete");
  assert.equal(h.count("proposal.check"), 0);
  h.actions.resolvePick(id("2"));
  assert.equal(h.state().problem, null);
  await h.actions.check();
  await h.actions.apply();
  assert.deepEqual(h.state().proposal, { entries: [] });
  assert.equal(h.state().applied?.graph_revision, "r2");
  assert.equal(h.refreshed(), 1);
});

test("the in-flight marker is saved before the apply request leaves", async () => {
  const h = harness(
    answers({
      ...minted,
      "proposal.check": () => ({ accepted: true, report: report() }),
      "proposal.apply": () => ({ graph_revision: "r2", report: report() }),
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.check();
  await h.actions.apply();
  const sent = h.calls.find((c) => c.method === "proposal.apply");
  assert.equal(sent?.state.pending, "apply");
  assert.deepEqual(sent?.state.inFlight, {
    plan_id: "lc1:p",
    request: sent?.state.outcome?.request,
    base_revision: "r",
    base_end: "1",
  });
  // The save before the call carried the marker, the one after cleared it.
  const marked = h.saved.findIndex((s) => s.in_flight !== null);
  assert.ok(marked >= 0);
  assert.equal(h.saved.at(-1)?.in_flight, null);
});

test("after an uncertain apply, Apply stays off until a decision and a new check", async () => {
  const h = harness(
    answers({
      ...minted,
      "proposal.check": () => ({ accepted: true, report: report() }),
      "proposal.apply": () => {
        throw new TransportError("lost");
      },
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.check();
  await h.actions.apply();
  const uncertain = h.state().uncertain;
  assert.equal(uncertain?.error, "transport");
  assert.equal(uncertain?.request, h.state().outcome?.request);
  assert.equal(h.state().inFlight, null);
  assert.equal(h.state().proposal.entries.length, 1);
  assert.equal(h.refreshed(), 1);
  assert.equal(applicable(h.state()), false);
  assert.ok(h.saved.at(-1)?.uncertain !== null);
  // Neither Apply nor Replan acts while the record stands.
  await h.actions.apply();
  await h.actions.replan();
  assert.equal(h.count("proposal.apply"), 1);
  assert.equal(h.count("plan_id.mint"), 1);
  h.dispatch({ kind: "decide", present: false });
  await h.actions.apply();
  assert.equal(h.count("proposal.apply"), 1);
  await h.actions.check();
  assert.equal(applicable(h.state()), true);
});

test("a publication_uncertain outcome offers its ids for clearing the draft", async () => {
  const outcome = {
    kind: "applied" as const,
    graph_revision: "r2",
    report: { ...report(), operations: [{ name: "task_1", entry: id("1") }] },
  };
  const h = harness(
    answers({
      ...minted,
      "proposal.check": () => ({ accepted: true, report: report() }),
      "proposal.apply": () => {
        throw rpc({
          code: 1008,
          message: "unsure",
          data: { kind: "publication_uncertain", outcome },
        });
      },
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.check();
  await h.actions.apply();
  assert.deepEqual(h.state().uncertain?.outcome, outcome);
  h.dispatch({ kind: "decide", present: true });
  assert.deepEqual(h.state().proposal.entries, []);
  assert.deepEqual(h.state().applied?.operations, outcome.report.operations);
});

test("the draft is frozen while an apply is in flight", async () => {
  let release: (result: Result<"proposal.apply">) => void = () => {};
  const h = harness(
    answers({
      ...minted,
      "proposal.check": () => ({ accepted: true, report: report() }),
      "proposal.apply": () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.check();
  const applying = h.actions.apply();
  await settle();
  assert.equal(h.actions.addNode("Note"), null);
  assert.equal(h.actions.revise({ ...node("3") }, "revise"), null);
  assert.equal(h.actions.importProposal({ entries: [] }, "replace"), null);
  h.dispatch({ kind: "clear" });
  h.dispatch({ kind: "remove", names: ["task_1"] });
  assert.deepEqual(
    h.state().proposal.entries.map((e) => e.name),
    ["task_1"],
  );
  release({
    graph_revision: "r2",
    report: { ...report(), operations: [{ name: "task_1", entry: id("1") }] },
  });
  await applying;
  assert.deepEqual(h.state().proposal, { entries: [] });
  assert.equal(h.count("entries.inspect"), 0);
});

test("apply reuses the check's captured instant and refuses a changed evaluation", async () => {
  const h = harness(
    answers({
      ...minted,
      "proposal.check": () => ({
        accepted: true,
        report: report("2026-10-15T08:30:00.123456789Z"),
      }),
      "proposal.apply": () => ({ graph_revision: "r2", report: report() }),
    }),
  );
  h.actions.addNode("Task");
  await settle();
  h.dispatch({ kind: "evaluation", evaluation: { kind: "now" } });
  await h.actions.check();
  h.dispatch({ kind: "evaluation", evaluation: { kind: "none" } });
  await h.actions.apply();
  assert.equal(h.count("proposal.apply"), 0);
  h.dispatch({ kind: "evaluation", evaluation: { kind: "now" } });
  await h.actions.apply();
  const sent = h.calls.find((c) => c.method === "proposal.apply");
  assert.deepEqual(sent?.params, {
    proposal: sent?.state.proposal,
    evaluation_time: "2026-10-15T08:30:00.123456789Z",
  });
});

test("a mint failure is a notice and leaves the banner of the last call", async () => {
  const h = harness(
    answers({
      "plan_id.mint": () => {
        throw new TransportError("down");
      },
    }),
  );
  h.dispatch({ kind: "stale", expected: "a", actual: "b" });
  h.dispatch({
    kind: "problem",
    problem: { kind: "busy", message: "busy", action: "check" },
  });
  h.actions.addNode("Task");
  await settle();
  assert.equal(h.state().problem?.kind, "busy");
  assert.deepEqual(h.state().stale, { expected: "a", actual: "b" });
  assert.match(h.state().notices[0]?.text ?? "", /could not mint a plan ID/);
  assert.equal(h.state().pending, null);
  await h.actions.check();
  assert.equal(h.count("proposal.check"), 0);
  assert.equal(h.state().notices.length, 2);
});

const node = (position: string): NodeRecord => ({
  kind: "node",
  id: id(position),
  ty_name: "Task",
  payload: { title: position },
  current: true,
});

function inspection(of: NodeRecord, others: NodeRecord[]) {
  const edges: IncidentEdge[] = others.map((other, i) => ({
    edge: {
      kind: "edge",
      id: id(`${40 + i}`),
      ty_name: "DependsOn",
      from: of.id,
      to: other.id,
      current: true,
    },
    direction: "outgoing",
    other,
  }));
  return {
    kind: "node" as const,
    view: { database: "db", end_exclusive: "50", head: null },
    entry: of,
    type: { kind: "missing" as const },
    lineage: [of],
    edges,
  };
}

test("revise keeps the node's current edges right after it; branch reads none", async () => {
  const n = node("1");
  const h = harness(
    answers({
      ...minted,
      "entries.inspect": () => inspection(n, [node("2"), node("3")]),
    }),
  );
  h.actions.addNode("Task");
  const name = h.actions.revise(n, "revise");
  assert.equal(name, "task_2");
  await settle();
  assert.deepEqual(
    h.calls.find((c) => c.method === "entries.inspect")?.params,
    { id: n.id },
  );
  assert.deepEqual(
    h.state().proposal.entries.map((e) => e.name),
    ["task_1", "task_2", "task_2_keep_40", "task_2_keep_41"],
  );
  assert.equal(h.state().selected, "task_2");
  h.actions.revise(n, "branch");
  await settle();
  assert.equal(h.count("entries.inspect"), 1);
});

test("a late edge answer for a removed revision lands nowhere, even under the same name", async () => {
  const a = node("1");
  const b = node("7");
  let answerA: (value: Result<"entries.inspect">) => void = () => {};
  const h = harness(
    answers({
      ...minted,
      "entries.inspect": ({ id: asked }) =>
        asked.position === "1"
          ? new Promise((resolve) => {
              answerA = resolve;
            })
          : new Promise(() => {}),
    }),
  );
  assert.equal(h.actions.revise(a, "revise"), "task_1");
  await settle();
  h.dispatch({ kind: "remove", names: ["task_1"] });
  assert.equal(h.actions.revise(b, "revise"), "task_1");
  answerA(inspection(a, [node("2")]));
  await settle();
  assert.deepEqual(
    h.state().proposal.entries.map((e) => e.name),
    ["task_1"],
  );
  assert.match(
    h.state().notices.at(-1)?.text ?? "",
    /arrived after task_1 was removed or changed/,
  );
});

test("picking a predecessor for a revise keeps its edges, without duplicates", async () => {
  const n = node("1");
  const h = harness(
    answers({
      ...minted,
      "entries.inspect": () => inspection(n, [node("2")]),
    }),
  );
  h.actions.addNode("Task");
  await settle();
  h.actions.startPick("task_1", "predecessor");
  h.actions.resolvePick(n.id);
  await settle();
  assert.deepEqual(
    h.state().proposal.entries.map((e) => e.name),
    ["task_1", "task_1_keep_40"],
  );
  // Picking the same node again reads again but keeps nothing twice.
  h.actions.startPick("task_1", "predecessor");
  h.actions.resolvePick(n.id);
  await settle();
  assert.equal(h.state().proposal.entries.length, 2);
  // A branch picks without reading.
  h.actions.addNode("Task");
  h.dispatch({
    kind: "replace",
    name: "task_2",
    entry: {
      name: "task_2",
      kind: "node",
      ty_name: "Task",
      predecessor: { existing: id("9") },
      intent: "branch",
      payload: {},
    },
  });
  h.actions.startPick("task_2", "predecessor");
  h.actions.resolvePick(n.id);
  await settle();
  assert.equal(h.count("entries.inspect"), 2);
});

test("export refuses an unset endpoint locally and keeps the banners", async () => {
  const h = harness(
    answers({
      ...minted,
      "proposal.encode": () => {
        throw new TransportError("down");
      },
    }),
  );
  h.actions.connect(id("1"));
  await settle();
  h.dispatch({
    kind: "problem",
    problem: { kind: "busy", message: "busy", action: "check" },
  });
  assert.deepEqual(await h.actions.encode("toml"), {
    kind: "failed",
    message: "cannot export: edge edge_1 has no to node yet",
  });
  assert.equal(h.count("proposal.encode"), 0);
  h.actions.resolvePick(id("2"));
  const failed = await h.actions.encode("json");
  assert.equal(failed.kind, "failed");
  assert.equal(h.state().problem?.kind, "busy");
  assert.equal(h.state().pending, null);
});

test("looking for the entries of an uncertain apply only reads", async () => {
  let revision = "r";
  let listed: EntryRecord[] = [];
  const h = harness(
    answers({
      ...minted,
      "proposal.check": () => ({ accepted: true, report: report() }),
      "proposal.apply": () => {
        throw failure("backend_failure");
      },
      "database.describe": () => ({
        database: "db",
        graph_revision: revision,
        view: { database: "db", end_exclusive: "2", head: null },
        suite: [],
        registered: [],
      }),
      "entries.list": () => ({
        database: "db",
        graph_revision: revision,
        view: { database: "db", end_exclusive: "2", head: null },
        entries: listed,
      }),
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.check();
  await h.actions.apply();
  await h.actions.lookForEntries();
  assert.deepEqual(h.state().uncertain?.verdict, { kind: "unchanged" });
  revision = "r2";
  listed = [
    {
      kind: "node",
      id: { origin: "x", position: "1" },
      ty_name: "Other",
      payload: {},
      current: true,
    },
  ];
  await h.actions.lookForEntries();
  assert.deepEqual(h.state().uncertain?.verdict, { kind: "other" });
  listed = [{ ...(listed[0] as EntryRecord), ty_name: "Task" }];
  await h.actions.lookForEntries();
  assert.deepEqual(h.state().uncertain?.verdict, {
    kind: "present",
    operations: [{ name: "task_1", entry: { origin: "x", position: "1" } }],
  });
  assert.equal(h.count("proposal.apply"), 1);
  assert.deepEqual(h.calls.find((c) => c.method === "entries.list")?.params, {
    currentness: "all",
  });
});

test("decode reports malformed input with its location and never parses locally", async () => {
  const h = harness(
    answers({
      "proposal.decode": ({ text }) => {
        if (text.startsWith("bad")) {
          throw new RpcError("proposal.decode", {
            code: 2001,
            message: "malformed",
            data: {
              kind: "malformed_proposal",
              decode: {
                format: "toml",
                message: "nope",
                path: "entries[0]",
                line: 2,
                column: 3,
              },
            },
          });
        }
        return { entries: [] };
      },
    }),
  );
  const bad = await h.actions.decode("bad", "toml");
  assert.equal(bad.kind, "malformed");
  assert.equal(bad.kind === "malformed" && bad.decode.line, 2);
  const ok = await h.actions.decode("x = 1", "toml");
  assert.deepEqual(ok, { kind: "ok", proposal: { entries: [] } });
  assert.equal(h.state().pending, null);
});

test("import refuses an acknowledgment and replace lands on the first operation", async () => {
  const h = harness(answers({ ...minted }));
  const acknowledges = {
    database: "db",
    analyzer: { name: "A", version: "1", configuration: "" },
    kind: "suggestion" as const,
    analyzer_certificate: "c",
  };
  assert.equal(
    h.actions.importProposal(
      {
        acknowledges,
        entries: [{ name: "a", kind: "node", ty_name: "T", payload: {} }],
      },
      "replace",
    ),
    null,
  );
  assert.deepEqual(h.state().proposal.entries, []);
  assert.equal(
    h.actions.importProposal(
      {
        entries: [
          { name: "a", kind: "node", ty_name: "T", payload: {} },
          { name: "a", kind: "node", ty_name: "T", payload: {} },
        ],
      },
      "replace",
    ),
    "a",
  );
  await settle();
  assert.deepEqual(
    h.state().proposal.entries.map((e) => e.name),
    ["a", "a_2"],
  );
  assert.equal(h.state().proposal.plan_id, "lc1:p");
  assert.equal(
    h.actions.importProposal(
      { entries: [{ name: "a", kind: "node", ty_name: "T", payload: {} }] },
      "append",
    ),
    "a_3",
  );
});

test("a suggestion's actions append as operations without an acknowledgment", async () => {
  const h = harness(answers({ ...minted }));
  assert.deepEqual(h.actions.addSuggested([]), []);
  h.actions.addNode("Task");
  assert.deepEqual(
    h.actions.addSuggested([
      { kind: "node", ty_name: "Task", payload: { title: "t" } },
      {
        kind: "edge",
        ty_name: "Of",
        from: { added: 0 },
        to: { existing: id("1") },
      },
    ]),
    ["task_2", "of_1"],
  );
  await settle();
  const { proposal } = h.state();
  assert.equal(proposal.acknowledges, undefined);
  assert.equal(proposal.plan_id, "lc1:p");
  assert.deepEqual(
    proposal.entries.map((e) => [e.name, e.kind === "edge" ? e.from : null]),
    [
      ["task_1", null],
      ["task_2", null],
      ["of_1", { added: "task_2" }],
    ],
  );
});
