import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  AnalysisOutput,
  Dismissal,
  EntryId,
  EvaluatedView,
  FindingKey,
  MessageType,
  Observation,
} from "../contract.ts";
import {
  aboutEntry,
  categoryCounts,
  defaultFilter,
  type FindingFilter,
  groupFindings,
  keyText,
  kindWords,
  rangeText,
  shownSubjects,
  unconfirmed,
} from "./findings.ts";
import { sameId } from "./labels.ts";

const id = (position: string): EntryId => ({ origin: "o", position });
const view = (end_exclusive: string) => ({
  database: "db",
  end_exclusive,
  head: null,
});
const prefixes = (first: string, last = first): EvaluatedView => ({
  kind: "prefixes",
  first: view(first),
  last: view(last),
});
const initialization: EvaluatedView = { kind: "initialization" };

function key(kind: MessageType, certificate: string): FindingKey {
  return {
    database: "db",
    analyzer: { name: "A", version: "1", configuration: "{}" },
    kind,
    analyzer_certificate: certificate,
  };
}

function observation(
  kind: MessageType,
  certificate: string,
  evaluated_view: EvaluatedView,
  {
    sticky = false,
    subjects = [],
    focus = null,
    message = certificate,
  }: {
    sticky?: boolean;
    subjects?: EntryId[];
    focus?: EntryId | null;
    message?: string;
  } = {},
): Observation {
  const diagnostic = { subjects, focus, message, sticky };
  return {
    key: key(kind, certificate),
    evaluated_view,
    diagnostic:
      kind === "suggestion"
        ? { ...diagnostic, kind, actions: [] }
        : { ...diagnostic, kind },
  };
}

// The boundary is 9.
function output(observations: Observation[]): AnalysisOutput {
  return {
    view: view("9"),
    inputs: { evaluation_time: null },
    implementations: [{ name: "A", version: "1", configuration: "{}" }],
    observations,
  };
}

const dismissed = (kind: MessageType, certificate: string): Dismissal => ({
  key: key(kind, certificate),
  reason: "acknowledged",
  note: null,
});

const shown = (
  analysis: AnalysisOutput,
  dismissals: Dismissal[],
  filter: Partial<FindingFilter> = {},
) =>
  groupFindings(analysis, dismissals, { ...defaultFilter, ...filter }).map(
    (group) => [
      group.key.analyzer_certificate,
      group.selected.map((o) => rangeText(o.evaluated_view)),
    ],
  );

const mixed = output([
  observation("warning", "init", initialization),
  observation("warning", "held", prefixes("3", "9")),
  observation("warning", "resolved", prefixes("2", "5")),
  // Resolved, then held again up to the boundary: one key, two ranges.
  observation("warning", "again", prefixes("2", "4")),
  observation("warning", "again", prefixes("7", "9")),
  observation("suggestion", "sticky", prefixes("4"), { sticky: true }),
  observation("inconsistency", "past", prefixes("5", "6")),
]);

test("current shows initialization, the boundary, and sticky advisories", () => {
  assert.deepEqual(shown(mixed, []), [
    ["init", ["initialization"]],
    ["held", ["boundaries 3–9"]],
    ["again", ["boundaries 7–9"]],
    ["sticky", ["boundary 4"]],
    ["past", ["boundaries 5–6"]],
  ]);
});

test("history shows every observation in first-emission order", () => {
  assert.deepEqual(shown(mixed, [], { selection: "history" }), [
    ["init", ["initialization"]],
    ["held", ["boundaries 3–9"]],
    ["resolved", ["boundaries 2–5"]],
    ["again", ["boundaries 2–4", "boundaries 7–9"]],
    ["sticky", ["boundary 4"]],
    ["past", ["boundaries 5–6"]],
  ]);
});

test("a group keeps all its observations beside the selected ones", () => {
  const again = groupFindings(mixed, [], defaultFilter).find(
    (group) => group.key.analyzer_certificate === "again",
  );
  assert.equal(again?.observations.length, 2);
  assert.equal(again?.selected.length, 1);
});

test("a sticky advisory that ended before the boundary is unconfirmed", () => {
  const [sticky, held] = [mixed.observations[5], mixed.observations[1]];
  assert.ok(sticky && held);
  assert.equal(unconfirmed(sticky, mixed), true);
  assert.equal(unconfirmed(held, mixed), false);
  const atEnd = observation("warning", "s", prefixes("9"), { sticky: true });
  assert.equal(unconfirmed(atEnd, output([atEnd])), false);
});

