import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  ErrorObject,
  Method,
  Params,
  ProposalReport,
  Result,
} from "../contract.ts";
import { HttpRejectionError, RpcError, TransportError } from "../rpc.ts";
import { type Call, classify, DraftActions } from "./actions.ts";
import {
  type DraftAction,
  type DraftState,
  draftReducer,
  initialDraft,
} from "./model.ts";

const report: ProposalReport = {
  database: "db",
  base_revision: "r",
  base: { database: "db", end_exclusive: "1", head: null },
  operations: [],
  analysis: {
    view: { database: "db", end_exclusive: "1", head: null },
    inputs: { evaluation_time: null },
    implementations: [],
    observations: [],
  },
};

// Lets the plan ID mint that adding an operation starts answer.
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function rpc(error: ErrorObject): RpcError {
  return new RpcError("proposal.apply", error);
}

test("failures are classified by kind, and by action for lost answers", () => {
  assert.deepEqual(
    classify(
      rpc({
        code: 2005,
        message: "stale",
        data: { kind: "stale_base", expected: "a", actual: "b" },
      }),
      "check",
    ),
    { kind: "stale", expected: "a", actual: "b" },
  );
  assert.deepEqual(
    classify(
      rpc({ code: 2006, message: "no", data: { kind: "rejected", report } }),
      "apply",
    ),
    { kind: "rejected", report },
  );
  const invalid = classify(
    rpc({
      code: 2002,
      message: "bad",
      data: { kind: "invalid_proposal", location: { operation: 1, name: "x" } },
    }),
    "check",
  );
  assert.deepEqual(invalid, {
    kind: "problem",
    problem: {
      kind: "invalid_proposal",
      message: "bad",
      location: { operation: 1, name: "x" },
    },
  });
  const busy = classify(
    rpc({ code: 1006, message: "busy", data: { kind: "busy", lock: "l" } }),
    "apply",
  );
  assert.equal(busy.kind === "problem" && busy.problem.kind, "busy");
  const backend = {
    code: 5001 as const,
    message: "gone",
    data: { kind: "backend_failure" as const },
  };
  assert.equal(
    classify(rpc(backend), "apply").kind === "problem" &&
      (classify(rpc(backend), "apply") as { problem: { kind: string } }).problem
        .kind,
    "uncertain",
  );
  const onCheck = classify(rpc(backend), "check");
  assert.equal(onCheck.kind === "problem" && onCheck.problem.kind, "retryable");
  const lostApply = classify(new TransportError("lost"), "apply");
  assert.equal(
    lostApply.kind === "problem" && lostApply.problem.kind,
    "uncertain",
  );
  const lostCheck = classify(new TransportError("lost"), "check");
  assert.equal(
    lostCheck.kind === "problem" && lostCheck.problem.kind,
    "retryable",
  );
  const refused = classify(new HttpRejectionError(401, null, "no"), "check");
  assert.equal(refused.kind === "problem" && refused.problem.kind, "failed");
  const unknown = classify(
    rpc({ code: -32602, message: "params", data: { kind: "invalid_params" } }),
    "check",
  );
  assert.equal(unknown.kind === "problem" && unknown.problem.kind, "failed");
});

// A reducer-backed harness: the actions see the state their dispatches
// produced, as under React.
function harness(
  answer: <M extends Method>(method: M, params: Params<M>) => Result<M>,
) {
  let state: DraftState = initialDraft;
  const calls: { method: Method; params: unknown }[] = [];
  const call: Call = async (method, params) => {
    calls.push({ method, params });
    return answer(method, params);
  };
  const dispatch = (action: DraftAction) => {
    state = draftReducer(state, action);
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
  return { actions, calls, state: () => state, refreshed: () => refreshed };
}

function answers(
  overrides: Partial<{ [M in Method]: (params: Params<M>) => Result<M> }>,
) {
  return <M extends Method>(method: M, params: Params<M>): Result<M> => {
    const handler = overrides[method] as
      | ((p: Params<M>) => Result<M>)
      | undefined;
    if (handler === undefined) {
      throw new Error(`unexpected ${method}`);
    }
    return handler(params);
  };
}

test("the first operation mints a plan ID once, and check sends the draft with it", async () => {
  let minted = 0;
  const h = harness(
    answers({
      "plan_id.mint": () => {
        minted += 1;
        return { plan_id: `lc1:${minted}` };
      },
      "proposal.check": () => ({ accepted: true, report }),
    }),
  );
  const name = h.actions.addNode("Task");
  assert.equal(name, "task_1");
  h.actions.addNode("Task");
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(minted, 1);
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
      "plan_id.mint": () => ({ plan_id: "lc1:p" }),
      "proposal.check": () => ({ accepted: true, report }),
      "proposal.apply": () => ({
        graph_revision: "r2",
        report: {
          ...report,
          operations: [
            { name: "edge_1", entry: { origin: "o", position: "9" } },
          ],
        },
      }),
    }),
  );
  h.actions.connect({ origin: "o", position: "1" });
  assert.deepEqual(h.state().picking, { operation: "edge_1", role: "to" });
  await settle();
  await h.actions.check();
  assert.equal(h.state().problem?.kind, "incomplete");
  assert.equal(
    h.calls.some((c) => c.method === "proposal.check"),
    false,
  );
  h.actions.resolvePick({ origin: "o", position: "2" });
  assert.equal(h.state().problem, null);
  await h.actions.check();
  assert.equal(h.refreshed(), 0);
  await h.actions.apply();
  assert.deepEqual(h.state().proposal, { entries: [] });
  assert.equal(h.state().applied?.graph_revision, "r2");
  assert.equal(h.refreshed(), 1);
});

