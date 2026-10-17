import type {
  Body,
  BooleanPayload,
  DecimalString,
  FieldDecls,
  IntegerPayload,
  Payload,
  RecordPayload,
  Shape,
  TimestampPayload,
  TimestampString,
} from "../contract.ts";

// ---- Own properties ----

// Field names are data, so `__proto__`, `constructor`, and `toString` are
// ordinary names: membership is an own-property test, and writes define a
// data property, since assigning `__proto__` would replace the prototype.
export function getOwn<T>(
  object: { readonly [key: string]: T },
  key: string,
): T | undefined {
  return Object.hasOwn(object, key) ? object[key] : undefined;
}

export function setOwn<T>(
  object: { [key: string]: T },
  key: string,
  value: T,
): void {
  Object.defineProperty(object, key, {
    value,
    enumerable: true,
    writable: true,
    configurable: true,
  });
}

// ---- Ordering ----

// Code point order, which is the byte order of the server's UTF-8 `String`s
// in a `BTreeSet`. Plain `<` compares UTF-16 units, which puts an astral
// character before U+E000..U+FFFF.
export function compareCodePoints(a: string, b: string): number {
  let i = 0;
  while (i < a.length && i < b.length) {
    const x = a.codePointAt(i) ?? 0;
    const y = b.codePointAt(i) ?? 0;
    if (x !== y) {
      return x < y ? -1 : 1;
    }
    // Equal code points span the same number of units in both strings.
    i += x > 0xffff ? 2 : 1;
  }
  return Math.sign(a.length - b.length);
}

// ---- Integers (FILES §2.1): one spelling per value, within i64 ----

const DECIMAL = /^(?:0|-?[1-9][0-9]*)$/;
const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;

export function decimalError(text: string): string | null {
  if (text === "") {
    return "integer required";
  }
  if (!DECIMAL.test(text)) {
    return "not a canonical decimal: 0, or an optional - and digits without a leading zero";
  }
  // BigInt, never Number: the whole i64 domain must survive intact.
  const value = BigInt(text);
  if (value < I64_MIN || value > I64_MAX) {
    return "outside the signed 64-bit range";
  }
  return null;
}

export function isDecimal(text: string): boolean {
  return decimalError(text) === null;
}

// ---- Timestamps: Jiff decides (FILES §2.1) ----

// Any non-empty text is a timestamp value as far as the client goes: Jiff's
// parser, range, and normalization are the server's, and Check reports what
// it rejects. Empty text is refused only because nothing would accept it.
export function timestampError(text: string): string | null {
  return text === "" ? "a timestamp cannot be empty" : null;
}

// A loose recognizer for RFC 3339 and RFC 9557 spellings: a date with a
// four- or six-digit year, extended or basic; a time to the hour, minute,
// or second with any fraction; `Z` or an offset to the second; bracketed
// annotations. Values are not range-checked. It only spots text that is
// plainly something else, so the hint can say so before a Check.
const LOOSE_TIMESTAMP =
  /^[+-]?\d{4,6}-?\d{2}-?\d{2}[Tt ]\d{2}(?::?\d{2}(?::?\d{2}(?:[.,]\d+)?)?)?(?:[Zz]|[+-]\d{2}(?::?\d{2}(?::?\d{2}(?:[.,]\d+)?)?)?)(?:\[[^\]]+\])*$/;

export const TIMESTAMP_HINT =
  "not in the usual RFC 3339 form; Check will report it if Jiff rejects it";

export function timestampHint(text: string): string | null {
  if (text === "") {
    return "an instant such as 2026-10-15T08:30:00Z, or now (UTC)";
  }
  return LOOSE_TIMESTAMP.test(text) ? null : TIMESTAMP_HINT;
}

// The instant `Date` knows, at the millisecond precision it has: no digits
// are invented beyond it.
export function nowTimestamp(date: Date = new Date()): TimestampString {
  return date.toISOString();
}

// ---- Operation names (FILES §2.3) ----

export const NAME = /^[A-Za-z_][A-Za-z0-9_-]*$/;

export function nameError(name: string): string | null {
  if (name === "") {
    return "name required";
  }
  return NAME.test(name) ? null : "names match [A-Za-z_][A-Za-z0-9_-]*";
}

// ---- Payload envelopes ----

export type PayloadKind = Shape["kind"];

export function kindOf(value: Payload): PayloadKind {
  if (typeof value === "string") {
    return "text";
  }
  if (Array.isArray(value)) {
    return "list";
  }
  return value.kind;
}

export function integer(value: DecimalString): IntegerPayload {
  return { kind: "integer", value };
}

export function boolean(value: boolean): BooleanPayload {
  return { kind: "boolean", value };
}

