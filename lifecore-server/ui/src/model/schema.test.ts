import assert from "node:assert/strict";
import { test } from "node:test";
import type { Body, TypeSchema } from "../contract.ts";
import {
  declarationBody,
  declarationSummary,
  parseDeclaration,
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
