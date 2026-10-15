import assert from "node:assert/strict";
import { test } from "node:test";
import type { Body, TypeSchema } from "../contract.ts";
import {
  declarationBody,
  declarationForForm,
  declarationSummary,
  MAX_SHAPE_DEPTH,
  PRESENCE_WORDS,
  parseDeclaration,
  readDeclaration,
  shapeDepth,
} from "./schema.ts";

const task: Body = {
  name: "Task",
  fields: {
    kind: "record",
    value: {
      title: ["mandatory", "text"],
      tags: ["optional", "list", "text"],
      details: [
        "optional",
        "record",
        { kind: "record", value: { owner: ["mandatory", "text"] } },
      ],
    },
  },
  outgoing: ["DependsOn"],
  incoming: ["Mentions", "DependsOn"],
  successors: [],
};

test("a declaration payload parses into the structured schema", () => {
  assert.deepEqual(parseDeclaration(task), {
    name: "Task",
    fields: {
      title: { status: "mandatory", shape: { kind: "text" } },
      tags: {
        status: "optional",
        shape: { kind: "list", element: { kind: "text" } },
      },
      details: {
        status: "optional",
        shape: {
          kind: "record",
          fields: { owner: { status: "mandatory", shape: { kind: "text" } } },
        },
      },
    },
    outgoing: ["DependsOn"],
    incoming: ["DependsOn", "Mentions"],
    successors: [],
  });
});

test("omitted collections are empty and unreadable payloads are null", () => {
  assert.deepEqual(parseDeclaration({ name: "Bare" }), {
    name: "Bare",
    fields: {},
    outgoing: [],
    incoming: [],
    successors: [],
  });
  assert.equal(parseDeclaration({}), null);
  assert.equal(
    parseDeclaration({
      name: "X",
      fields: { kind: "record", value: { a: ["sometimes", "text"] } },
    }),
    null,
  );
  assert.equal(
    parseDeclaration({
      name: "X",
      fields: { kind: "record", value: { a: ["optional", "text", "text"] } },
    }),
    null,
  );
  assert.equal(
    parseDeclaration({
      name: "X",
      fields: { kind: "record", value: { a: ["optional", "record"] } },
    }),
    null,
  );
  assert.equal(parseDeclaration({ name: "X", outgoing: "DependsOn" }), null);
});

test("fields given as a list are unreadable; an empty name still parses", () => {
  assert.equal(
    parseDeclaration({ name: "X", fields: ["optional", "text"] }),
    null,
  );
  assert.deepEqual(parseDeclaration({ name: "" }), {
    name: "",
    fields: {},
    outgoing: [],
    incoming: [],
    successors: [],
  });
});

test("the Type table summarizes a declaration", () => {
  assert.equal(
    declarationSummary(task),
    "3 fields; out DependsOn; in DependsOn, Mentions; successors none",
  );
  assert.equal(declarationSummary({}), "unreadable declaration");
});