export function timestamp(value: TimestampString): TimestampPayload {
  return { kind: "timestamp", value };
}

export function record(value: Body): RecordPayload {
  return { kind: "record", value };
}

// Only the outermost kind: a list editor judges each element on its own.
export function matchesShape(value: Payload, shape: Shape): boolean {
  return kindOf(value) === shape.kind;
}

// The value a field gets when it becomes present, or null for an integer
// or a timestamp: neither has a neutral value, so the editor waits for one
// to be typed rather than inventing `0` or the current time. A record starts
// with those of its mandatory fields that have a default.
export function defaultValue(shape: Shape): Payload | null {
  switch (shape.kind) {
    case "text":
      return "";
    case "integer":
    case "timestamp":
      return null;
    case "boolean":
      return boolean(false);
    case "list":
      return [];
    case "record":
      return record(defaultBody(shape.fields));
  }
}

export function defaultBody(fields: FieldDecls): Body {
  const body: Body = {};
  for (const [field, decl] of Object.entries(fields)) {
    const value = decl.status === "mandatory" ? defaultValue(decl.shape) : null;
    if (value !== null) {
      setOwn(body, field, value);
    }
  }
  return body;
}

// ---- Canonical JSON text (FILES §2.1) ----

// Raw JSON typed in the browser must already be canonical: bare numbers are
// refused rather than parsed, since `JSON.parse` would have rounded a large
// one before any check could run.
class Malformed extends Error {}

const INTEGER_FORM = '{"kind":"integer","value":"42"}';
const BOOLEAN_FORM = '{"kind":"boolean","value":true}';
const RECORD_FORM = '{"kind":"record","value":{…}}';

function join(path: string, field: string): string {
  return path === "" ? field : `${path}.${field}`;
}

export interface RepeatedKey {
  // The JSON path of the object that repeats `key`; "" for the outermost.
  path: string;
  key: string;
}

interface Frame {
  // Null for an array.
  keys: Set<string> | null;
  path: string;
  key: string;
  index: number;
  expectKey: boolean;
}

// `JSON.parse` keeps the last of repeated keys, but a repeated field name is
// malformed wherever it appears (FILES §2.1), so the text is scanned first:
// one key set per open object, strings skipped whole, so braces, commas,
// and escaped quotes inside them are inert. Keys compare decoded, as
// `"a"` and `"\u0061"` name the same field. Text that is not JSON yields
// null and is left to `JSON.parse` to report.
export function findRepeatedKey(text: string): RepeatedKey | null {
  const stack: Frame[] = [];
  const childPath = (): string => {
    const top = stack.at(-1);
    if (top === undefined) {
      return "";
    }
    return top.keys === null
      ? `${top.path}[${top.index}]`
      : join(top.path, top.key);
  };
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '"') {
      let end = i + 1;
      while (end < text.length && text[end] !== '"') {
        end += text[end] === "\\" ? 2 : 1;
      }
      if (end >= text.length) {
        return null;
      }
      const top = stack.at(-1);
      if (top !== undefined && top.keys !== null && top.expectKey) {
        let key: unknown;
        try {
          key = JSON.parse(text.slice(i, end + 1));
        } catch {
          return null;
        }
        if (typeof key !== "string") {
          return null;
        }
        if (top.keys.has(key)) {
          return { path: top.path, key };
        }
        top.keys.add(key);
        top.key = key;
        top.expectKey = false;
      }
      i = end + 1;
      continue;
    }
    if (c === "{" || c === "[") {
      stack.push({
        keys: c === "{" ? new Set() : null,
        path: childPath(),
        key: "",
        index: 0,
        expectKey: c === "{",
      });
    } else if (c === "}" || c === "]") {
      stack.pop();
    } else if (c === ",") {
      const top = stack.at(-1);
      if (top !== undefined) {
        if (top.keys === null) {
          top.index += 1;
        } else {
          top.expectKey = true;
        }
      }
    }
    i += 1;
  }
  return null;
}

// `JSON.parse` turns an escape such as `\ud800` into a lone surrogate, which
// no UTF-8 text holds, so the codec refuses it in a key or a string.
const LONE_SURROGATE =
  /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;

function wellFormed(text: string, path: string): string {
  if (LONE_SURROGATE.test(text)) {
    throw new Malformed(
      `${path}: holds a lone surrogate, which is not Unicode text`,
    );
  }
  return text;
}

