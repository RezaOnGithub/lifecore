import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  AnalysisOutput,
  AnalysisRun,
  Dismissal,
  EntryId,
  EvaluatedView,
  FindingKey,
  MessageType,
  Observation,
} from "../contract.ts";
import * as grouping from "../fixtures/grouping.ts";
import {
  aboutEntry,
  addBlocker,
  categoryCounts,
  clusterFoci,
  clusterGroups,
  defaultFilter,
  type FindingFilter,
  filterText,
  groupFindings,
  keyText,
  kindWords,
  rangeWords,
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

function key(
  kind: MessageType,
  certificate: string,
  analyzer = "A",
): FindingKey {
  return {
    database: "db",
    analyzer: { name: analyzer, version: "1", configuration: "{}" },
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
    analyzer = "A",
  }: {
    sticky?: boolean;
    subjects?: EntryId[];
    focus?: EntryId | null;
    message?: string;
    analyzer?: string;
  } = {},
): Observation {
  const diagnostic = { subjects, focus, message, sticky };
  return {
    key: key(kind, certificate, analyzer),
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
      group.selected.map((o) => rangeWords(o.evaluated_view, "9").text),
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
    ["held", ["since 3"]],
    ["again", ["since 7"]],
    ["sticky", ["at 4"]],
    ["past", ["5–6"]],
  ]);
});

