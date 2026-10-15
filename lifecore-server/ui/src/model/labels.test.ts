import assert from "node:assert/strict";
import { test } from "node:test";
import type { NodeEntry } from "../contract.ts";
import { entryKey, labelField, rowLabel, sameId, typeName } from "./labels.ts";

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

test("entry keys and identity comparison", () => {
  const id = { origin: "o", position: "7" };
  assert.equal(entryKey(id), "o:7");
  assert.ok(sameId(id, { origin: "o", position: "7" }));
  assert.ok(!sameId(id, { origin: "o", position: "8" }));
});
