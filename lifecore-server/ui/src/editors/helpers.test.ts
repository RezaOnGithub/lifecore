import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  ProposalEntry,
  Shape,
  TypeSchema,
  TypeSummary,
} from "../contract.ts";
import {
  addedNodeType,
  allEdgeLabels,
  earlierNodes,
  edgeTypeSuggestions,
  fieldRows,
  fitKeys,
  initialShape,
  isUnpicked,
  lineCount,
  liveDraft,
  moveItem,
  namedKeys,
  nextKey,
  operationNameError,
  parseTags,
  renameField,
  renameKey,
  sameReference,
  shapeOfKind,
  typeNames,
  withField,
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

const blocked: TypeSchema = {
  name: "Wall",
  fields: {},
  outgoing: [],
  incoming: [],
  successors: [],
};

test("edge type suggestions intersect what the known ends permit", () => {
  assert.deepEqual(allEdgeLabels(types), ["DependsOn", "Mentions"]);
  assert.deepEqual(allEdgeLabels(null), []);
  // One end known: its own set.
  assert.deepEqual(edgeTypeSuggestions(task, null, types), {
    labels: ["DependsOn"],
    note: null,
  });
  assert.deepEqual(edgeTypeSuggestions(null, task, types), {
    labels: ["DependsOn", "Mentions"],
    note: null,
  });
  // Both known: Note permits Mentions outgoing, Task both incoming.
  assert.deepEqual(edgeTypeSuggestions(note, task, types), {
    labels: ["Mentions"],
    note: null,
  });
  assert.deepEqual(edgeTypeSuggestions(task, task, types), {
    labels: ["DependsOn"],
    note: null,
  });
  // Both known and nothing in common.
  assert.deepEqual(
    edgeTypeSuggestions(
      { ...task, outgoing: ["Blocks"] },
      { ...note, incoming: ["Mentions"] },
      types,
    ),
    {
      labels: [],
      note: "no label is both outgoing from Task and incoming to Note",
    },
  );
  // Nothing known about either end: every declared label.
  assert.deepEqual(edgeTypeSuggestions(null, null, types), {
    labels: ["DependsOn", "Mentions"],
    note: null,
  });
  // A known end that permits nothing says so instead of offering everything.
  assert.deepEqual(edgeTypeSuggestions(null, note, types), {
    labels: [],
    note: "Note permits no incoming edges",
  });
  assert.deepEqual(edgeTypeSuggestions(blocked, task, types), {
    labels: [],
    note: "Wall permits no outgoing edges",
  });
  assert.deepEqual(edgeTypeSuggestions(task, blocked, types), {
    labels: [],
    note: "Wall permits no incoming edges",
  });
});

test("references: unpicked placeholders and equal ends", () => {
  const unset = { existing: { origin: "", position: "" } };
  assert.equal(isUnpicked(unset), true);
  assert.equal(isUnpicked({ existing: id }), false);
  assert.equal(isUnpicked({ added: "x" }), false);
  assert.equal(sameReference({ added: "a" }, { added: "a" }), true);
  assert.equal(sameReference({ added: "a" }, { added: "b" }), false);
  assert.equal(sameReference({ existing: id }, { existing: { ...id } }), true);
  assert.equal(
    sameReference({ existing: id }, { existing: { ...id, position: "4" } }),
    false,
  );
  assert.equal(sameReference({ existing: id }, { added: "3" }), false);
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

test("field rows treat prototype names as ordinary fields", () => {
  const text = {
    status: "optional" as const,
    shape: { kind: "text" as const },
  };
  const fields = JSON.parse(
    `{"constructor":${JSON.stringify(text)},"__proto__":${JSON.stringify(text)}}`,
  );
  const body = JSON.parse(
    '{"toString":"t","__proto__":"p","hasOwnProperty":"h"}',
  );
  assert.deepEqual(
    fieldRows(fields, body).map((row) => [row.field, row.decl !== null]),
    [
      ["__proto__", true],
      ["constructor", true],
      ["hasOwnProperty", false],
      ["toString", false],
    ],
  );
  // No schema field is inherited: `toString` in the body is undeclared
  // even against an empty declaration.
  assert.deepEqual(
    fieldRows({}, { toString: "t" }).map((row) => row.decl),
    [null],
  );
});

test("body and declaration edits keep order and drop keys cleanly", () => {
  const body = { a: "1", b: "2" };
  assert.deepEqual(Object.keys(withField(body, "c", "3")), ["a", "b", "c"]);
  assert.deepEqual(withField(body, "a", "x"), { a: "x", b: "2" });
  assert.deepEqual(Object.keys(withField(body, "a", "x")), ["a", "b"]);
  assert.deepEqual(withoutField(body, "a"), { b: "2" });
  assert.equal("a" in withoutField(body, "a"), false);
  const text = {
    status: "optional" as const,
    shape: { kind: "text" as const },
  };
  const integer = {
    status: "mandatory" as const,
    shape: { kind: "integer" as const },
  };
  const decls = { a: text, b: integer, c: text };
  assert.deepEqual(Object.keys(renameField(decls, "b", "x")), ["a", "x", "c"]);
  assert.equal(renameField(decls, "b", "x").x, integer);
  assert.deepEqual(Object.keys(withoutField(decls, "b")), ["a", "c"]);
});

test("renaming onto a declared name is refused, not an overwrite", () => {
  const text = {
    status: "optional" as const,
    shape: { kind: "text" as const },
  };
  const integer = {
    status: "mandatory" as const,
    shape: { kind: "integer" as const },
  };
  const decls = { a: text, b: integer };
  assert.equal(renameField(decls, "a", "b"), decls);
  assert.deepEqual(decls, { a: text, b: integer });
  // A name that is not declared, or no change, leaves them as they are.
  assert.equal(renameField(decls, "z", "y"), decls);
  assert.equal(renameField(decls, "a", "a"), decls);
  // A number-like name is an ordinary name, although JavaScript lists
  // such keys first; the row keys follow the name, not the place.
  const renamed = renameField(decls, "b", "1");
  assert.deepEqual(renamed, { 1: integer, a: text });
  assert.equal(Object.hasOwn(renamed, "b"), false);
  // Prototype names become own fields.
  const odd = renameField(decls, "a", "__proto__");
  assert.deepEqual(Object.keys(odd), ["__proto__", "b"]);
  assert.equal(Object.getPrototypeOf(odd), Object.prototype);
  assert.equal(Object.getOwnPropertyDescriptor(odd, "__proto__")?.value, text);
  const withProto = withField({ a: "x" }, "__proto__", "y");
  assert.deepEqual(Object.keys(withProto), ["a", "__proto__"]);
  assert.equal(Object.getPrototypeOf(withProto), Object.prototype);
  assert.deepEqual(Object.keys(withoutField(withProto, "a")), ["__proto__"]);
});

test("list items move within bounds", () => {
  assert.deepEqual(moveItem(["a", "b", "c"], 0, 1), ["b", "a", "c"]);
  assert.deepEqual(moveItem(["a", "b", "c"], 2, 1), ["a", "c", "b"]);
  assert.deepEqual(moveItem(["a", "b", "c"], 0, -1), ["a", "b", "c"]);
  assert.deepEqual(moveItem(["a", "b", "c"], 2, 3), ["a", "b", "c"]);
});

test("tags parse to a canonical name list and report repeats", () => {
  assert.deepEqual(parseTags("Mentions, DependsOn,,  Mentions ,"), {
    tags: ["DependsOn", "Mentions"],
    repeated: ["Mentions"],
  });
  assert.deepEqual(parseTags(""), { tags: [], repeated: [] });
  assert.deepEqual(parseTags(" , "), { tags: [], repeated: [] });
  assert.deepEqual(parseTags("\u{1f600}, \uff61, b").tags, [
    "b",
    "\uff61",
    "\u{1f600}",
  ]);
});

test("draft text shows only over the value it was typed over", () => {
  const draft = { text: "12a", base: "12" };
  assert.equal(liveDraft(draft, "12"), draft);
  assert.equal(liveDraft(draft, "13"), null);
  assert.equal(liveDraft(null, "12"), null);
  // A pending value: nothing committed yet.
  const pending = { text: "-", base: null };
  assert.equal(liveDraft(pending, null), pending);
  assert.equal(liveDraft(pending, "5"), null);
  assert.equal(lineCount(""), 1);
  assert.equal(lineCount("a\nb\n"), 3);
});

test("row keys follow moves, removals, and renames", () => {
  assert.deepEqual(fitKeys([], 3), [0, 1, 2]);
  assert.deepEqual(fitKeys([4, 2], 4), [4, 2, 5, 6]);
  assert.deepEqual(fitKeys([4, 2, 7], 2), [4, 2]);
  assert.equal(nextKey([]), 0);
  assert.equal(nextKey([3, 1]), 4);
  // A move carries the key with the element.
  assert.deepEqual(moveItem(fitKeys([], 3), 0, 2), [1, 2, 0]);
  const named = namedKeys(["a", "b"], new Map());
  assert.deepEqual(
    [...named],
    [
      ["a", 0],
      ["b", 1],
    ],
  );
  // A rename keeps the key; a new name gets a fresh one.
  const renamed = renameKey(named, "b", "1");
  assert.deepEqual(
    [...namedKeys(["1", "a", "c"], renamed)],
    [
      ["1", 1],
      ["a", 0],
      ["c", 2],
    ],
  );
  // A removed name's key is not reused while others hold higher ones.
  assert.deepEqual([...namedKeys(["b"], named)], [["b", 1]]);
});

test("shape changes keep nested declarations where they can", () => {
  const record: Shape = {
    kind: "record",
    fields: { owner: { status: "mandatory", shape: { kind: "text" } } },
  };
  // record → list wraps it, so a list of records keeps the fields.
  const list = shapeOfKind("list", record);
  assert.deepEqual(list, { kind: "list", element: record });
  // list of record → record unwraps it again.
  assert.equal(shapeOfKind("record", list), record);
  // list of integer → integer unwraps; → text replaces.
  const integers: Shape = { kind: "list", element: { kind: "integer" } };
  assert.deepEqual(shapeOfKind("integer", integers), { kind: "integer" });
  assert.deepEqual(shapeOfKind("text", integers), { kind: "text" });
  // A list of lists unwraps one level.
  const matrix: Shape = { kind: "list", element: integers };
  assert.equal(shapeOfKind("list", matrix), matrix);
  assert.deepEqual(shapeOfKind("list", { kind: "text" }), {
    kind: "list",
    element: { kind: "text" },
  });
  assert.deepEqual(shapeOfKind("record", { kind: "text" }), {
    kind: "record",
    fields: {},
  });
  assert.deepEqual(initialShape("list"), {
    kind: "list",
    element: { kind: "text" },
  });
  assert.deepEqual(initialShape("record"), { kind: "record", fields: {} });
  assert.deepEqual(initialShape("boolean"), { kind: "boolean" });
});
