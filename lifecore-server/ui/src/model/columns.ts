import type {
  FieldStatus,
  IntegerPayload,
  NodeEntry,
  Payload,
  TimestampPayload,
  TypeSchema,
} from "../contract.ts";
import { formatPayload } from "./payload.ts";

export const STATUS_ORDER: readonly FieldStatus[] = [
  "mandatory",
  "encouraged",
  "optional",
  "deprecated",
];

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

function isInteger(value: Payload): value is IntegerPayload {
  return (
    typeof value === "object" &&
    !Array.isArray(value) &&
    value.kind === "integer"
  );
}

function isTimestamp(value: Payload): value is TimestampPayload {
  return (
    typeof value === "object" &&
    !Array.isArray(value) &&
    value.kind === "timestamp"
  );
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

// Two integers compare by value, two timestamps by instant, anything else by
// its formatted text; an absent cell sorts last either way.
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
  if (isInteger(a) && isInteger(b)) {
    const x = BigInt(a.value);
    const y = BigInt(b.value);
    if (x < y) {
      return -1;
    }
    return x > y ? 1 : 0;
  }
  if (isTimestamp(a) && isTimestamp(b)) {
    return compareTimestamps(a.value, b.value);
  }
  return compareStrings(formatPayload(a), formatPayload(b));
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
  // Array.prototype.sort is stable, so equal cells keep append order.
  return [...rows].sort((a, b) => sign * compareCells(cell(a), cell(b)));
}