const roundTrips: [string, TypeSchema][] = [
  [
    "all four presences on scalar shapes",
    {
      name: "Scalars",
      fields: {
        a: { status: "mandatory", shape: { kind: "text" } },
        b: { status: "encouraged", shape: { kind: "integer" } },
        c: { status: "deprecated", shape: { kind: "boolean" } },
        d: { status: "optional", shape: { kind: "timestamp" } },
      },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  ],
  [
    "a list of lists",
    {
      name: "Matrix",
      fields: {
        cells: {
          status: "optional",
          shape: {
            kind: "list",
            element: { kind: "list", element: { kind: "integer" } },
          },
        },
      },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  ],
  [
    "a record in a list",
    {
      name: "Team",
      fields: {
        members: {
          status: "mandatory",
          shape: {
            kind: "list",
            element: {
              kind: "record",
              fields: {
                name: { status: "mandatory", shape: { kind: "text" } },
                since: { status: "optional", shape: { kind: "timestamp" } },
              },
            },
          },
        },
      },
      outgoing: ["Leads"],
      incoming: ["Joins", "Leads"],
      successors: ["Squad"],
    },
  ],
  [
    "a record with a nested record",
    {
      name: "Nested",
      fields: {
        details: {
          status: "optional",
          shape: {
            kind: "record",
            fields: {
              owner: { status: "mandatory", shape: { kind: "text" } },
              address: {
                status: "encouraged",
                shape: {
                  kind: "record",
                  fields: {
                    lines: {
                      status: "optional",
                      shape: { kind: "list", element: { kind: "text" } },
                    },
                  },
                },
              },
            },
          },
        },
      },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  ],
  [
    "empty fields and permissions",
    { name: "Bare", fields: {}, outgoing: [], incoming: [], successors: [] },
  ],
  [
    "a record inside a list inside a record",
    {
      name: "Project",
      fields: {
        plan: {
          status: "encouraged",
          shape: {
            kind: "record",
            fields: {
              steps: {
                status: "mandatory",
                shape: {
                  kind: "list",
                  element: {
                    kind: "record",
                    fields: {
                      title: { status: "mandatory", shape: { kind: "text" } },
                      due: { status: "optional", shape: { kind: "timestamp" } },
                    },
                  },
                },
              },
            },
          },
        },
      },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  ],
  [
    "prototype names as fields",
    {
      name: "Odd",
      fields: JSON.parse(
        '{"__proto__":{"status":"optional","shape":{"kind":"text"}},"constructor":{"status":"mandatory","shape":{"kind":"record","fields":{"toString":{"status":"optional","shape":{"kind":"integer"}}}}}}',
      ),
      outgoing: [],
      incoming: [],
      successors: [],
    },
  ],
  [
    "an empty nested record",
    {
      name: "Hollow",
      fields: {
        shell: { status: "optional", shape: { kind: "record", fields: {} } },
      },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  ],
];

for (const [title, schema] of roundTrips) {
  test(`declarationBody round-trips ${title}`, () => {
    const body = declarationBody(schema);
    assert.deepEqual(parseDeclaration(body), schema);
    // The text survives a JSON round trip unchanged, as a proposal would.
    assert.deepEqual(
      parseDeclaration(JSON.parse(JSON.stringify(body))),
      schema,
    );
  });
}

test("declarationBody writes exactly the FILES §2.6 words", () => {
  assert.deepEqual(
    declarationBody({
      name: "Task",
      fields: {
        title: { status: "mandatory", shape: { kind: "text" } },
        tags: {
          status: "optional",
          shape: { kind: "list", element: { kind: "text" } },
        },
        cells: {
          status: "optional",
          shape: {
            kind: "list",
            element: { kind: "list", element: { kind: "integer" } },
          },
        },
        details: {
          status: "optional",
          shape: {
            kind: "record",
            fields: { owner: { status: "mandatory", shape: { kind: "text" } } },
          },
        },
      },
      outgoing: ["DependsOn"],
      incoming: ["DependsOn"],
      successors: ["Note"],
    }),
    {
      name: "Task",
      fields: {
        kind: "record",
        value: {
          title: ["mandatory", "text"],
          tags: ["optional", "list", "text"],
          cells: ["optional", "list", "list", "integer"],
          details: [
            "optional",
            "record",
            { kind: "record", value: { owner: ["mandatory", "text"] } },
          ],
        },
      },
      outgoing: ["DependsOn"],
      incoming: ["DependsOn"],
      successors: ["Note"],
    },
  );
  // Nothing declared still writes all five keys.
  assert.deepEqual(
    declarationBody({
      name: "",
      fields: {},
      outgoing: [],
      incoming: [],
      successors: [],
    }),
    {
      name: "",
      fields: { kind: "record", value: {} },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  );
});

test("declarationBody sorts and deduplicates permissions", () => {
  const body = declarationBody({
    name: "X",
    fields: {},
    outgoing: ["b", "a", "b"],
    incoming: [],
    successors: [],
  });
  assert.deepEqual(body.outgoing, ["a", "b"]);
});

test("the server's parsed schema reproduces its declaration", () => {
  const schema = parseDeclaration(task);
  assert.notEqual(schema, null);
  if (schema !== null) {
    assert.deepEqual(declarationBody(schema), {
      ...task,
      incoming: ["DependsOn", "Mentions"],
    });
  }
});

test("a new Type node's empty payload opens in the form", () => {
  // `parseDeclaration` keeps the server's view: no name, no declaration.
  assert.equal(parseDeclaration({}), null);
  const reading = declarationForForm({});
  assert.deepEqual(reading, {
    kind: "ok",
    schema: {
      name: "",
      fields: {},
      outgoing: [],
      incoming: [],
      successors: [],
    },
  });
  // The first edit writes all five keys.
  if (reading.kind === "ok") {
    assert.deepEqual(declarationBody({ ...reading.schema, name: "Task" }), {
      name: "Task",
      fields: { kind: "record", value: {} },
      outgoing: [],
      incoming: [],
      successors: [],
    });
  }
  // Other keys still read without a name.
  assert.deepEqual(declarationForForm({ outgoing: ["A"] }), {
    kind: "ok",
    schema: {
      name: "",
      fields: {},
      outgoing: ["A"],
      incoming: [],
      successors: [],
    },
  });
  // A name that is there is read as it is.
  assert.deepEqual(declarationForForm({ name: ["X"] }), {
    kind: "malformed",
    path: "name",
    message: "expected text, found list",
  });
});

test("keys outside the five are reported, not dropped", () => {
  assert.deepEqual(readDeclaration({ name: "X", outgoin: ["A"], extra: "" }), {
    kind: "unknown_keys",
    keys: ["outgoin", "extra"],
  });
  assert.equal(parseDeclaration({ name: "X", outgoin: ["A"] }), null);
  assert.deepEqual(declarationForForm({ outgoin: [] }), {
    kind: "unknown_keys",
    keys: ["outgoin"],
  });
  assert.deepEqual(readDeclaration(JSON.parse('{"name":"X","__proto__":[]}')), {
    kind: "unknown_keys",
    keys: ["__proto__"],
  });
});

function malformedAt(words: unknown): { path: string; message: string } {
  const reading = readDeclaration({
    name: "X",
    fields: { kind: "record", value: { a: words as never } },
  });
  assert.equal(reading.kind, "malformed", JSON.stringify(words));
  return reading.kind === "malformed"
    ? { path: reading.path, message: reading.message }
    : { path: "", message: "" };
}

test("malformed word lists are reported at the offending position", () => {
  assert.deepEqual(malformedAt([]), {
    path: "fields.a[0]",
    message: "missing a presence word",
  });
  assert.deepEqual(malformedAt(["text"]), {
    path: "fields.a[0]",
    message: `unknown presence "text"; expected ${PRESENCE_WORDS.join(", ")}`,
  });
  assert.deepEqual(malformedAt(["optional"]), {
    path: "fields.a[1]",
    message: "missing a shape word",
  });
  assert.deepEqual(malformedAt(["optional", "list"]), {
    path: "fields.a[2]",
    message: "missing a shape word",
  });
  assert.deepEqual(malformedAt(["optional", { kind: "integer", value: "1" }]), {
    path: "fields.a[1]",
    message: "expected a shape word, found integer",
  });
  assert.equal(malformedAt([["optional"], "text"]).path, "fields.a[0]");
  assert.deepEqual(malformedAt(["optional", "txt"]), {
    path: "fields.a[1]",
    message:
      'unknown shape word "txt"; expected text, integer, boolean, timestamp, list, or record',
  });
  assert.deepEqual(malformedAt(["optional", "text", "text"]), {
    path: "fields.a[2]",
    message: "unexpected element after the shape",
  });
  assert.deepEqual(malformedAt(["optional", "record"]), {
    path: "fields.a[2]",
    message:
      "record must be followed by a record envelope of field declarations",
  });
  assert.deepEqual(
    malformedAt(["optional", "record", { kind: "integer", value: "1" }]),
    {
      path: "fields.a[2]",
      message: "expected a record envelope, found integer",
    },
  );
  assert.deepEqual(malformedAt(["optional", "record", ["mandatory", "text"]]), {
    path: "fields.a[2]",
    message: "expected a record envelope, found list",
  });
  assert.deepEqual(
    malformedAt(["optional", "record", { kind: "record", value: {} }, "text"]),
    { path: "fields.a[3]", message: "unexpected element after the shape" },
  );
  assert.deepEqual(
    malformedAt([
      "optional",
      "list",
      "record",
      { kind: "record", value: { b: ["mandatory", "lst"] } },
    ]),
    {
      path: "fields.a[3].b[1]",
      message:
        'unknown shape word "lst"; expected text, integer, boolean, timestamp, list, or record',
    },
  );
  assert.deepEqual(malformedAt("text"), {
    path: "fields.a",
    message: 'expected a list of words, found text "text"',
  });
  assert.deepEqual(readDeclaration({ name: "X", fields: [] }), {
    kind: "malformed",
    path: "fields",
    message: "expected a record envelope, found list",
  });
});

function nestedLists(count: number): unknown[] {
  return [...Array.from({ length: count }, () => "list"), "text"];
}

test("at most 64 nested list and record words, counted across records", () => {
  assert.equal(MAX_SHAPE_DEPTH, 64);
  const deepest = readDeclaration({
    name: "X",
    fields: {
      kind: "record",
      value: { f: ["optional", ...nestedLists(64)] as never },
    },
  });
  assert.equal(deepest.kind, "ok");
  if (deepest.kind === "ok") {
    const shape = deepest.schema.fields.f?.shape;
    assert.ok(shape !== undefined);
    assert.equal(shapeDepth(shape), 64);
    assert.deepEqual(
      parseDeclaration(declarationBody(deepest.schema)),
      deepest.schema,
    );
  }
  assert.deepEqual(malformedAt(["optional", ...nestedLists(65)]), {
    path: "fields.a[65]",
    message: "more than 64 nested list and record words",
  });
  // 63 lists, then a record whose field adds one more `list`: 65 in all.
  assert.deepEqual(
    malformedAt([
      "optional",
      ...nestedLists(63).slice(0, 63),
      "record",
      { kind: "record", value: { g: ["mandatory", "list", "text"] } },
    ]),
    {
      path: "fields.a[65].g[1]",
      message: "more than 64 nested list and record words",
    },
  );
  assert.equal(
    shapeDepth({
      kind: "record",
      fields: {
        a: { status: "optional", shape: { kind: "text" } },
        b: {
          status: "optional",
          shape: { kind: "list", element: { kind: "record", fields: {} } },
        },
      },
    }),
    3,
  );
  assert.equal(shapeDepth({ kind: "integer" }), 0);
});

test("permission lists: text only, no repeats, code point order", () => {
  assert.deepEqual(readDeclaration({ name: "X", outgoing: ["A", "B", "A"] }), {
    kind: "malformed",
    path: "outgoing[2]",
    message: '"A" is repeated',
  });
  assert.equal(parseDeclaration({ name: "X", incoming: ["A", "A"] }), null);
  assert.deepEqual(
    readDeclaration({
      name: "X",
      successors: ["A", { kind: "integer", value: "1" }],
    }),
    {
      kind: "malformed",
      path: "successors[1]",
      message: "expected text, found integer",
    },
  );
  assert.deepEqual(readDeclaration({ name: "X", outgoing: "A" }), {
    kind: "malformed",
    path: "outgoing",
    message: 'expected a list, found text "A"',
  });
  const schema = parseDeclaration({
    name: "X",
    outgoing: ["\u{1f600}", "\uff61", "b"],
  });
  assert.deepEqual(schema?.outgoing, ["b", "\uff61", "\u{1f600}"]);
  assert.deepEqual(
    declarationBody({
      name: "X",
      fields: {},
      outgoing: ["\u{1f600}", "\uff61", "\u{1f600}"],
      incoming: [],
      successors: [],
    }).outgoing,
    ["\uff61", "\u{1f600}"],
  );
});

test("the Type table summarizes what the server would reject as unreadable", () => {
  assert.equal(
    declarationSummary({ name: "X", outgoing: ["A", "A"] }),
    "unreadable declaration",
  );
  assert.equal(
    declarationSummary({ name: "X", other: "" }),
    "unreadable declaration",
  );
});
