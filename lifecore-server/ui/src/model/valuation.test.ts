import assert from "node:assert/strict";
import { test } from "node:test";
import type { NodeEntry, Ranked, Reason, Valuation } from "../contract.ts";
import * as recorded from "../fixtures/planning.valuation.ts";
import {
  appliedSample,
  configuresPlanning,
  emphasis,
  problemWords,
  rowName,
  sampleChoice,
  TIERS,
  tierGroups,
  tierList,
  valuationParams,
} from "./valuation.ts";

const head: Valuation = recorded.response.result;
const asOf: Valuation = recorded.asOfResponse.result;
const unsampled: Valuation = recorded.unsampledResponse.result;

const positions = (rows: readonly Ranked[]) =>
  rows.map((row) => row.revision.position);

test("every tier is named once, in order of pressure", () => {
  assert.deepEqual(
    TIERS.map(({ tier }) => tier),
    [0, 1, 2, 3, 4, 5],
  );
  assert.equal(new Set(TIERS.map(({ title }) => title)).size, 6);
});

test("the recorded ranking groups into every tier in the server's order", () => {
  const { shown, empty } = tierGroups(head.ranked);
  assert.deepEqual(empty, []);
  assert.deepEqual(
    shown.map(({ tier, rows }) => [tier, positions(rows)]),
    [
      [0, ["15", "17"]],
      // Not by position: the server orders a tier by its keys.
      [1, ["20", "11"]],
      [2, ["13"]],
      [3, ["27"]],
      [4, ["16"]],
      [5, ["10", "19", "37"]],
    ],
  );
  assert.equal(shown[0]?.title, "What the next seal would reject");
  // The groups hold every row once, as the server sent it.
  assert.deepEqual(
    shown.flatMap(({ rows }) => rows),
    head.ranked,
  );
});

test("tiers without rows are listed apart, in order", () => {
  const { shown, empty } = tierGroups(asOf.ranked);
  assert.deepEqual(
    shown.map(({ tier }) => tier),
    [0, 2, 4, 5],
  );
  assert.deepEqual(empty, [1, 3]);
  assert.equal(
    tierList(empty),
    "tier 1 (standing commitments) or tier 3 (deferred or waiting)",
  );
  assert.equal(
    tierList([0, 2, 4]),
    "tier 0 (what the next seal would reject), tier 2 (habits and concerns below quota), or tier 4 (starved goals and projects)",
  );
  assert.equal(tierList([5]), "tier 5 (everything else in play)");
  assert.deepEqual(tierGroups([]), {
    shown: [],
    empty: [0, 1, 2, 3, 4, 5],
  });
});

test("the deadline sample may reorder a tier, and the screen follows the server", () => {
  const tier1 = (valuation: Valuation) =>
    positions(
      tierGroups(valuation.ranked).shown.find(({ tier }) => tier === 1)?.rows ??
        [],
    );
  // With a sample, the passed deadline sorts first; without one, neither
  // task has a deadline reason, and they keep their roots' order.
  assert.deepEqual(tier1(head), ["20", "11"]);
  assert.deepEqual(tier1(unsampled), ["11", "20"]);
  const names = unsampled.ranked.flatMap(({ reasons }) =>
    reasons.map(({ reason }) => reason.name),
  );
  assert.equal(names.includes("deadline_in"), false);
  assert.equal(names.includes("deadline_passed"), false);
  assert.deepEqual(valuationParams({}, 0), recorded.unsampledRequest.params);
});

test("a tier out of the server's order keeps the order within it", () => {
  const row = (position: string, tier: Ranked["tier"]): Ranked => ({
    revision: { origin: "o", position },
    tier,
    reasons: [],
  });
  const { shown } = tierGroups([row("9", 2), row("1", 0), row("3", 2)]);
  assert.deepEqual(
    shown.map(({ tier, rows }) => [tier, positions(rows)]),
    [
      [0, ["1"]],
      [2, ["9", "3"]],
    ],
  );
});

