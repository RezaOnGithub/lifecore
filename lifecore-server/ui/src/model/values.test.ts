import assert from "node:assert/strict";
import { test } from "node:test";
import {
  decimalError,
  defaultValue,
  isDecimal,
  kindOf,
  matchesShape,
  nameError,
  nowTimestamp,
  timestampError,
  validateCanonicalPayload,
  validateCanonicalValue,
} from "./values.ts";

test("canonical decimals: one spelling per value, within i64", () => {
  for (const ok of [
    "0",
    "7",
    "-7",
    "9223372036854775807",
    "-9223372036854775808",
  ]) {
    assert.equal(decimalError(ok), null, ok);
    assert.ok(isDecimal(ok));
  }
  for (const bad of [
    "",
    "-0",
    "007",
    "+1",
    " 1",
    "1 ",
    "1.0",
    "1e3",
    "0x10",
    "abc",
    "9223372036854775808",
    "-9223372036854775809",
  ]) {
    assert.notEqual(decimalError(bad), null, bad);
    assert.ok(!isDecimal(bad));
  }
  assert.equal(
    decimalError("9223372036854775808"),
    "outside the signed 64-bit range",
  );
});

test("timestamps: RFC 3339 with up to nine fraction digits, Z or offset", () => {
  for (const ok of [
    "2026-10-15T08:30:00Z",
    "2026-10-15T08:30:00.1Z",
    "2026-10-15T08:30:00.123456789Z",
    "2026-10-15T08:30:00+05:30",
    "2026-10-15T08:30:00.5-07:00",
    "2024-02-29T23:59:60Z",
    "2026-10-15t08:30:00z",
    "2026-10-15 08:30:00Z",
  ]) {
    assert.equal(timestampError(ok), null, ok);
  }
  for (const bad of [
    "",
    "2026-10-15",
    "2026-10-15T08:30Z",
    "2026-10-15T08:30:00",
    "2026-10-15T08:30:00.1234567890Z",
    "2026-13-01T00:00:00Z",
    "2026-02-29T00:00:00Z",
    "2026-10-15T24:00:00Z",
    "2026-10-15T08:60:00Z",
    "2026-10-15T08:30:61Z",
    "2026-10-15T08:30:00+24:00",
    "2026-10-15T08:30:00+05:60",
    "now",
  ]) {
    assert.notEqual(timestampError(bad), null, bad);
  }
  assert.equal(
    timestampError("2026-13-01T00:00:00Z"),
    "month 13 is out of range",
  );
});

test("nowTimestamp prints the instant Date knows, at millisecond precision", () => {
  const instant = nowTimestamp(new Date(Date.UTC(2026, 9, 15, 8, 30, 0, 123)));
  assert.equal(instant, "2026-10-15T08:30:00.123Z");
  assert.equal(timestampError(instant), null);
  assert.equal(timestampError(nowTimestamp()), null);
});

test("operation names", () => {
  assert.equal(nameError("task_1"), null);
  assert.equal(nameError("_"), null);
  assert.equal(nameError("A-b"), null);
  assert.equal(nameError(""), "name required");
  assert.notEqual(nameError("1a"), null);
  assert.notEqual(nameError("a b"), null);
  assert.notEqual(nameError("-a"), null);
  assert.notEqual(nameError("é"), null);
});

test("payload kinds and defaults", () => {
  assert.equal(kindOf("x"), "text");
  assert.equal(kindOf([]), "list");
  assert.equal(kindOf({ kind: "integer", value: "1" }), "integer");
  assert.equal(kindOf({ kind: "record", value: {} }), "record");
  assert.ok(matchesShape([], { kind: "list", element: { kind: "text" } }));
  assert.ok(!matchesShape("1", { kind: "integer" }));
  const now = () => "2026-10-15T08:30:00Z";
  assert.equal(defaultValue({ kind: "text" }, now), "");
  assert.deepEqual(defaultValue({ kind: "integer" }, now), {
    kind: "integer",
    value: "0",
  });
  assert.deepEqual(defaultValue({ kind: "boolean" }, now), {
    kind: "boolean",
    value: false,
  });
  assert.deepEqual(defaultValue({ kind: "timestamp" }, now), {
    kind: "timestamp",
    value: "2026-10-15T08:30:00Z",
  });
  assert.deepEqual(
    defaultValue({ kind: "list", element: { kind: "text" } }, now),
    [],
  );
  assert.deepEqual(
    defaultValue(
      {
        kind: "record",
        fields: {
          owner: { status: "mandatory", shape: { kind: "text" } },
          note: { status: "encouraged", shape: { kind: "text" } },
          when: { status: "mandatory", shape: { kind: "timestamp" } },
        },
      },
      now,
    ),
    {
      kind: "record",
      value: { owner: "", when: { kind: "timestamp", value: now() } },
    },
  );
});

