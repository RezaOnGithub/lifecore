import type {
  FieldStatus,
  NodeEntry,
  Payload,
  PayloadKind,
  TypeSchema,
} from "../contract.ts";
import { formatPayload } from "./payload.ts";
import { PRESENCE_WORDS } from "./schema.ts";

export const STATUS_ORDER: readonly FieldStatus[] = PRESENCE_WORDS;

export interface FieldColumn {
  field: string;
  // Absent for a field the schema does not declare.
  status?: FieldStatus;
}

export function compareStrings(a: string, b: string): number {
  if (a < b) {
    return -1;
  }
  return a > b ? 1 : 0;
}

// Declared fields by status then name, then every other field present in the
// rows by name.
export function fieldColumns(
  schema: TypeSchema | null,
  rows: readonly Pick<NodeEntry, "payload">[],
): FieldColumn[] {
  const declared: FieldColumn[] =
    schema === null
      ? []
      : Object.entries(schema.fields).map(([field, decl]) => ({
          field,
          status: decl.status,
        }));
  declared.sort(
    (a, b) =>
      STATUS_ORDER.indexOf(a.status ?? "optional") -
        STATUS_ORDER.indexOf(b.status ?? "optional") ||
      compareStrings(a.field, b.field),
  );
  const known = new Set(declared.map((column) => column.field));
  const extra = new Set<string>();
  for (const row of rows) {
    for (const field of Object.keys(row.payload)) {
      if (!known.has(field)) {
        extra.add(field);
      }
    }
  }
  const undeclared = [...extra]
    .sort(compareStrings)
    .map((field) => ({ field }));
  return [...declared, ...undeclared];
}

// The string of a scalar envelope of this kind, or null.
function scalar(value: Payload, kind: PayloadKind): string | null {
  return typeof value === "object" &&
    !Array.isArray(value) &&
    value.kind === kind &&
    typeof value.value === "string"
    ? value.value
    : null;
}

function compareBigInts(x: bigint, y: bigint): number {
  if (x < y) {
    return -1;
  }
  return x > y ? 1 : 0;
}

// Nanoseconds in a fraction such as `5` (half a second), padded to nine.
function fractionNanos(fraction: string | undefined): bigint {
  return BigInt((fraction ?? "").padEnd(9, "0"));
}

// Jiff's ISO 8601 duration display: a sign, then hours, minutes, and
// seconds with a fraction, each omitted when zero (`PT0S` for nothing).
const DURATION = /^(-?)PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)(?:\.(\d+))?S)?$/;

function durationNanos(text: string): bigint | null {
  const match = DURATION.exec(text);
  if (match === null) {
    return null;
  }
  const [, sign, hours, minutes, seconds, fraction] = match;
  const total =
    ((BigInt(hours ?? "0") * 60n + BigInt(minutes ?? "0")) * 60n +
      BigInt(seconds ?? "0")) *
      1_000_000_000n +
    fractionNanos(fraction);
  return sign === "-" ? -total : total;
}

// Jiff's offset timestamp display: the local date and time to the second,
// an optional fraction, and a numeric offset to the minute.
const OFFSET_TIMESTAMP =
  /^([+-]\d{6}|\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?([+-])(\d{2}):(\d{2})$/;

// The instant, in nanoseconds since the epoch: the local time read as UTC,
// less the offset. `setUTCFullYear` keeps years below 100 as written.
function instantNanos(text: string): bigint | null {
  const match = OFFSET_TIMESTAMP.exec(text);
  if (match === null) {
    return null;
  }
  const [, year, month, day, hour, minute, second, fraction, sign, oh, om] =
    match.map((part) => part ?? "");
  const local = new Date(0);
  local.setUTCFullYear(Number(year), Number(month) - 1, Number(day));
  local.setUTCHours(Number(hour), Number(minute), Number(second), 0);
  const offsetMinutes =
    (sign === "-" ? -1 : 1) * (Number(oh) * 60 + Number(om));
  return (
    (BigInt(local.getTime()) - BigInt(offsetMinutes) * 60_000n) * 1_000_000n +
    fractionNanos(fraction)
  );
}

// Two values compared by `measure`, or null when either does not measure.
function compareMeasured(
  a: string | null,
  b: string | null,
  measure: (text: string) => bigint | null,
): number | null {
  if (a === null || b === null) {
    return null;
  }
  const x = measure(a);
  const y = measure(b);
  return x === null || y === null ? null : compareBigInts(x, y);
}

// Jiff's spelling: the instant to the second, an optional fraction, and `Z`.
const TIMESTAMP = /^(.*?)(?:\.(\d+))?Z$/;

// Jiff omits a zero fraction, so as text `…:00Z` would sort after `…:00.5Z`
// (`Z` follows `.`). The seconds compare first, then the fractions padded to
// nanoseconds; a spelling outside the pattern falls back to text.
function compareTimestamps(a: string, b: string): number {
  const x = TIMESTAMP.exec(a);
  const y = TIMESTAMP.exec(b);
  if (x === null || y === null) {
    return compareStrings(a, b);
  }
  return (
    compareStrings(x[1] ?? "", y[1] ?? "") ||
    compareStrings((x[2] ?? "").padEnd(9, "0"), (y[2] ?? "").padEnd(9, "0"))
  );
}

// Two integers compare by value, two timestamps or two offset timestamps by
// instant (one instant at two offsets ties, keeping append order), two
// durations by length, anything else by its formatted text, which orders
// canonical dates by day; an absent cell sorts last either way. A spelling
// the pattern does not know falls back to text.
export function compareCells(
  a: Payload | undefined,
  b: Payload | undefined,
): number {
  if (a === undefined) {
    return b === undefined ? 0 : 1;
  }
  if (b === undefined) {
    return -1;
  }
  const integers = compareMeasured(
    scalar(a, "integer"),
    scalar(b, "integer"),
    BigInt,
  );
  if (integers !== null) {
    return integers;
  }
  const x = scalar(a, "timestamp");
  const y = scalar(b, "timestamp");
  if (x !== null && y !== null) {
    return compareTimestamps(x, y);
  }
  const measured =
    compareMeasured(
      scalar(a, "duration"),
      scalar(b, "duration"),
      durationNanos,
    ) ??
    compareMeasured(
      scalar(a, "offset_timestamp"),
      scalar(b, "offset_timestamp"),
      instantNanos,
    );
  return measured ?? compareStrings(formatPayload(a), formatPayload(b));
}

export interface Sort {
  column: string;
  descending: boolean;
}

// A click on the sorted column flips it; on another column sorts ascending.
export function toggleSort(sort: Sort | null, column: string): Sort {
  if (sort !== null && sort.column === column) {
    return { column, descending: !sort.descending };
  }
  return { column, descending: false };
}

export function sortedBy<T>(
  rows: readonly T[],
  cell: (row: T) => Payload | undefined,
  descending: boolean,
): T[] {
  const sign = descending ? -1 : 1;
  // Each cell is read once, not once per comparison; Array.prototype.sort
  // is stable, so equal cells keep append order.
  return rows
    .map((row) => ({ row, value: cell(row) }))
    .sort((a, b) =>
      // Absent cells stay last whichever way the column sorts.
      a.value === undefined || b.value === undefined
        ? compareCells(a.value, b.value)
        : sign * compareCells(a.value, b.value),
    )
    .map(({ row }) => row);
}
