import assert from "node:assert/strict";
import { test } from "node:test";
import type { Body, SchemaError, TypeSchema } from "../contract.ts";
import { declarations } from "../fixtures/codec.ts";
import {
  response as listed,
  unresolvedResponse,
} from "../fixtures/types.list.ts";
import {
  declarationBody,
  declarationForForm,
  declarationSummary,
  MAX_SHAPE_DEPTH,
  PRESENCE_WORDS,
  parseDeclaration,
  readDeclaration,
  schemaErrorText,
  serverReadings,
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
    declarationSummary(readDeclaration(task)),
    "3 fields; out DependsOn; in DependsOn, Mentions; successors none",
  );
  assert.equal(
    declarationSummary(readDeclaration({})),
    "unreadable declaration: name: required but missing",
  );
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
    error: {
      path: ["name"],
      kind: "wrong_shape",
      expected: "text",
      actual: "list",
    },
  });
});

test("keys outside the five are reported, not dropped", () => {
  // In code point order, the first being the one the server reports.
  assert.deepEqual(readDeclaration({ name: "X", outgoin: ["A"], extra: "" }), {
    kind: "unknown_keys",
    keys: ["extra", "outgoin"],
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

function malformedAt(words: unknown): SchemaError | null {
  const reading = readDeclaration({
    name: "X",
    fields: { kind: "record", value: { a: words as never } },
  });
  assert.equal(reading.kind, "malformed", JSON.stringify(words));
  return reading.kind === "malformed" ? reading.error : null;
}

const at = (...rest: (string | number)[]) => ["fields", "a", ...rest];
const integerWord = { kind: "integer", value: "1" } as const;

test("malformed word lists are reported at the offending position", () => {
  assert.deepEqual(malformedAt([]), {
    path: at(0),
    kind: "missing_presence",
  });
  assert.deepEqual(malformedAt(["text"]), {
    path: at(0),
    kind: "unknown_presence",
    word: "text",
  });
  assert.deepEqual(malformedAt(["optional"]), {
    path: at(1),
    kind: "missing_shape",
  });
  assert.deepEqual(malformedAt(["optional", "list"]), {
    path: at(2),
    kind: "missing_shape",
  });
  assert.deepEqual(malformedAt(["optional", integerWord]), {
    path: at(1),
    kind: "wrong_shape",
    expected: "text",
    actual: "integer",
  });
  assert.deepEqual(malformedAt([["optional"], "text"])?.path, at(0));
  assert.deepEqual(malformedAt(["optional", "txt"]), {
    path: at(1),
    kind: "unknown_shape",
    word: "txt",
  });
  assert.deepEqual(malformedAt(["optional", "text", "text"]), {
    path: at(2),
    kind: "trailing_elements",
  });
  assert.deepEqual(malformedAt(["optional", "record"]), {
    path: at(2),
    kind: "missing_record_fields",
  });
  assert.deepEqual(malformedAt(["optional", "record", integerWord]), {
    path: at(2),
    kind: "wrong_shape",
    expected: "record",
    actual: "integer",
  });
  assert.deepEqual(malformedAt(["optional", "record", ["mandatory", "text"]]), {
    path: at(2),
    kind: "wrong_shape",
    expected: "record",
    actual: "list",
  });
  assert.deepEqual(
    malformedAt(["optional", "record", { kind: "record", value: {} }, "text"]),
    { path: at(3), kind: "trailing_elements" },
  );
  assert.deepEqual(
    malformedAt([
      "optional",
      "list",
      "record",
      { kind: "record", value: { b: ["mandatory", "lst"] } },
    ]),
    { path: at(3, "b", 1), kind: "unknown_shape", word: "lst" },
  );
  assert.deepEqual(malformedAt("text"), {
    path: at(),
    kind: "wrong_shape",
    expected: "list",
    actual: "text",
  });
  assert.deepEqual(readDeclaration({ name: "X", fields: [] }), {
    kind: "malformed",
    error: {
      path: ["fields"],
      kind: "wrong_shape",
      expected: "record",
      actual: "list",
    },
  });
});

test("declaration errors read as a path and the client's words", () => {
  const text = (error: SchemaError) => schemaErrorText(error);
  assert.equal(
    text({ path: ["name"], kind: "missing_field" }),
    "name: required but missing",
  );
  assert.equal(
    text({ path: ["colour"], kind: "unknown_field" }),
    "colour: not a declaration key; a declaration has name, fields, outgoing, incoming, successors",
  );
  assert.equal(
    text({
      path: ["fields", "a.b", 1],
      kind: "wrong_shape",
      expected: "text",
      actual: "integer",
    }),
    'fields."a.b"[1]: expected text, found an integer',
  );
  assert.equal(
    text({ path: at(0), kind: "unknown_presence", word: "sometimes" }),
    `fields.a[0]: unknown presence "sometimes"; expected ${PRESENCE_WORDS.join(", ")}`,
  );
  assert.equal(
    text({ path: at(65), kind: "too_deep" }),
    "fields.a[65]: more than 64 nested list and record words",
  );
  assert.equal(
    text({ path: ["outgoing", 2], kind: "duplicate_permission", name: "A" }),
    'outgoing[2]: "A" is repeated',
  );
  assert.equal(
    text({ path: [], kind: "trailing_elements" }),
    "unexpected element after the shape",
  );
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
    path: at(65),
    kind: "too_deep",
  });
  // 63 lists, then a record whose field adds one more `list`: 65 in all.
  assert.deepEqual(
    malformedAt([
      "optional",
      ...nestedLists(63).slice(0, 63),
      "record",
      { kind: "record", value: { g: ["mandatory", "list", "text"] } },
    ]),
    { path: at(65, "g", 1), kind: "too_deep" },
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
    error: { path: ["outgoing", 2], kind: "duplicate_permission", name: "A" },
  });
  assert.equal(parseDeclaration({ name: "X", incoming: ["A", "A"] }), null);
  assert.deepEqual(
    readDeclaration({
      name: "X",
      successors: ["A", { kind: "integer", value: "1" }],
    }),
    {
      kind: "malformed",
      error: {
        path: ["successors", 1],
        kind: "wrong_shape",
        expected: "text",
        actual: "integer",
      },
    },
  );
  assert.deepEqual(readDeclaration({ name: "X", outgoing: "A" }), {
    kind: "malformed",
    error: {
      path: ["outgoing"],
      kind: "wrong_shape",
      expected: "list",
      actual: "text",
    },
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
    declarationSummary(readDeclaration({ name: "X", outgoing: ["A", "A"] })),
    'unreadable declaration: outgoing[1]: "A" is repeated',
  );
  assert.equal(
    declarationSummary(readDeclaration({ name: "X", other: "", more: "" })),
    "unreadable declaration: more, other are not keys",
  );
});

// The fixture test records each payload with `Type::from_body`'s reading.
test("readDeclaration reads every recorded payload as the server does", () => {
  for (const fixture of declarations) {
    const reading = readDeclaration(fixture.payload);
    const label = JSON.stringify(fixture.payload);
    if ("schema" in fixture) {
      assert.deepEqual(reading, { kind: "ok", schema: fixture.schema }, label);
    } else if (reading.kind === "unknown_keys") {
      // The client names every unknown key; the server reports the first.
      assert.deepEqual(
        fixture.error,
        { path: reading.keys.slice(0, 1), kind: "unknown_field" },
        label,
      );
    } else {
      assert.deepEqual(
        reading,
        { kind: "malformed", error: fixture.error },
        label,
      );
    }
  }
});

test("the server's readings cover each claimant of a listed name", () => {
  const kinds = (types: Parameters<typeof serverReadings>[0]) =>
    [...serverReadings(types).values()].map((reading) => reading.kind).sort();
  assert.deepEqual(kinds(listed.result.types), ["ok"]);
  // Three `Note` claimants, one malformed, and a malformed `Tag`.
  assert.deepEqual(kinds(unresolvedResponse.result.types), [
    "malformed",
    "malformed",
    "ok",
    "ok",
  ]);
  assert.deepEqual(kinds(null), []);
});
