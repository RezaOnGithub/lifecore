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