function canonicalValue(json: unknown, path: string): Payload {
  if (typeof json === "string") {
    return wellFormed(json, path);
  }
  if (typeof json === "number") {
    throw new Malformed(
      `${path}: bare numbers are not values; integers must be ${INTEGER_FORM}`,
    );
  }
  if (typeof json === "boolean") {
    throw new Malformed(
      `${path}: bare booleans are not values; booleans must be ${BOOLEAN_FORM}`,
    );
  }
  if (json === null) {
    throw new Malformed(`${path}: null is not a value; omit the field instead`);
  }
  if (Array.isArray(json)) {
    return json.map((item, index) => canonicalValue(item, `${path}[${index}]`));
  }
  if (typeof json !== "object") {
    throw new Malformed(`${path}: not a JSON value`);
  }
  if (!Object.hasOwn(json, "kind")) {
    throw new Malformed(
      `${path}: an object must be an envelope with "kind" and "value"; a record is ${RECORD_FORM}`,
    );
  }
  if (!Object.hasOwn(json, "value") || Object.keys(json).length !== 2) {
    throw new Malformed(
      `${path}: an envelope has exactly "kind" and "value", as in ${INTEGER_FORM}`,
    );
  }
  const { kind, value } = json as { kind: unknown; value: unknown };
  switch (kind) {
    case "integer": {
      if (typeof value !== "string") {
        throw new Malformed(
          `${path}: an integer value is a decimal string, as in ${INTEGER_FORM}`,
        );
      }
      const problem = decimalError(value);
      if (problem !== null) {
        throw new Malformed(`${path}: ${problem}`);
      }
      return integer(value);
    }
    case "boolean":
      if (typeof value !== "boolean") {
        throw new Malformed(
          `${path}: a boolean value is true or false, as in ${BOOLEAN_FORM}`,
        );
      }
      return boolean(value);
    case "timestamp": {
      if (typeof value !== "string") {
        throw new Malformed(`${path}: a timestamp value is a string`);
      }
      const problem = timestampError(value);
      if (problem !== null) {
        throw new Malformed(`${path}: ${problem}`);
      }
      return timestamp(wellFormed(value, path));
    }
    case "record":
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        throw new Malformed(
          `${path}: a record value is an object of fields, as in ${RECORD_FORM}`,
        );
      }
      return record(canonicalBody(value, path));
    default:
      throw new Malformed(
        `${path}: unknown kind ${JSON.stringify(kind)}; envelopes are integer, boolean, timestamp, or record`,
      );
  }
}

function canonicalBody(json: object, path: string): Body {
  const body: Body = {};
  for (const [field, value] of Object.entries(json)) {
    const at = join(path, field);
    setOwn(body, wellFormed(field, at), canonicalValue(value, at));
  }
  return body;
}

// `where` names the text in messages: a field label, or "" for a payload.
function parseJson(
  text: string,
  where: string,
): { json: unknown } | { error: string } {
  const repeated = findRepeatedKey(text);
  if (repeated !== null) {
    const path = join(where, repeated.path);
    return {
      error: `${path === "" ? "the payload" : path} repeats the key ${JSON.stringify(repeated.key)}; a name may appear once (FILES §2.1)`,
    };
  }
  try {
    return { json: JSON.parse(text) as unknown };
  } catch (failure) {
    const message =
      failure instanceof Error ? failure.message : String(failure);
    return { error: `invalid JSON: ${message}` };
  }
}

function malformed(failure: unknown): { error: string } {
  if (failure instanceof Malformed) {
    return { error: failure.message };
  }
  throw failure;
}

// A whole node payload: a JSON object of canonically encoded fields.
export function validateCanonicalPayload(
  text: string,
): { payload: Body } | { error: string } {
  const parsed = parseJson(text, "");
  if ("error" in parsed) {
    return parsed;
  }
  const { json } = parsed;
  if (typeof json !== "object" || json === null || Array.isArray(json)) {
    return { error: "a payload is a JSON object of fields" };
  }
  try {
    return { payload: canonicalBody(json, "") };
  } catch (failure) {
    return malformed(failure);
  }
}

// One field's value; `path` names it in messages.
export function validateCanonicalValue(
  text: string,
  path = "value",
): { value: Payload } | { error: string } {
  const parsed = parseJson(text, path);
  if ("error" in parsed) {
    return parsed;
  }
  try {
    return { value: canonicalValue(parsed.json, path) };
  } catch (failure) {
    return malformed(failure);
  }
}

export interface PayloadByKind {
  text: string;
  list: Payload[];
  integer: IntegerPayload;
  boolean: BooleanPayload;
  timestamp: TimestampPayload;
  record: RecordPayload;
}

// The value as its kind's type, or null when it is another kind.
export function narrow<K extends PayloadKind>(
  value: Payload,
  kind: K,
): PayloadByKind[K] | null {
  return kindOf(value) === kind ? (value as PayloadByKind[K]) : null;
}
