import type { DecimalString, EntryId, NodeEntry } from "../contract.ts";
import { compareCodePoints } from "./values.ts";

// Map key of an entry identity.
export function entryKey(id: EntryId): string {
  return `${id.origin}:${id.position}`;
}

export function sameId(a: EntryId, b: EntryId): boolean {
  return a.origin === b.origin && a.position === b.position;
}

// Whether the entry lies past the as-of boundary, null being the whole
// history. Anything learned of it at a wider boundary, its label included,
// must not show while it does.
export function pastBoundary(id: EntryId, asOf: DecimalString | null): boolean {
  return asOf !== null && BigInt(id.position) >= BigInt(asOf);
}

// The view `[0, end)` in plain words, for a reader who does not know
// half-open intervals.
export function viewWords(end: DecimalString): string {
  switch (end) {
    case "0":
      return "the database before its first entry";
    case "1":
      return "the database as it was after its first entry, position 0";
    default:
      return `the database as it was after its first ${end} entries, positions 0 to ${BigInt(end) - 1n}`;
  }
}

// A type name as shown on screen: the empty name is legal and must stay
// visible and clickable.
export function typeName(name: string): string {
  return name === "" ? "(unnamed)" : name;
}

// Labels longer than this are cut on screen, with the full text as a tooltip
// (INTERFACES §5.2).
export const LABEL_LIMIT = 60;

function isLabelText(value: unknown): value is string {
  // Blank text would leave nothing to click on, so it does not label a row.
  return typeof value === "string" && value.trim() !== "";
}

// The payload field a row is labeled by (INTERFACES §5.2): `title` when text,
// else `name` when text, else the top-level text field whose name sorts first
// by code point, the byte order `views.render` labels its drawing by; null
// when the row has none.
export function labelField(node: Pick<NodeEntry, "payload">): string | null {
  const { payload } = node;
  for (const preferred of ["title", "name"]) {
    if (isLabelText(payload[preferred])) {
      return preferred;
    }
  }
  const textual = Object.keys(payload).filter((field) =>
    isLabelText(payload[field]),
  );
  textual.sort(compareCodePoints);
  return textual[0] ?? null;
}

// The label text itself; `<ty_name> @<position>` when no field labels the row.
export function rowLabel(
  node: Pick<NodeEntry, "payload" | "ty_name" | "id">,
): string {
  const field = labelField(node);
  if (field === null) {
    return `${typeName(node.ty_name)} @${node.id.position}`;
  }
  return String(node.payload[field]);
}

// The field that labels every row, if one does: its column would only
// repeat the label.
export function commonLabelField(
  rows: readonly Pick<NodeEntry, "payload">[],
): string | null {
  const [first, ...rest] = rows;
  const field = first === undefined ? null : labelField(first);
  return field !== null && rest.every((row) => labelField(row) === field)
    ? field
    : null;
}
