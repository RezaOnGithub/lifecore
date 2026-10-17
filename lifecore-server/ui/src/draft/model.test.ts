import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProposalEntry, ProposalReport } from "../contract.ts";
import {
  applicable,
  applyParams,
  currentRefusal,
  type DraftState,
  detachedDraft,
  frozenReason,
  initialDraft,
  isUnset,
  pickUse,
  proposalOf,
  requestOf,
  sameEvaluation,
  sameReference,
  UNSET,
} from "./model.ts";
import { draftReducer } from "./reducer.ts";

const id = (position: string) => ({ origin: "o", position });

const task: ProposalEntry = {
  name: "task_1",
  kind: "node",
  ty_name: "Task",
  payload: { title: "a" },
};
const edge: ProposalEntry = {
  name: "dependson_1",
  kind: "edge",
  ty_name: "DependsOn",
  from: { added: "task_1" },
  to: { existing: id("3") },
};

function state(entries: ProposalEntry[], more: Partial<DraftState> = {}) {
  return { ...initialDraft, proposal: { entries }, ...more };
}

function report(evaluation_time: string | null = null): ProposalReport {
  return {
    database: "db",
    base_revision: "r",
    base: { database: "db", end_exclusive: "4", head: null },
    operations: [],
    analysis: {
      view: { database: "db", end_exclusive: "4", head: null },
      inputs: { evaluation_time },
      implementations: [],
      observations: [],
    },
  };
}

function checked(
  draft: DraftState,
  more: { accepted?: boolean; time?: string | null } = {},
) {
  return draftReducer(draft, {
    kind: "checked",
    result: { accepted: more.accepted ?? true, report: report(more.time) },
    request: requestOf(draft),
    evaluation: draft.evaluation,
  });
}

test("the unset placeholder is an existing reference that is never real", () => {
  assert.equal(isUnset(UNSET), true);
  assert.equal(isUnset({ existing: { origin: "", position: "" } }), true);
  assert.equal(isUnset({ existing: id("1") }), false);
  assert.equal(isUnset({ added: "" }), false);
  assert.equal(sameReference({ added: "a" }, { added: "a" }), true);
  assert.equal(sameReference({ added: "a" }, { added: "b" }), false);
  assert.equal(sameReference({ added: "a" }, { existing: id("1") }), false);
  assert.equal(sameReference({ existing: id("1") }, { added: "1" }), false);
  assert.equal(
    sameReference({ existing: id("1") }, { existing: { ...id("1") } }),
    true,
  );
  assert.equal(
    sameReference({ existing: id("1") }, { existing: id("4") }),
    false,
  );
});

test("evaluation choices compare by kind and, when explicit, by time", () => {
  assert.equal(sameEvaluation({ kind: "none" }, { kind: "none" }), true);
  assert.equal(sameEvaluation({ kind: "now" }, { kind: "none" }), false);
  assert.equal(
    sameEvaluation({ kind: "at", time: "x" }, { kind: "at", time: "x" }),
    true,
  );
  assert.equal(
    sameEvaluation({ kind: "at", time: "x" }, { kind: "at", time: "y" }),
    false,
  );
});

test("proposalOf strips client state and omits an absent plan ID", () => {
  assert.deepEqual(proposalOf(state([task], { selected: "task_1" })), {
    entries: [task],
  });
  assert.equal(requestOf(state([task])), JSON.stringify({ entries: [task] }));
  assert.deepEqual(
    Object.keys(
      proposalOf({
        ...state([task]),
        proposal: { plan_id: "p", entries: [task] },
      }),
    ),
    ["plan_id", "entries"],
  );
});

