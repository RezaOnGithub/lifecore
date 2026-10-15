import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProposalEntry, TypeSchema, TypeSummary } from "../contract.ts";
import {
  addedNodeType,
  allEdgeLabels,
  earlierNodes,
  edgeTypeSuggestions,
  fieldRows,
  freshFieldName,
  moveItem,
  operationNameError,
  parseTags,
  renameField,
  typeNames,
  withField,
  withoutDecl,
  withoutField,
} from "./helpers.ts";

const id = { origin: "db", position: "3" };
const operations: ProposalEntry[] = [
  { name: "task_1", kind: "node", ty_name: "Task", payload: {} },
  {
    name: "link_1",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "task_1" },
    to: { existing: id },
  },
  { name: "note_1", kind: "node", ty_name: "Note", payload: {} },
  { name: "task_2", kind: "node", ty_name: "Task", payload: {} },
];

const task: TypeSchema = {
  name: "Task",
  fields: {},
  outgoing: ["DependsOn"],
  incoming: ["DependsOn", "Mentions"],
  successors: [],
};
const note: TypeSchema = {
  name: "Note",
  fields: {},
  outgoing: ["Mentions"],
  incoming: [],
  successors: [],
};
const types: TypeSummary[] = [
  {
    name: "Task",
    resolution: { kind: "declared", declaration: id, schema: task },
    nodes: { current: "0", historical: "0" },
  },
  {
    name: "Note",
    resolution: { kind: "declared", declaration: id, schema: note },
    nodes: { current: "0", historical: "0" },
  },
  {
    name: "Type",
    resolution: { kind: "builtin" },
    nodes: { current: "2", historical: "0" },
  },
  {
    name: "Ghost",
    resolution: { kind: "missing" },
    nodes: { current: "1", historical: "0" },
  },
];

test("operation names must be well formed and unique among the others", () => {
  assert.equal(operationNameError("task_1", operations, 0), null);
  assert.equal(
    operationNameError("task_1", operations, 2),
    "another operation is named task_1",
  );
  assert.equal(operationNameError("fresh", operations, 2), null);
  assert.equal(operationNameError("", operations, 2), "name required");
  assert.notEqual(operationNameError("1x", operations, 2), null);
});

test("only earlier node operations can be named by added", () => {
  assert.deepEqual(
    earlierNodes(operations, 3).map((operation) => operation.name),
    ["task_1", "note_1"],
  );
  assert.deepEqual(earlierNodes(operations, 0), []);
  assert.deepEqual(
    earlierNodes(operations, 1).map((operation) => operation.name),
    ["task_1"],
  );
  const earlier = earlierNodes(operations, 3);
  assert.equal(addedNodeType({ added: "note_1" }, earlier), "Note");
  assert.equal(addedNodeType({ added: "task_2" }, earlier), null);
  assert.equal(addedNodeType({ existing: id }, earlier), null);
});

test("edge type suggestions follow the known endpoint types", () => {
  assert.deepEqual(allEdgeLabels(types), ["DependsOn", "Mentions"]);
  assert.deepEqual(allEdgeLabels(null), []);
  assert.deepEqual(edgeTypeSuggestions(task, null, types), ["DependsOn"]);
  assert.deepEqual(edgeTypeSuggestions(null, task, types), [
    "DependsOn",
    "Mentions",
  ]);
  assert.deepEqual(edgeTypeSuggestions(note, task, types), [
    "DependsOn",
    "Mentions",
  ]);
  // Nothing known about either end: every declared label.
  assert.deepEqual(edgeTypeSuggestions(null, null, types), [
    "DependsOn",
    "Mentions",
  ]);
  // Known ends that permit nothing fall back the same way.
  assert.deepEqual(edgeTypeSuggestions(null, note, types), [
    "DependsOn",
    "Mentions",
  ]);
});

test("type names keep the server's order and always include Type", () => {
  assert.deepEqual(typeNames(types), ["Task", "Note", "Type", "Ghost"]);
  assert.deepEqual(typeNames(null), ["Type"]);
  assert.deepEqual(typeNames([]), ["Type"]);
});

test("field rows: declared by status then name, then undeclared by name", () => {
  const fields = {
    zeta: { status: "optional" as const, shape: { kind: "text" as const } },
    title: { status: "mandatory" as const, shape: { kind: "text" as const } },
    old: { status: "deprecated" as const, shape: { kind: "text" as const } },
    hint: { status: "encouraged" as const, shape: { kind: "text" as const } },
  };
  const rows = fieldRows(fields, { zeta: "z", extra: "e", also: "a" });
  assert.deepEqual(
    rows.map((row) => [row.field, row.decl?.status ?? null]),
    [
      ["title", "mandatory"],
      ["hint", "encouraged"],
      ["zeta", "optional"],
      ["old", "deprecated"],
      ["also", null],
      ["extra", null],
    ],
  );
  assert.deepEqual(
    fieldRows(null, { b: "1", a: "2" }).map((row) => row.field),
    ["a", "b"],
  );
  assert.deepEqual(fieldRows(null, {}), []);
});

test("body and declaration edits keep order and drop keys cleanly", () => {
  const body = { a: "1", b: "2" };
  assert.deepEqual(Object.keys(withField(body, "c", "3")), ["a", "b", "c"]);
  assert.deepEqual(withField(body, "a", "x"), { a: "x", b: "2" });
  assert.deepEqual(withoutField(body, "a"), { b: "2" });
  assert.equal("a" in withoutField(body, "a"), false);
  const text = {
    status: "optional" as const,
    shape: { kind: "text" as const },
  };
  const decls = { a: text, b: text, c: text };
  assert.deepEqual(Object.keys(renameField(decls, "b", "x")), ["a", "x", "c"]);
  assert.deepEqual(Object.keys(withoutDecl(decls, "b")), ["a", "c"]);
  assert.equal(freshFieldName(decls), "field_1");
  assert.equal(freshFieldName({ field_1: text, field_2: text }), "field_3");
});

test("list items move within bounds", () => {
  assert.deepEqual(moveItem(["a", "b", "c"], 0, 1), ["b", "a", "c"]);
  assert.deepEqual(moveItem(["a", "b", "c"], 2, 1), ["a", "c", "b"]);
  assert.deepEqual(moveItem(["a", "b", "c"], 0, -1), ["a", "b", "c"]);
  assert.deepEqual(moveItem(["a", "b", "c"], 2, 3), ["a", "b", "c"]);
});

test("tags parse to a canonical name list", () => {
  assert.deepEqual(parseTags("Mentions, DependsOn,,  Mentions ,"), [
    "DependsOn",
    "Mentions",
  ]);
  assert.deepEqual(parseTags(""), []);
  assert.deepEqual(parseTags(" , "), []);
});