test("dismissed advisories are hidden unless asked for, by complete key", () => {
  const records = [
    dismissed("warning", "held"),
    // Same certificate, another category: a different key.
    dismissed("suggestion", "init"),
  ];
  assert.deepEqual(
    shown(mixed, records).map(([certificate]) => certificate),
    ["init", "again", "sticky", "past"],
  );
  const groups = groupFindings(mixed, records, {
    ...defaultFilter,
    dismissed: true,
  });
  assert.deepEqual(
    groups.map((group) => [
      group.key.analyzer_certificate,
      group.dismissal?.reason ?? null,
    ]),
    [
      ["init", null],
      ["held", "acknowledged"],
      ["again", null],
      ["sticky", null],
      ["past", null],
    ],
  );
});

test("a record that differs in any key member does not match", () => {
  const record = dismissed("warning", "held");
  const others: Dismissal[] = [
    { ...record, key: { ...record.key, database: "other" } },
    {
      ...record,
      key: {
        ...record.key,
        analyzer: { ...record.key.analyzer, configuration: "" },
      },
    },
    {
      ...record,
      key: {
        ...record.key,
        analyzer: { ...record.key.analyzer, version: "2" },
      },
    },
    { ...record, key: { ...record.key, analyzer_certificate: "held " } },
  ];
  for (const other of others) {
    assert.notEqual(keyText(other.key), keyText(record.key));
    const held = groupFindings(mixed, [other], defaultFilter).find(
      (group) => group.key.analyzer_certificate === "held",
    );
    assert.equal(held?.dismissal, null);
  }
  // Member order in the response does not matter.
  const reordered = JSON.parse(
    `{"analyzer_certificate":"held","kind":"warning","analyzer":{"configuration":"{}","version":"1","name":"A"},"database":"db"}`,
  ) as FindingKey;
  assert.equal(keyText(reordered), keyText(record.key));
});

test("inconsistencies are never hidden, whatever the filters or records", () => {
  const records = [dismissed("inconsistency", "past")];
  const groups = groupFindings(mixed, records, {
    selection: "current",
    warning: false,
    suggestion: false,
    dismissed: false,
  });
  assert.deepEqual(
    groups.map((group) => [group.key.analyzer_certificate, group.dismissal]),
    [["past", null]],
  );
});

test("the category filter hides warnings or suggestions", () => {
  assert.deepEqual(
    shown(mixed, [], { warning: false }).map(([certificate]) => certificate),
    ["sticky", "past"],
  );
  assert.deepEqual(
    shown(mixed, [], { suggestion: false }).map(([c]) => c),
    ["init", "held", "again", "past"],
  );
});

test("counts are per category before the category and dismissal filters", () => {
  const records = [dismissed("warning", "held")];
  assert.deepEqual(categoryCounts(mixed, records, "current"), {
    inconsistency: { findings: 1, dismissed: 0 },
    warning: { findings: 3, dismissed: 1 },
    suggestion: { findings: 1, dismissed: 0 },
  });
  assert.deepEqual(categoryCounts(mixed, records, "history").warning, {
    findings: 4,
    dismissed: 1,
  });
});

test("findings about an entry match its focus or subjects", () => {
  const analysis = output([
    observation("warning", "focus", prefixes("9"), {
      subjects: [id("2")],
      focus: id("2"),
    }),
    observation("warning", "subject", prefixes("9"), {
      subjects: [id("1"), id("2")],
      focus: id("1"),
    }),
    observation("warning", "other", prefixes("9"), {
      subjects: [id("3")],
      focus: id("3"),
    }),
  ]);
  const groups = groupFindings(analysis, [], defaultFilter);
  assert.deepEqual(
    aboutEntry(groups, id("2")).map((g) => g.key.analyzer_certificate),
    ["focus", "subject"],
  );
  assert.deepEqual(aboutEntry(groups, { origin: "x", position: "2" }), []);
});

test("subjects that only repeat the focus are not listed", () => {
  assert.deepEqual(shownSubjects(id("1"), [id("1")], sameId), []);
  assert.deepEqual(shownSubjects(id("1"), [id("1"), id("1")], sameId), []);
  assert.deepEqual(shownSubjects(id("1"), [id("1"), id("2")], sameId), [
    id("1"),
    id("2"),
  ]);
  assert.deepEqual(shownSubjects(null, [id("1")], sameId), [id("1")]);
  assert.deepEqual(shownSubjects(id("1"), [], sameId), []);
});

test("kind words name only the kinds present", () => {
  assert.equal(
    kindWords({ inconsistency: 0, warning: 12, suggestion: 0 }),
    "12 warnings",
  );
  assert.equal(
    kindWords({ inconsistency: 1, warning: 0, suggestion: 2 }),
    "1 inconsistency, 2 suggestions",
  );
  assert.equal(
    kindWords({ inconsistency: 0, warning: 0, suggestion: 0 }),
    "none",
  );
});