test("apply is refused without an accepted check of the same content", async () => {
  const h = harness(
    answers({
      "plan_id.mint": () => ({ plan_id: "lc1:p" }),
      "proposal.check": () => ({ accepted: true, report }),
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.apply();
  await h.actions.check();
  h.actions.addNode("Task");
  await h.actions.apply();
  assert.equal(
    h.calls.some((c) => c.method === "proposal.apply"),
    false,
  );
});

test("revise copies the node and keeps its current edges right after it", async () => {
  const other = (position: string) => ({
    kind: "node" as const,
    id: { origin: "o", position },
    ty_name: "Task",
    payload: {},
    current: true,
  });
  const node = { ...other("1"), payload: { title: "t" } };
  const h = harness(
    answers({
      "plan_id.mint": () => ({ plan_id: "lc1:p" }),
      "entries.inspect": () => ({
        kind: "node",
        view: { database: "db", end_exclusive: "9", head: null },
        entry: node,
        type: { kind: "missing" },
        lineage: [node],
        edges: [
          {
            edge: {
              kind: "edge",
              id: { origin: "o", position: "4" },
              ty_name: "DependsOn",
              from: node.id,
              to: other("2").id,
              current: true,
            },
            direction: "outgoing",
            other: other("2"),
          },
          {
            edge: {
              kind: "edge",
              id: { origin: "o", position: "5" },
              ty_name: "DependsOn",
              from: other("3").id,
              to: node.id,
              current: true,
            },
            direction: "incoming",
            other: other("3"),
          },
        ],
      }),
    }),
  );
  h.actions.addNode("Task");
  const name = h.actions.revise(node, "revise");
  assert.equal(name, "task_2");
  await settle();
  const inspect = h.calls.find((c) => c.method === "entries.inspect");
  assert.deepEqual(inspect?.params, { id: node.id });
  assert.equal(h.calls.filter((c) => c.method === "plan_id.mint").length, 1);
  const { entries } = h.state().proposal;
  assert.deepEqual(
    entries.map((e) => e.name),
    ["task_1", "task_2", "task_2_keep_4", "task_2_keep_5"],
  );
  const revision = entries[1];
  assert.ok(revision?.kind === "node");
  assert.deepEqual(revision.predecessor, { existing: node.id });
  assert.equal(revision.intent, "revise");
  assert.deepEqual(revision.payload, { title: "t" });
  assert.equal(h.state().selected, "task_2");
  // A branch keeps nothing by default and reads no edges.
  h.actions.revise(node, "branch");
  await settle();
  assert.equal(h.calls.filter((c) => c.method === "entries.inspect").length, 1);
});

test("a stale check keeps the draft and replan mints anew", async () => {
  let minted = 0;
  const h = harness(
    answers({
      "plan_id.mint": () => {
        minted += 1;
        return { plan_id: `lc1:${minted}` };
      },
      "proposal.check": () => {
        throw rpc({
          code: 2005,
          message: "stale",
          data: { kind: "stale_base", expected: "a", actual: "b" },
        });
      },
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.check();
  assert.deepEqual(h.state().stale, { expected: "a", actual: "b" });
  assert.equal(h.state().proposal.entries.length, 1);
  await h.actions.replan();
  assert.equal(h.state().proposal.plan_id, "lc1:2");
  assert.equal(h.state().stale, null);
});

test("a rejected apply keeps its report; a lost apply is uncertain", async () => {
  let lose = false;
  const h = harness(
    answers({
      "plan_id.mint": () => ({ plan_id: "lc1:p" }),
      "proposal.check": () => ({ accepted: true, report }),
      "proposal.apply": () => {
        if (lose) {
          throw new TransportError("lost");
        }
        throw rpc({
          code: 2006,
          message: "no",
          data: { kind: "rejected", report },
        });
      },
    }),
  );
  h.actions.addNode("Task");
  await settle();
  await h.actions.check();
  await h.actions.apply();
  assert.equal(h.state().outcome?.source, "apply");
  assert.equal(h.state().outcome?.accepted, false);
  assert.equal(h.state().proposal.entries.length, 1);
  await h.actions.check();
  lose = true;
  assert.equal(h.refreshed(), 0);
  await h.actions.apply();
  assert.equal(h.state().problem?.kind, "uncertain");
  assert.equal(h.state().proposal.entries.length, 1);
  assert.equal(h.refreshed(), 1);
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
