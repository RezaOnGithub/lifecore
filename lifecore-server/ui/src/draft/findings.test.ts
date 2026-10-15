import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  EntryId,
  EvaluatedView,
  Observation,
  ProposalReport,
} from "../contract.ts";
import {
  attachFindings,
  countFindings,
  countWords,
  reportCounts,
  statusWords,
} from "./findings.ts";

const id = (position: string): EntryId => ({ origin: "o", position });
const view = (end_exclusive: string) => ({
  database: "db",
  end_exclusive,
  head: null,
});

function observation(
  kind: "inconsistency" | "warning" | "suggestion",
  evaluated_view: EvaluatedView,
  subjects: EntryId[],
  focus: EntryId | null,
): Observation {
  const diagnostic = { subjects, focus, message: kind, sticky: false };
  return {
    key: {
      database: "db",
      analyzer: { name: "A", version: "1", configuration: "{}" },
      kind,
      analyzer_certificate: kind,
    },
    evaluated_view,
    diagnostic:
      kind === "suggestion"
        ? { ...diagnostic, kind, actions: [] }
        : { ...diagnostic, kind },
  };
}

const prefixes = (first: string, last = first): EvaluatedView => ({
  kind: "prefixes",
  first: view(first),
  last: view(last),
});

// Base [0, 4): operations a, b, c get positions 4, 5, 6; the candidate's
// head is 7.
const report: ProposalReport = {
  database: "db",
  base_revision: "r",
  base: view("4"),
  operations: [
    { name: "a", entry: id("4") },
    { name: "b", entry: id("5") },
    { name: "c", entry: id("6") },
  ],
  analysis: {
    view: view("7"),
    inputs: { evaluation_time: null },
    implementations: [],
    observations: [
      observation("warning", { kind: "initialization" }, [], null),
      // Held since prefix [0, 2) up to the head: in history, still current.
      observation("warning", prefixes("2", "7"), [id("1")], id("1")),
      // First emitted at [0, 5): the newest entry is 4, operation a.
      observation("inconsistency", prefixes("5", "7"), [id("4")], id("4")),
      // First emitted at [0, 7): operation c, pointing back at b and at 1.
      observation("suggestion", prefixes("7"), [id("5"), id("1")], id("6")),
      observation("warning", prefixes("7"), [], null),
      // Exactly the base boundary: the newest entry 3 is history; resolved
      // by the draft at [0, 6).
      observation("warning", prefixes("4", "5"), [id("3")], null),
      // Resolved before the base: not listed.
      observation("warning", prefixes("1", "3"), [], null),
      // Past the plan: cannot be placed.
      observation("warning", prefixes("9"), [], null),
    ],
  },
};

test("findings attach by operation index, matched by entry position", () => {
  const attachment = attachFindings(report);
  assert.deepEqual([...attachment.operations.keys()].sort(), [0, 2]);
  assert.equal(attachment.initialization.length, 1);
  assert.equal(attachment.history.length, 2);
  assert.equal(attachment.settled, 1);
  assert.equal(attachment.unplaced.length, 1);
  assert.equal(attachment.operations.get(0)?.length, 1);
  assert.equal(attachment.operations.get(1), undefined);
  assert.equal(attachment.operations.get(2)?.length, 2);
  assert.equal(
    attachment.operations.get(0)?.[0]?.observation.diagnostic.kind,
    "inconsistency",
  );
});

test("positions decide, not the order of the report's operations", () => {
  const shuffled = attachFindings({
    ...report,
    operations: [...report.operations].reverse(),
  });
  assert.equal(
    shuffled.operations.get(0)?.[0]?.observation.diagnostic.kind,
    "inconsistency",
  );
  // An operation the report does not list at that position is unplaced.
  const gap = attachFindings({
    ...report,
    operations: report.operations.filter((op) => op.name !== "a"),
  });
  assert.equal(gap.operations.get(0), undefined);
  assert.equal(gap.unplaced.length, 2);
});

test("each finding carries its prefix range and whether the draft resolved it", () => {
  const attachment = attachFindings(report);
  const [kept, resolved] = attachment.history;
  assert.deepEqual(kept?.range, { first: "2", last: "7" });
  assert.equal(kept?.resolvedLater, false);
  assert.deepEqual(resolved?.range, { first: "4", last: "5" });
  assert.equal(resolved?.resolvedLater, true);
  assert.equal(attachment.initialization[0]?.range, null);
  assert.equal(attachment.operations.get(0)?.[0]?.resolvedLater, false);
});

test("provisional ids map back to operation indexes, existing ones stay ids", () => {
  const attachment = attachFindings(report);
  const suggestion = attachment.operations.get(2)?.[0];
  assert.deepEqual(suggestion?.focus, { kind: "operation", index: 2 });
  assert.deepEqual(suggestion?.subjects, [
    { kind: "operation", index: 1 },
    { kind: "existing", id: id("1") },
  ]);
  assert.deepEqual(attachment.history[0]?.focus, {
    kind: "existing",
    id: id("1"),
  });
});

test("the attachment of a report is computed once", () => {
  assert.equal(attachFindings(report), attachFindings(report));
  assert.notEqual(attachFindings({ ...report }), attachFindings(report));
});

test("status words count inconsistencies and warnings, not suggestions", () => {
  const c = attachFindings(report).operations.get(2) ?? [];
  assert.deepEqual(countFindings(c), {
    inconsistency: 0,
    warning: 1,
    suggestion: 1,
  });
  assert.equal(statusWords(countFindings(c)), "1 warning");
  assert.equal(statusWords(countFindings([])), "ok");
  assert.equal(
    statusWords({ inconsistency: 2, warning: 3, suggestion: 0 }),
    "2 inconsistencies, 3 warnings",
  );
  assert.deepEqual(reportCounts(report), {
    total: { inconsistency: 1, warning: 6, suggestion: 1 },
    introduced: { inconsistency: 1, warning: 1, suggestion: 1 },
  });
  assert.equal(
    countWords({ inconsistency: 1, warning: 0, suggestion: 2 }),
    "1 inconsistency, 0 warnings, 2 suggestions",
  );
});
