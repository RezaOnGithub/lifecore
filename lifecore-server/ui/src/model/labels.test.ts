import assert from "node:assert/strict";
import { test } from "node:test";
import type { Body, NodeEntry } from "../contract.ts";
import {
  commonLabelField,
  entryKey,
  labelField,
  pastBoundary,
  rowLabel,
  sameId,
  typeName,
  viewWords,
} from "./labels.ts";

function node(payload: NodeEntry["payload"], ty_name = "Task"): NodeEntry {
  return {
    kind: "node",
    id: { origin: "o", position: "7" },
    ty_name,
    payload,
  };
}

test("title labels a row before name and other text", () => {
  const row = node({ name: "n", title: "t", alpha: "a" });
  assert.equal(labelField(row), "title");
  assert.equal(rowLabel(row), "t");
});

test("name labels a row without a title", () => {
  assert.equal(rowLabel(node({ zeta: "z", name: "Task" }, "Type")), "Task");
});

test("without title or name the alphabetically first text field labels", () => {
  const row = node({
    zeta: "z",
    beta: "b",
    count: { kind: "integer", value: "3" },
  });
  assert.equal(labelField(row), "beta");
  assert.equal(rowLabel(row), "b");
});

test("field names sort by code point, as the server's drawing labels do", () => {
  // UTF-16 units put U+1F600 (D83D DE00) before U+FF5E; code points and
  // UTF-8 bytes put it after.
  const row = node({ "\u{1F600}": "astral", "～": "wide" });
  assert.equal(labelField(row), "～");
  assert.equal(rowLabel(row), "wide");
});

test("blank text is skipped and a row without text falls back", () => {
  assert.equal(rowLabel(node({ title: "  ", alpha: "a" })), "a");
  assert.equal(labelField(node({ n: { kind: "integer", value: "1" } })), null);
  assert.equal(rowLabel(node({})), "Task @7");
});

test("a non-text title is skipped and an empty type name reads (unnamed)", () => {
  assert.equal(
    rowLabel(node({ title: { kind: "integer", value: "1" }, name: "n" })),
    "n",
  );
  assert.equal(labelField(node({ title: ["t"] })), null);
  assert.equal(rowLabel(node({}, "")), "(unnamed) @7");
  assert.equal(typeName(""), "(unnamed)");
  assert.equal(typeName("Task"), "Task");
});

test("an entry is past a boundary at or after it, never in the whole history", () => {
  const id = { origin: "o", position: "7" };
  assert.equal(pastBoundary(id, null), false);
  assert.equal(pastBoundary(id, "8"), false);
  assert.equal(pastBoundary(id, "7"), true);
  assert.equal(pastBoundary(id, "0"), true);
  // Positions compare as integers, not as text.
  assert.equal(pastBoundary({ origin: "o", position: "10" }, "9"), true);
  assert.equal(pastBoundary({ origin: "o", position: "9" }, "10"), false);
});

test("a view reads in words by its entry count", () => {
  assert.equal(viewWords("0"), "the database before its first entry");
  assert.equal(
    viewWords("1"),
    "the database as it was after its first entry, position 0",
  );
  assert.equal(
    viewWords("12"),
    "the database as it was after its first 12 entries, positions 0 to 11",
  );
});

test("entry keys and identity comparison", () => {
  const id = { origin: "o", position: "7" };
  assert.equal(entryKey(id), "o:7");
  assert.ok(sameId(id, { origin: "o", position: "7" }));
  assert.ok(!sameId(id, { origin: "o", position: "8" }));
});

test("the field labelling every row is found only when all rows share it", () => {
  const rows = (...payloads: Body[]) =>
    payloads.map((payload) => ({ payload }));
  assert.equal(commonLabelField(rows({ title: "a" }, { title: "b" })), "title");
  assert.equal(
    commonLabelField(rows({ title: "a" }, { name: "b", title: " " })),
    null,
  );
  assert.equal(commonLabelField(rows({ count: "1" }, {})), null);
  assert.equal(commonLabelField([]), null);
});