test("history shows every observation in first-emission order", () => {
  assert.deepEqual(shown(mixed, [], { selection: "history" }), [
    ["init", ["initialization"]],
    ["held", ["since 3"]],
    ["resolved", ["2–5"]],
    ["again", ["2–4", "since 7"]],
    ["sticky", ["at 4"]],
    ["past", ["5–6"]],
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
  assert.equal(unconfirmed(sticky, "9"), true);
  assert.equal(unconfirmed(held, "9"), false);
  const atEnd = observation("warning", "s", prefixes("9"), { sticky: true });
  assert.equal(unconfirmed(atEnd, "9"), false);
  assert.equal(unconfirmed(atEnd, "10"), true);
  const init = observation("warning", "i", initialization, { sticky: true });
  assert.equal(unconfirmed(init, "9"), false);
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
    {
      ...record,
      key: {
        ...record.key,
        analyzer: { ...record.key.analyzer, name: "B" },
      },
    },
    { ...record, key: { ...record.key, kind: "suggestion" } },
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

test("subjects are listed without the focus, which is named once", () => {
  assert.deepEqual(shownSubjects(id("1"), [id("1")], sameId), []);
  assert.deepEqual(shownSubjects(id("1"), [id("1"), id("1")], sameId), []);
  assert.deepEqual(shownSubjects(id("1"), [id("1"), id("2")], sameId), [
    id("2"),
  ]);
  assert.deepEqual(shownSubjects(id("2"), [id("1"), id("3")], sameId), [
    id("1"),
    id("3"),
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

test("ranges read against the boundary, and ended says only not emitted there", () => {
  assert.deepEqual(rangeWords(initialization, "9"), {
    text: "initialization",
    ended: false,
  });
  assert.deepEqual(rangeWords(prefixes("3", "9"), "9"), {
    text: "since 3",
    ended: false,
  });
  assert.deepEqual(rangeWords(prefixes("9"), "9"), {
    text: "since 9",
    ended: false,
  });
  assert.deepEqual(rangeWords(prefixes("3", "8"), "9"), {
    text: "3–8",
    ended: true,
  });
  assert.deepEqual(rangeWords(prefixes("4"), "9"), {
    text: "at 4",
    ended: true,
  });
  // The same range against a later boundary, as when history grew.
  assert.deepEqual(rangeWords(prefixes("3", "9"), "10"), {
    text: "3–9",
    ended: true,
  });
});

test("clusters gather one analyzer's findings of one kind and message", () => {
  const analysis = output([
    observation("warning", "a", prefixes("9"), {
      message: "m",
      focus: id("1"),
    }),
    observation("warning", "b", prefixes("9"), { message: "n" }),
    observation("warning", "c", prefixes("9"), {
      message: "m",
      focus: id("2"),
    }),
    // Same message, another analyzer or kind: other clusters.
    observation("warning", "d", prefixes("9"), { message: "m", analyzer: "B" }),
    observation("suggestion", "e", prefixes("9"), { message: "m" }),
    // A key whose wording changed: clustered by its newest selected one.
    observation("warning", "f", prefixes("2", "4"), { message: "old" }),
    observation("warning", "f", prefixes("6", "9"), { message: "m" }),
  ]);
  const groups = groupFindings(analysis, [], {
    ...defaultFilter,
    selection: "history",
  });
  assert.deepEqual(
    clusterGroups(groups).map((cluster) => [
      cluster.analyzer.name,
      cluster.kind,
      cluster.message,
      cluster.groups.map((group) => group.key.analyzer_certificate),
    ]),
    [
      ["A", "warning", "m", ["a", "c", "f"]],
      ["A", "warning", "n", ["b"]],
      ["B", "warning", "m", ["d"]],
      ["A", "suggestion", "m", ["e"]],
    ],
  );
});

test("a cluster head names its first distinct foci and counts the findings they leave out", () => {
  const alike = (certificate: string, focus: EntryId | null) =>
    observation("warning", certificate, prefixes("9"), { message: "m", focus });
  const head = (observations: Observation[], limit?: number) => {
    const [cluster] = clusterGroups(
      groupFindings(output(observations), [], defaultFilter),
    );
    assert.ok(cluster);
    const { foci, rest } = clusterFoci(cluster, limit);
    return [foci.map((focus) => focus.position), rest];
  };
  // A lead without a focus is skipped and counted with the rest.
  assert.deepEqual(
    head([
      alike("a", null),
      alike("b", id("1")),
      alike("c", id("2")),
      alike("d", id("3")),
      alike("e", id("4")),
    ]),
    [["1", "2", "3"], 2],
  );
  // A focus already named names its other findings too.
  assert.deepEqual(
    head([alike("a", id("1")), alike("b", id("1")), alike("c", id("2"))]),
    [["1", "2"], 0],
  );
  assert.deepEqual(head([alike("a", null), alike("b", null)]), [[], 2]);
  assert.deepEqual(head([alike("a", id("1")), alike("b", id("2"))], 1), [
    ["1"],
    1,
  ]);
});

// The core's `AnalysisOutput::groups` of a recorded run under every report
// option, which the client's grouping and dismissal lookup must reproduce.
test("grouping and dismissal lookup match the core's for every report option", () => {
  const run: AnalysisRun = grouping.response.result;
  const { analysis, dismissals } = run;
  const indices = (observations: readonly Observation[]) =>
    observations.map((o) => analysis.observations.indexOf(o));
  for (const report of grouping.reports) {
    const { selection, advisories, include_dismissed } = report;
    const groups = groupFindings(analysis, dismissals, {
      selection,
      warning: advisories === "all" || advisories === "warnings",
      suggestion: advisories === "all" || advisories === "suggestions",
      dismissed: include_dismissed,
    });
    assert.deepEqual(
      groups.map((group) => ({
        observations: indices(group.observations),
        selected: indices(group.selected),
        dismissal:
          group.dismissal === null ? null : dismissals.indexOf(group.dismissal),
      })),
      report.groups,
      `${selection}, ${advisories}, dismissed ${include_dismissed}`,
    );
  }
  // What the fixture is there to hold, so that changing it cannot quietly
  // stop pinning a case: a key observed twice whose current report selects
  // one observation, an advisory only history selects, records hidden by
  // default and shown on request, an inconsistency that ended before the
  // boundary, and keys that read alike.
  const report = (filter: Partial<FindingFilter>) =>
    groupFindings(analysis, dismissals, { ...defaultFilter, ...filter });
  const current = report({});
  const ended = (o: Observation) =>
    rangeWords(o.evaluated_view, analysis.view.end_exclusive).ended;
  assert.ok(
    current.some((g) => g.observations.length > 1 && g.selected.length === 1),
  );
  assert.ok(report({ selection: "history" }).length > current.length);
  assert.ok(current.every((g) => g.dismissal === null));
  assert.ok(report({ dismissed: true }).some((g) => g.dismissal !== null));
  assert.ok(
    current.some(
      (g) => g.key.kind === "inconsistency" && g.selected.every(ended),
    ),
  );
  assert.ok(clusterGroups(current).some((c) => c.groups.length > 1));
});

test("the text filter reads messages and focus labels, never hiding inconsistencies", () => {
  const analysis = output([
    observation("warning", "a", prefixes("9"), {
      message: "Field owner missing",
      focus: id("1"),
    }),
    observation("warning", "b", prefixes("9"), {
      message: "other",
      focus: id("2"),
    }),
    observation("inconsistency", "c", prefixes("5"), { message: "bad" }),
  ]);
  const groups = groupFindings(analysis, [], defaultFilter);
  const label = (entry: EntryId) =>
    entry.position === "2" ? "Hinge OWNER" : `@${entry.position}`;
  const kept = (text: string) =>
    filterText(groups, text, label).map((g) => g.key.analyzer_certificate);
  assert.deepEqual(kept(""), ["a", "b", "c"]);
  assert.deepEqual(kept("  "), ["a", "b", "c"]);
  assert.deepEqual(kept("owner"), ["a", "b", "c"]);
  assert.deepEqual(kept("MISSING"), ["a", "c"]);
  assert.deepEqual(kept("hinge"), ["b", "c"]);
  assert.deepEqual(kept("nothing"), ["c"]);
});

test("suggested actions can be added only from a whole, fresh run at its boundary", () => {
  const atEnd = observation("suggestion", "s", prefixes("4", "9"));
  const earlier = observation("suggestion", "s", prefixes("4", "8"));
  const init = observation("suggestion", "s", initialization);
  const analysis = output([atEnd]);
  const fresh = { whole: true, current: true };
  assert.equal(addBlocker(atEnd, analysis, fresh), null);
  assert.match(addBlocker(earlier, analysis, fresh) ?? "", /not emitted/);
  assert.match(addBlocker(init, analysis, fresh) ?? "", /not emitted/);
  assert.match(
    addBlocker(atEnd, analysis, { whole: false, current: true }) ?? "",
    /\[0, 9\) only/,
  );
  assert.match(
    addBlocker(atEnd, analysis, { whole: true, current: false }) ?? "",
    /reloaded/,
  );
});
