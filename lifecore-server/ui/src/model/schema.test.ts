import assert from "node:assert/strict";
import { test } from "node:test";
import type { Body } from "../contract.ts";
import { declarationSummary, parseDeclaration } from "./schema.ts";

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

test("the Type table summarizes a declaration", () => {
  assert.equal(
    declarationSummary(task),
    "3 fields; out DependsOn; in DependsOn, Mentions; successors none",
  );
  assert.equal(declarationSummary({}), "unreadable declaration");
});
