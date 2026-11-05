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

test("the new kinds format as their canonical strings and words", () => {
  assert.equal(
    formatPayload({ kind: "duration", value: "PT1H30M" }),
    "PT1H30M",
  );
  assert.equal(
    formatPayload({ kind: "date", value: "2026-11-06" }),
    "2026-11-06",
  );
  assert.equal(
    formatPayload({
      kind: "offset_timestamp",
      value: "2026-10-29T08:15:00+01:00",
    }),
    "2026-10-29T08:15:00+01:00",
  );
  assert.equal(
    formatPayload({
      kind: "variant",
      tag: "committed",
      value: { cost: { kind: "duration", value: "PT2H" } },
    }),
    "committed {cost: PT2H}",
  );
  assert.equal(
    formatPayload({ kind: "variant", tag: "resumed", value: {} }),
    "resumed {}",
  );
  assert.equal(shapeWords({ kind: "offset_timestamp" }), "offset_timestamp");
  assert.equal(
    shapeWords({ kind: "enum", members: ["day", "week"] }),
    "enum day | week",
  );
  assert.equal(
    shapeWords({
      kind: "variant",
      tags: {
        committed: {
          cost: { status: "mandatory", shape: { kind: "duration" } },
        },
        resumed: {},
      },
    }),
    "variant committed {cost: duration} | resumed {}",
  );
});
