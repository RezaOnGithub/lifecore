import assert from "node:assert/strict";
import { test } from "node:test";
import { payloadTexts } from "../fixtures/codec.ts";
import {
  compareCodePoints,
  decimalError,
  defaultValue,
  findRepeatedKey,
  getOwn,
  isDecimal,
  kindOf,
  matchesShape,
  nameError,
  nowTimestamp,
  setOwn,
  TIMESTAMP_HINT,
  timestampError,
  timestampHint,
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

// Jiff 0.2.33's own verdict on each string: `serde_json` decoding a
// `jiff::Timestamp`, as the server decodes a timestamp envelope's value.
const JIFF: [string, boolean][] = [
  ["2026-10-15T08:30:00Z", true],
  ["2026-10-15T08:30:00.1Z", true],
  ["2026-10-15T08:30:00.123456789Z", true],
  ["2026-10-15T08:30:00.1234567890Z", false],
  ["2026-10-15T08:30:00.000Z", true],
  ["2026-10-15T08:30:00+05:30", true],
  ["2026-10-15T08:30:00.5-07:00", true],
  ["2024-02-29T23:59:60Z", true],
  ["2026-10-15t08:30:00z", true],
  ["2026-10-15 08:30:00Z", true],
  ["2026-10-15T08:30Z", true],
  ["2026-10-15T08Z", true],
  ["2026-10-15Z", false],
  ["2026-10-15", false],
  ["2026-10-15T08:30:00", false],
  ["2026-10-15T08:30:00+0530", true],
  ["2026-10-15T08:30:00+05", true],
  ["2026-10-15T08:30:00+05:30:15", true],
  ["2026-10-15T08:30:00+05:30:15.5", true],
  ["2026-10-15T08:30:00-00:00", true],
  ["2026-10-15T08:30:00+24:00", true],
  ["2026-10-15T08:30:00+23:59", true],
  ["2026-10-15T08:30:00+25:00", true],
  ["2026-10-15T08:30:00,5Z", true],
  ["2026-10-15T08:30:00Z[UTC]", true],
  ["2026-10-15T08:30:00+03:00[Europe/Istanbul]", true],
  ["2026-10-15T08:30:00Z[!UTC]", true],
  ["2026-10-15T08:30:00Z[u-ca=iso8601]", true],
  ["20261015T083000Z", true],
  ["2026-10-15T083000Z", true],
  ["+002026-10-15T08:30:00Z", true],
  ["-000001-01-01T00:00:00Z", true],
  ["0000-01-01T00:00:00Z", true],
  ["9999-12-30T22:00:00Z", true],
  ["9999-12-31T23:59:59Z", false],
  ["-009999-01-02T01:59:59Z", true],
  ["-009999-01-01T00:00:00Z", false],
  ["2026-10-15T08:30:00.Z", false],
  ["2026-10-15T08:30:60.5Z", true],
  ["2026-12-31T23:59:60Z", true],
  ["2026-02-29T00:00:00Z", false],
  ["2026-10-15T24:00:00Z", false],
  ["2026-W42-4T08:30:00Z", false],
  ["2026-288T08:30:00Z", false],
  [" 2026-10-15T08:30:00Z", false],
  ["2026-10-15T08:30:00Z ", false],
  ["now", false],
  ["yesterday", false],
  ["tomorrow at noon", false],
  ["2026", false],
  ["12:00", false],
  ["08:30:00Z", false],
  ["2026-10-15T08:30:00Z[", false],
  ["1760517000", false],
  ["2026/10/15 08:30:00Z", false],
  ["Oct 15 2026 08:30 UTC", false],
  ["2026-10-15T08:30:00+05:30[Asia/Kolkata]", true],
  ["2026-10-15T08:30:00.000000001Z", true],
  ["1970-01-01T00:00:00Z", true],
  ["2026-10-15T08:30:00.5+00:00", true],
  ["2026-10-15T08:30:00-23:59:59", true],
  ["2026-10-15T08:30:00Z[Europe/Paris]", true],
  ["2026-10-15T08:30:00+01:00[Europe/Paris]", true],
];

test("every timestamp Jiff accepts is committed, in the form and as JSON", () => {
  for (const [text, accepted] of JIFF) {
    if (!accepted) {
      continue;
    }
    assert.equal(timestampError(text), null, text);
    assert.deepEqual(
      validateCanonicalValue(
        JSON.stringify({ kind: "timestamp", value: text }),
      ),
      { value: { kind: "timestamp", value: text } },
      text,
    );
  }
});

test("the timestamp hint flags text that is clearly no timestamp", () => {
  for (const text of [
    "now",
    "yesterday",
    "tomorrow at noon",
    "2026",
    "2026-10-15",
    "12:00",
    "08:30:00Z",
    "1760517000",
    "2026/10/15 08:30:00Z",
    "Oct 15 2026 08:30 UTC",
  ]) {
    assert.equal(
      JIFF.some(([known, accepted]) => known === text && accepted),
      false,
      text,
    );
    assert.equal(timestampHint(text), TIMESTAMP_HINT, text);
    // Advisory only: the text is still committed, and Check reports it.
    assert.equal(timestampError(text), null, text);
  }
  // Empty text is the one thing never committed; its hint suggests a form.
  assert.notEqual(timestampError(""), null);
  assert.match(timestampHint("") ?? "", /2026-10-15T08:30:00Z/);
  const empty = validateCanonicalValue('{"kind":"timestamp","value":""}', "f");
  assert.ok("error" in empty);
});

test("nowTimestamp prints the instant Date knows, at millisecond precision", () => {
  const instant = nowTimestamp(new Date(Date.UTC(2026, 9, 15, 8, 30, 0, 123)));
  assert.equal(instant, "2026-10-15T08:30:00.123Z");
  assert.equal(timestampHint(instant), null);
  assert.equal(timestampHint(nowTimestamp()), null);
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
  assert.equal(defaultValue({ kind: "text" }), "");
  // No neutral integer or instant: the editor waits for one to be typed.
  assert.equal(defaultValue({ kind: "integer" }), null);
  assert.equal(defaultValue({ kind: "timestamp" }), null);
  assert.deepEqual(defaultValue({ kind: "boolean" }), {
    kind: "boolean",
    value: false,
  });
  assert.deepEqual(
    defaultValue({ kind: "list", element: { kind: "text" } }),
    [],
  );
  assert.deepEqual(
    defaultValue({
      kind: "record",
      fields: {
        owner: { status: "mandatory", shape: { kind: "text" } },
        note: { status: "encouraged", shape: { kind: "text" } },
        when: { status: "mandatory", shape: { kind: "timestamp" } },
        count: { status: "mandatory", shape: { kind: "integer" } },
      },
    }),
    { kind: "record", value: { owner: "" } },
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

test("a repeated key is malformed in the body, a record, or an envelope", () => {
  assert.equal(
    errorOf('{"a":"x","b":"y","a":"z"}'),
    'the payload repeats the key "a"; a name may appear once (FILES §2.1)',
  );
  assert.match(
    errorOf('{"r":{"kind":"record","value":{"a":"x","a":"y"}}}'),
    /^r\.value repeats the key "a"/,
  );
  assert.match(
    errorOf('{"n":{"kind":"integer","kind":"integer","value":"1"}}'),
    /^n repeats the key "kind"/,
  );
  assert.match(
    errorOf(
      '{"l":[{"kind":"record","value":{}},{"kind":"boolean","value":true,"value":false}]}',
    ),
    /^l\[1\] repeats the key "value"/,
  );
  // Escapes decode before comparing, so these name the same key.
  assert.match(errorOf('{"a":"x","\\u0061":"y"}'), /repeats the key "a"/);
  const value = validateCanonicalValue(
    '{"kind":"record","value":{"x":"1","x":"2"}}',
    "details",
  );
  assert.ok("error" in value);
  assert.match(value.error, /^details\.value repeats the key "x"/);
});

test("the key scanner ignores braces, quotes, and commas inside strings", () => {
  assert.equal(
    findRepeatedKey('{"a":"} , \\"a\\": {","b":["a","a"],"c":{"a":1}}'),
    null,
  );
  assert.deepEqual(findRepeatedKey('{"a":{"b":1},"c":{"b":1,"b":2}}'), {
    path: "c",
    key: "b",
  });
  assert.deepEqual(findRepeatedKey('[{"x":1},{"x":1,"x":2}]'), {
    path: "[1]",
    key: "x",
  });
  // Siblings in different objects do not collide.
  assert.equal(findRepeatedKey('{"a":{"k":1},"b":{"k":1}}'), null);
  // Not JSON: left for JSON.parse to report.
  assert.equal(findRepeatedKey('{"a":"x'), null);
  assert.match(errorOf('{"a":"x'), /^invalid JSON/);
});

test("prototype names are ordinary fields", () => {
  const text =
    '{"__proto__":"x","constructor":{"kind":"record","value":{"__proto__":"y","toString":"z"}},"hasOwnProperty":["w"]}';
  const result = validateCanonicalPayload(text);
  assert.ok("payload" in result);
  const { payload } = result;
  assert.equal(Object.getPrototypeOf(payload), Object.prototype);
  assert.deepEqual(Object.keys(payload), [
    "__proto__",
    "constructor",
    "hasOwnProperty",
  ]);
  assert.equal(getOwn(payload, "__proto__"), "x");
  assert.equal(JSON.stringify(payload), text);
  assert.deepEqual(payload, JSON.parse(text));
  // Inherited members are not fields.
  assert.equal(getOwn({}, "toString"), undefined);
  assert.equal(getOwn({}, "__proto__"), undefined);
  const body: Record<string, string> = {};
  setOwn(body, "__proto__", "v");
  assert.equal(Object.getPrototypeOf(body), Object.prototype);
  assert.deepEqual(Object.keys(body), ["__proto__"]);
  assert.deepEqual(
    defaultValue({
      kind: "record",
      fields: JSON.parse(
        '{"__proto__":{"status":"mandatory","shape":{"kind":"text"}}}',
      ),
    }),
    { kind: "record", value: JSON.parse('{"__proto__":""}') },
  );
});

test("code point order matches the server's UTF-8 byte order", () => {
  // U+FF61 is one UTF-16 unit above the surrogates; U+1F600 is two units
  // starting at U+D83D, so plain < would put it first.
  assert.equal(compareCodePoints("\uff61", "\u{1f600}"), -1);
  assert.equal("\uff61" < "\u{1f600}", false);
  assert.deepEqual(
    ["b", "\u{1f600}", "\uff61", "a", "ab", ""].sort(compareCodePoints),
    ["", "a", "ab", "b", "\uff61", "\u{1f600}"],
  );
  assert.equal(compareCodePoints("same", "same"), 0);
});

// The fixture test records each text with the Rust codec's reading.
test("the raw payload editor reads every recorded text as the codec does", () => {
  for (const { text, codec, client } of payloadTexts) {
    const result = validateCanonicalPayload(text);
    switch (client) {
      case "agrees":
        if (codec === null) {
          assert.ok("error" in result, text);
        } else {
          assert.deepEqual(result, { payload: codec }, text);
        }
        break;
      case "refuses_shorthand":
        assert.notEqual(codec, null, text);
        assert.ok("error" in result, text);
        break;
      case "defers_timestamp":
        // The text is kept as typed; Jiff judges and respells it on Check.
        assert.deepEqual(result, { payload: JSON.parse(text) }, text);
        assert.notDeepEqual(codec, JSON.parse(text), text);
        break;
    }
  }
});