test("a canonical payload round-trips through validation", () => {
  const text = JSON.stringify({
    title: "Submit report",
    priority: { kind: "integer", value: "9223372036854775807" },
    labels: ["work", ["nested"]],
    deadline: { kind: "timestamp", value: "2026-09-30T17:00:00Z" },
    completed: { kind: "boolean", value: false },
    details: {
      kind: "record",
      value: { owner: "Teto", n: { kind: "integer", value: "-1" } },
    },
    empty: { kind: "record", value: {} },
    none: [],
  });
  const result = validateCanonicalPayload(text);
  assert.ok("payload" in result);
  assert.equal(JSON.stringify(result.payload), text);
  // A big integer survives as text, untouched by Number.
  assert.equal(
    (result.payload.priority as { value: string }).value,
    "9223372036854775807",
  );
});

function errorOf(text: string): string {
  const result = validateCanonicalPayload(text);
  assert.ok("error" in result, `expected an error for ${text}`);
  return result.error;
}

test("raw payload text is rejected unless canonical", () => {
  assert.match(errorOf("{"), /^invalid JSON/);
  assert.equal(errorOf("[]"), "a payload is a JSON object of fields");
  assert.equal(errorOf('"text"'), "a payload is a JSON object of fields");
  assert.equal(errorOf("null"), "a payload is a JSON object of fields");
  assert.equal(
    errorOf('{"priority": 2}'),
    'priority: bare numbers are not values; integers must be {"kind":"integer","value":"42"}',
  );
  assert.match(errorOf('{"a": {"kind":"record","value":{"n": 1}}}'), /^a\.n: /);
  assert.match(errorOf('{"a": ["x", 1]}'), /^a\[1\]: bare numbers/);
  assert.match(
    errorOf('{"a": true}'),
    /booleans must be {"kind":"boolean","value":true}/,
  );
  assert.match(errorOf('{"a": null}'), /^a: null is not a value/);
  assert.match(errorOf('{"a": {}}'), /^a: an object must be an envelope/);
  assert.match(
    errorOf('{"a": {"owner": "x"}}'),
    /a record is {"kind":"record","value":{…}}/,
  );
  assert.match(
    errorOf('{"a": {"kind":"float","value":"1"}}'),
    /unknown kind "float"/,
  );
  assert.match(
    errorOf('{"a": {"kind":"integer"}}'),
    /exactly "kind" and "value"/,
  );
  assert.match(
    errorOf('{"a": {"kind":"integer","value":"1","x":1}}'),
    /exactly "kind" and "value"/,
  );
  assert.match(
    errorOf('{"a": {"kind":"integer","value":1}}'),
    /decimal string/,
  );
  assert.match(
    errorOf('{"a": {"kind":"integer","value":"01"}}'),
    /^a: not a canonical decimal/,
  );
  assert.match(
    errorOf('{"a": {"kind":"integer","value":"9223372036854775808"}}'),
    /64-bit/,
  );
  assert.match(
    errorOf('{"a": {"kind":"boolean","value":"true"}}'),
    /true or false/,
  );
  assert.match(
    errorOf('{"a": {"kind":"timestamp","value":"yesterday"}}'),
    /RFC 3339/,
  );
  assert.match(errorOf('{"a": {"kind":"timestamp","value":1}}'), /is a string/);
  assert.match(
    errorOf('{"a": {"kind":"record","value":[]}}'),
    /object of fields/,
  );
  assert.match(
    errorOf('{"a": {"kind":"record","value":null}}'),
    /object of fields/,
  );
});

test("a single value validates with its own path", () => {
  assert.deepEqual(validateCanonicalValue('"x"', "f"), { value: "x" });
  assert.deepEqual(validateCanonicalValue("[]", "f"), { value: [] });
  assert.deepEqual(
    validateCanonicalValue('{"kind":"integer","value":"3"}', "f"),
    {
      value: { kind: "integer", value: "3" },
    },
  );
  const bare = validateCanonicalValue("3", "f");
  assert.ok("error" in bare);
  assert.match(bare.error, /^f: bare numbers/);
  const nested = validateCanonicalValue(
    '[{"kind":"record","value":{"n":null}}]',
    "f",
  );
  assert.ok("error" in nested);
  assert.match(nested.error, /^f\[0\]\.n: null/);
  const broken = validateCanonicalValue("{", "f");
  assert.ok("error" in broken);
  assert.match(broken.error, /^invalid JSON/);
});
