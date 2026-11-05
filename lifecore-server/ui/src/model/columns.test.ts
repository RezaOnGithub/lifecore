import assert from "node:assert/strict";
import { test } from "node:test";
import type { Payload, TypeSchema } from "../contract.ts";
import { compareCells, fieldColumns, sortedBy, toggleSort } from "./columns.ts";

const schema: TypeSchema = {
  name: "Task",
  fields: {
    tags: { status: "optional", shape: { kind: "text" } },
    title: { status: "mandatory", shape: { kind: "text" } },
    legacy: { status: "deprecated", shape: { kind: "text" } },
    notes: { status: "encouraged", shape: { kind: "text" } },
    due: { status: "optional", shape: { kind: "timestamp" } },
  },
  outgoing: [],
  incoming: [],
  successors: [],
};

test("columns follow status order, then name, then undeclared fields", () => {
  const rows = [
    { payload: { zebra: "z", title: "t" } },
    { payload: { extra: "e" } },
  ];
  assert.deepEqual(
    fieldColumns(schema, rows).map((column) => column.field),
    ["title", "notes", "due", "tags", "legacy", "extra", "zebra"],
  );
  assert.deepEqual(fieldColumns(null, rows), [
    { field: "extra" },
    { field: "title" },
    { field: "zebra" },
  ]);
});

test("integers compare by value, other cells as text, absent last", () => {
  const int = (value: string): Payload => ({ kind: "integer", value });
  assert.ok(compareCells(int("9"), int("10")) < 0);
  assert.ok(compareCells(int("-1"), int("99999999999999999999")) < 0);
  assert.ok(compareCells("9", "10") > 0);
  assert.ok(compareCells(int("9"), "10") > 0);
  assert.ok(compareCells(undefined, "a") > 0);
  assert.equal(compareCells(undefined, undefined), 0);
});

test("timestamps compare by instant although Jiff omits a zero fraction", () => {
  const ts = (value: string): Payload => ({ kind: "timestamp", value });
  // As text `Z` follows `.`, which would put the whole second last.
  assert.ok(
    compareCells(ts("2026-10-15T08:30:00Z"), ts("2026-10-15T08:30:00.5Z")) < 0,
  );
  assert.ok(
    compareCells(
      ts("2026-10-15T08:30:00.000000001Z"),
      ts("2026-10-15T08:30:00Z"),
    ) > 0,
  );
  assert.ok(
    compareCells(ts("2026-10-15T08:30:00.5Z"), ts("2026-10-15T08:30:00.25Z")) >
      0,
  );
  assert.ok(
    compareCells(
      ts("2026-10-15T08:30:00.999999999Z"),
      ts("2026-10-15T08:30:01Z"),
    ) < 0,
  );
  assert.equal(
    compareCells(ts("2026-10-15T08:30:00.10Z"), ts("2026-10-15T08:30:00.1Z")),
    0,
  );
  // A timestamp against another kind still compares as text.
  assert.ok(compareCells(ts("2026-10-15T08:30:00Z"), "2027") < 0);
});

test("booleans sort false before true", () => {
  const bool = (value: boolean): Payload => ({ kind: "boolean", value });
  assert.ok(compareCells(bool(false), bool(true)) < 0);
  assert.equal(compareCells(bool(true), bool(true)), 0);
  assert.deepEqual(
    sortedBy([{ v: true }, { v: false }], (row) => bool(row.v), false).map(
      (row) => row.v,
    ),
    [false, true],
  );
});

test("sorting is stable and reversible", () => {
  const rows = [
    { n: "b", v: "2" },
    { n: "a", v: "1" },
    { n: "c", v: "2" },
  ];
  const cell = (row: { v: string }): Payload => ({
    kind: "integer",
    value: row.v,
  });
  assert.deepEqual(
    sortedBy(rows, cell, false).map((row) => row.n),
    ["a", "b", "c"],
  );
  assert.deepEqual(
    sortedBy(rows, cell, true).map((row) => row.n),
    ["b", "c", "a"],
  );
  assert.deepEqual(toggleSort(null, "v"), { column: "v", descending: false });
  assert.deepEqual(toggleSort({ column: "v", descending: false }, "v"), {
    column: "v",
    descending: true,
  });
  assert.deepEqual(toggleSort({ column: "v", descending: true }, "n"), {
    column: "n",
    descending: false,
  });
});

test("absent cells sort last in both directions", () => {
  const rows = [
    { n: "a", v: undefined },
    { n: "b", v: "2" },
    { n: "c", v: "1" },
  ];
  const cell = (row: { v: string | undefined }) =>
    row.v === undefined
      ? undefined
      : ({ kind: "integer", value: row.v } as const);
  assert.deepEqual(
    sortedBy(rows, cell, false).map((row) => row.n),
    ["c", "b", "a"],
  );
  assert.deepEqual(
    sortedBy(rows, cell, true).map((row) => row.n),
    ["b", "c", "a"],
  );
});

test("durations compare by length, offset timestamps by instant, dates by day", () => {
  const duration = (value: string): Payload => ({ kind: "duration", value });
  // As text `PT1H` would follow `PT10M`... and `PT2H` follow `PT10H`.
  assert.ok(compareCells(duration("PT2H"), duration("PT10H")) < 0);
  assert.ok(compareCells(duration("PT59M"), duration("PT1H")) < 0);
  assert.ok(compareCells(duration("-PT2H"), duration("-PT1H")) < 0);
  assert.ok(compareCells(duration("PT0S"), duration("PT0.000000001S")) < 0);
  assert.ok(compareCells(duration("PT1M0.5S"), duration("PT1M")) > 0);
  assert.equal(compareCells(duration("PT1H"), duration("PT1H")), 0);
  const at = (value: string): Payload => ({ kind: "offset_timestamp", value });
  // Later local time, earlier instant: 08:15+01:00 is 07:15Z.
  assert.ok(
    compareCells(
      at("2026-10-29T08:15:00+01:00"),
      at("2026-10-29T07:30:00+00:00"),
    ) < 0,
  );
  assert.equal(
    compareCells(
      at("2026-10-29T08:15:00+01:00"),
      at("2026-10-29T01:45:00-05:30"),
    ),
    0,
  );
  assert.ok(
    compareCells(
      at("2026-10-29T08:15:00.5+01:00"),
      at("2026-10-29T08:15:00+01:00"),
    ) > 0,
  );
  assert.ok(
    compareCells(
      at("-009999-01-01T00:00:00+00:00"),
      at("0001-01-01T00:00:00+00:00"),
    ) < 0,
  );
  const date = (value: string): Payload => ({ kind: "date", value });
  assert.ok(compareCells(date("2026-02-01"), date("2026-11-06")) < 0);
});