test("Apply is gated on an accepted, unspent check of identical content and evaluation", () => {
  const ok = checked(state([task]));
  assert.equal(applicable(ok), true);
  const edited = draftReducer(ok, {
    kind: "replace",
    name: "task_1",
    entry: { ...task, payload: { title: "changed" } },
  });
  assert.equal(applicable(edited), false);
  assert.equal(edited.outcome, ok.outcome);
  // Adding and removing the same operation restores identical content.
  const restored = draftReducer(
    draftReducer(ok, { kind: "add", entry: edge }),
    { kind: "remove", names: ["dependson_1"] },
  );
  assert.equal(applicable(restored), true);
  assert.equal(applicable(checked(state([task]), { accepted: false })), false);
  assert.equal(
    applicable(draftReducer(ok, { kind: "stale", expected: "a", actual: "b" })),
    false,
  );
  assert.equal(applicable({ ...ok, pending: "check" }), false);
  // A changed evaluation choice needs a new check, and so does going back
  // only to an explicit time that differs.
  const now = draftReducer(ok, {
    kind: "evaluation",
    evaluation: { kind: "now" },
  });
  assert.equal(applicable(now), false);
  assert.equal(
    applicable(
      draftReducer(now, { kind: "evaluation", evaluation: { kind: "none" } }),
    ),
    true,
  );
  const outcome = ok.outcome;
  assert.ok(outcome !== null);
  assert.equal(
    applicable({ ...ok, outcome: { ...outcome, spent: true } }),
    false,
  );
  assert.equal(
    applicable({
      ...ok,
      uncertain: {
        plan_id: null,
        request: "{}",
        base_revision: "r",
        base_end: "4",
        error: "transport",
        message: "lost",
        outcome: null,
        verdict: null,
      },
    }),
    false,
  );
});

test("apply sends the evaluation time the check reported, never a new now", () => {
  const draft = { ...state([task]), evaluation: { kind: "now" as const } };
  const ok = checked(draft, { time: "2026-10-15T08:30:00.123456789Z" });
  assert.ok(ok.outcome !== null);
  assert.deepEqual(applyParams(ok.outcome), {
    evaluation_time: "2026-10-15T08:30:00.123456789Z",
  });
  const none = checked(state([task]));
  assert.ok(none.outcome !== null);
  assert.deepEqual(applyParams(none.outcome), {});
});

test("a frozen draft says why it takes no edit", () => {
  const inFlight = {
    plan_id: null,
    request: "{}",
    base_revision: null,
    base_end: "0",
  };
  assert.equal(frozenReason(initialDraft), null);
  assert.equal(frozenReason(detachedDraft), "the draft waits for the database");
  assert.equal(
    frozenReason({ ...initialDraft, inFlight, pending: "apply" }),
    "an apply is in flight",
  );
  assert.equal(
    frozenReason({ ...initialDraft, inFlight }),
    "an apply is in flight in another tab",
  );
});

test("a pick puts its node to the use of the operation's role and intent", () => {
  const revision: ProposalEntry = {
    name: "task_2",
    kind: "node",
    ty_name: "Task",
    predecessor: UNSET,
    intent: "revise",
    payload: {},
  };
  const branch: ProposalEntry = {
    ...revision,
    name: "task_3",
    intent: "branch",
  };
  const entries = [task, edge, revision, branch];
  const use = (operation: string, role: "predecessor" | "from" | "to") =>
    pickUse({ operation, role }, entries);
  assert.equal(use("dependson_1", "from"), "endpoint");
  assert.equal(use("dependson_1", "to"), "endpoint");
  assert.equal(use("task_2", "predecessor"), "revise");
  assert.equal(use("task_3", "predecessor"), "branch");
  // A root without an intent becomes a revise when its predecessor lands.
  assert.equal(use("task_1", "predecessor"), "revise");
});

test("only a branch takes a node not known to be current in the current state", () => {
  for (const current of [true, false]) {
    for (const bounded of [true, false]) {
      assert.equal(currentRefusal("branch", current, bounded), null);
    }
  }
  assert.equal(currentRefusal("revise", true, false), null);
  assert.equal(currentRefusal("endpoint", true, false), null);
  assert.equal(
    currentRefusal("revise", false, false),
    "only a current node can be revised",
  );
  assert.equal(
    currentRefusal("endpoint", false, false),
    "only a current node can be an end of an edge",
  );
  // A bounded read says what was current at the boundary, current or not.
  for (const current of [true, false]) {
    assert.match(
      currentRefusal("revise", current, true) ?? "",
      /^drafts apply to the current state; clear the as-of boundary/,
    );
    assert.match(
      currentRefusal("endpoint", current, true) ?? "",
      /^drafts apply to the current state; clear the as-of boundary/,
    );
  }
});
