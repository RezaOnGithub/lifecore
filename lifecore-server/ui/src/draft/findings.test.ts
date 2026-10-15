import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  EntryId,
  EvaluatedView,
  Observation,
  ProposalReport,
} from "../contract.ts";
import { attachFindings, countFindings, statusWords } from "./findings.ts";

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

// Base [0, 4): operations a, b, c get positions 4, 5, 6.
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
      // Held since prefix [0, 2): already in history, about an existing entry.
      observation("warning", prefixes("2", "7"), [id("1")], id("1")),
      // First emitted at [0, 5): the newest entry is 4, operation a.
      observation("inconsistency", prefixes("5"), [id("4")], id("4")),
      // First emitted at [0, 7): operation c, pointing back at b and at 1.
      observation("suggestion", prefixes("7"), [id("5"), id("1")], id("6")),
      observation("warning", prefixes("7"), [], null),
      // Exactly the base boundary: the newest entry 3 is history.
      observation("warning", prefixes("4", "7"), [id("3")], null),
      // Past the plan: cannot be placed.
      observation("warning", prefixes("9"), [], null),
    ],
  },
};

test("findings attach to the operation of their first prefix", () => {
  const attachment = attachFindings(report);
  assert.deepEqual([...attachment.operations.keys()], ["a", "b", "c"]);
  assert.equal(attachment.initialization.length, 1);
  assert.equal(attachment.history.length, 2);
  assert.equal(attachment.unplaced.length, 1);
  assert.equal(attachment.operations.get("a")?.length, 1);
  assert.equal(attachment.operations.get("b")?.length, 0);
  assert.equal(attachment.operations.get("c")?.length, 2);
  assert.equal(
    attachment.operations.get("a")?.[0]?.observation.diagnostic.kind,
    "inconsistency",
  );
});

test("provisional ids map back to operation names, existing ones stay ids", () => {
  const attachment = attachFindings(report);
  const suggestion = attachment.operations.get("c")?.[0];
  assert.deepEqual(suggestion?.focus, { kind: "operation", name: "c" });
  assert.deepEqual(suggestion?.subjects, [
    { kind: "operation", name: "b" },
    { kind: "existing", id: id("1") },
  ]);
  const history = attachment.history[0];
  assert.deepEqual(history?.focus, { kind: "existing", id: id("1") });
  assert.equal(attachment.operations.get("c")?.[1]?.focus, null);
});

test("status words count inconsistencies and warnings, not suggestions", () => {
  const attachment = attachFindings(report);
  const c = attachment.operations.get("c") ?? [];
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
  assert.equal(
    statusWords({ inconsistency: 1, warning: 0, suggestion: 5 }),
    "1 inconsistency",
  );
});

test("a report without operations only has history and initialization", () => {
  const attachment = attachFindings({
    ...report,
    operations: [],
    analysis: {
      ...report.analysis,
      observations: [
        observation("warning", prefixes("1"), [], null),
        observation("warning", prefixes("5"), [], null),
      ],
    },
  });
  assert.equal(attachment.operations.size, 0);
  assert.equal(attachment.history.length, 1);
  assert.equal(attachment.unplaced.length, 1);
});
