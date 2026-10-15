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

// ---- Timestamps: a sanity check on what Jiff prints and parses ----

// Date, `T` (or a space), time, up to nine fraction digits, then `Z` or an
// offset. Jiff is the arbiter; this only catches obvious slips before a
// round trip to the server.
const TIMESTAMP =
  /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?([Zz]|[+-]\d{2}:\d{2})$/;

function daysInMonth(year: number, month: number): number {
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return days[month - 1] ?? 0;
}

export function timestampError(text: string): string | null {
  if (text === "") {
    return "timestamp required";
  }
  const match = TIMESTAMP.exec(text);
  if (match === null) {
    return "expected an RFC 3339 instant such as 2026-10-15T08:30:00Z, with up to nine fraction digits and Z or a ±HH:MM offset";
  }
  const part = (index: number) => Number(match[index] ?? "");
  const [year, month, day, hour, minute, second] = [1, 2, 3, 4, 5, 6].map(
    part,
  ) as [number, number, number, number, number, number];
  if (month < 1 || month > 12) {
    return `month ${month} is out of range`;
  }
  if (day < 1 || day > daysInMonth(year, month)) {
    return `day ${day} is out of range for ${match[1]}-${match[2]}`;
  }
  if (hour > 23 || minute > 59) {
    return "time of day is out of range";
  }
  // Jiff accepts a leap second and clamps it.
  if (second > 60) {
    return "second is out of range";
  }
  const offset = match[8] ?? "";
  if (offset.length === 6) {
    const hours = Number(offset.slice(1, 3));
    const minutes = Number(offset.slice(4, 6));
    if (hours > 23 || minutes > 59) {
      return "offset is out of range";
    }
  }
  return null;
}

// The instant `Date` knows, at the millisecond precision it has: no digits
// are invented beyond it.
export function nowTimestamp(date: Date = new Date()): TimestampString {
  return date.toISOString();
}

// ---- Operation names (FILES §2.3) ----

const NAME = /^[A-Za-z_][A-Za-z0-9_-]*$/;

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

// The value a field gets when it becomes present. A timestamp has no neutral
// spelling, so it starts at the clock; a record starts complete, with its
// mandatory fields defaulted in turn.
export function defaultValue(
  shape: Shape,
  now: () => TimestampString = nowTimestamp,
): Payload {
  switch (shape.kind) {
    case "text":
      return "";
    case "integer":
      return integer("0");
    case "boolean":
      return boolean(false);
    case "timestamp":
      return timestamp(now());
    case "list":
      return [];
    case "record":
      return record(defaultBody(shape.fields, now));
  }
}

export function defaultBody(
  fields: FieldDecls,
  now: () => TimestampString = nowTimestamp,
): Body {
  const body: Body = {};
  for (const [field, decl] of Object.entries(fields)) {
    if (decl.status === "mandatory") {
      body[field] = defaultValue(decl.shape, now);
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

function canonicalValue(json: unknown, path: string): Payload {
  if (typeof json === "string") {
    return json;
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
  const object = json as { [key: string]: unknown };
  if (!("kind" in object)) {
    throw new Malformed(
      `${path}: an object must be an envelope with "kind" and "value"; a record is ${RECORD_FORM}`,
    );
  }
  if (!("value" in object) || Object.keys(object).length !== 2) {
    throw new Malformed(
      `${path}: an envelope has exactly "kind" and "value", as in ${INTEGER_FORM}`,
    );
  }
  const { kind, value } = object;
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
      return timestamp(value);
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
    body[field] = canonicalValue(value, join(path, field));
  }
  return body;
}

function parseJson(text: string): { json: unknown } | { error: string } {
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
  const parsed = parseJson(text);
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
  const parsed = parseJson(text);
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