test("emphasis reads the structured reason, never its text", () => {
  const seen = new Map<Reason["name"], ReturnType<typeof emphasis>>();
  for (const { reasons } of head.ranked) {
    for (const { reason } of reasons) {
      seen.set(reason.name, emphasis(reason));
    }
  }
  // The recorded ranking gives every reason kind.
  assert.deepEqual(Object.fromEntries(seen), {
    broken_plan: "urgent",
    undecided: "urgent",
    open_for: null,
    deadline_passed: "urgent",
    deadline_in: null,
    shortfall: null,
    standing: null,
    lapses_at_next_seal: "soon",
    starved: null,
    new: null,
  });
});

test("decimal reasons stay strings", () => {
  const reasons = head.ranked.flatMap((row) => row.reasons);
  const find = (name: Reason["name"]) =>
    reasons.find(({ reason }) => reason.name === name)?.reason;
  assert.deepEqual(find("shortfall"), {
    name: "shortfall",
    have: 1,
    quota: "3",
  });
  assert.deepEqual(find("deadline_in"), { name: "deadline_in", reviews: "7" });
  assert.deepEqual(find("standing"), {
    name: "standing",
    kind: "deferred",
    seals_left: "1",
  });
});

test("a row keeps its label, type, and whether a field labels it", () => {
  const node = (payload: NodeEntry["payload"]): NodeEntry => ({
    kind: "node",
    id: { origin: "o", position: "12" },
    ty_name: "Goal",
    payload,
  });
  assert.deepEqual(rowName(node({ title: "Learn to cook" })), {
    label: "Learn to cook",
    ty_name: "Goal",
    labeled: true,
  });
  assert.deepEqual(
    rowName(
      node({ deadline: { kind: "timestamp", value: "2026-03-01T09:00:00Z" } }),
    ),
    {
      label: "Goal @12",
      ty_name: "Goal",
      labeled: false,
    },
  );
});

test("a payload problem reads as the end of the failure's sentence", () => {
  assert.equal(problemWords({ kind: "missing" }), "is missing");
  assert.equal(
    problemWords({ kind: "wrong_kind", expected: "integer", found: "text" }),
    "should be an integer, found text",
  );
});

test("the deadline sample is the default, a whole number, or refused", () => {
  assert.deepEqual(sampleChoice(""), { kind: "default" });
  assert.deepEqual(sampleChoice("  "), { kind: "default" });
  assert.deepEqual(sampleChoice("0"), { kind: "windows", windows: 0 });
  assert.deepEqual(sampleChoice(" 12 "), { kind: "windows", windows: 12 });
  assert.deepEqual(sampleChoice("06"), { kind: "windows", windows: 6 });
  for (const text of ["-1", "1.5", "1e3", "six", "+3"]) {
    assert.equal(sampleChoice(text).kind, "invalid", text);
  }
  assert.equal(sampleChoice(String(Number.MAX_SAFE_INTEGER)).kind, "windows");
  assert.deepEqual(sampleChoice("9007199254740992"), {
    kind: "invalid",
    problem: "too many windows",
  });
});

test("text that is not a number keeps the sample applied before", () => {
  assert.equal(appliedSample("3", null), 3);
  assert.equal(appliedSample("3x", 3), 3);
  assert.equal(appliedSample("3x", null), null);
  assert.equal(appliedSample("", 3), null);
  assert.equal(appliedSample("0", 3), 0);
  assert.equal(appliedSample("9007199254740992", 4), 4);
});

test("the request carries the sample only when one is chosen", () => {
  assert.deepEqual(valuationParams({}, null), {});
  assert.deepEqual(valuationParams({ as_of: "34" }, null), { as_of: "34" });
  assert.deepEqual(valuationParams({ as_of: "34" }, 0), {
    as_of: "34",
    deadline_sample_windows: 0,
  });
  // The recorded request at an earlier boundary has this shape.
  assert.deepEqual(
    valuationParams({ as_of: "34" }, 0),
    recorded.asOfRequest.params,
  );
  assert.deepEqual(valuationParams({}, null), recorded.request.params);
});

test("whether the suite configures Planning goes by its registered name", () => {
  assert.equal(
    configuresPlanning([{ name: "Type_Check", configuration: "{}" }]),
    false,
  );
  assert.equal(
    configuresPlanning([
      { name: "Type_Check", configuration: "{}" },
      { name: "Planning", configuration: "" },
    ]),
    true,
  );
  assert.equal(configuresPlanning([]), false);
});
