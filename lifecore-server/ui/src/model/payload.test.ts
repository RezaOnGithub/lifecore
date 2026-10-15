import assert from "node:assert/strict";
import { test } from "node:test";
import { formatPayload, shapeWords, truncate } from "./payload.ts";

test("payload values format compactly", () => {
  assert.equal(formatPayload("text"), "text");
  assert.equal(formatPayload({ kind: "integer", value: "-42" }), "-42");
  assert.equal(formatPayload({ kind: "boolean", value: false }), "false");
  assert.equal(
    formatPayload({ kind: "timestamp", value: "2026-10-15T08:30:00Z" }),
    "2026-10-15T08:30:00Z",
  );
  assert.equal(formatPayload(["a", ["b"]]), "[a, [b]]");
  assert.equal(
    formatPayload({
      kind: "record",
      value: { owner: "me", n: { kind: "integer", value: "1" } },
    }),
    "{owner: me, n: 1}",
  );
});

test("empty collections and records inside lists format", () => {
  assert.equal(formatPayload([]), "[]");
  assert.equal(formatPayload({ kind: "record", value: {} }), "{}");
  assert.equal(
    formatPayload([{ kind: "record", value: { a: "1", b: [] } }, "c"]),
    "[{a: 1, b: []}, c]",
  );
});

test("long values truncate with an ellipsis", () => {
  assert.equal(truncate("abcdef", 4), "abc…");
  assert.equal(truncate("abcd", 4), "abcd");
});

test("shapes read as words", () => {
  assert.equal(shapeWords({ kind: "text" }), "text");
  assert.equal(
    shapeWords({
      kind: "list",
      element: { kind: "list", element: { kind: "integer" } },
    }),
    "list of list of integer",
  );
  assert.equal(
    shapeWords({
      kind: "record",
      fields: { owner: { status: "mandatory", shape: { kind: "text" } } },
    }),
    "record {owner: text}",
  );
});
