import type { EntryId, NodeEntry } from "../contract.ts";

// Map key of an entry identity.
export function entryKey(id: EntryId): string {
  return `${id.origin}:${id.position}`;
}

export function sameId(a: EntryId, b: EntryId): boolean {
  return a.origin === b.origin && a.position === b.position;
}

export function shortUuid(uuid: string): string {
  return uuid.slice(0, 8);
}

function isLabelText(value: unknown): value is string {
  // Blank text would leave nothing to click on, so it does not label a row.
  return typeof value === "string" && value.trim() !== "";
}

// The payload field a row is labeled by (INTERFACES §5): `title` when text,
// else `name` when text, else the alphabetically first top-level text field;
// null when the row has none.
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
  textual.sort();
  return textual[0] ?? null;
}

// The label text itself; `<ty_name> @<position>` when no field labels the row.
export function rowLabel(
  node: Pick<NodeEntry, "payload" | "ty_name" | "id">,
): string {
  const field = labelField(node);
  if (field === null) {
    return `${node.ty_name} @${node.id.position}`;
  }
  return String(node.payload[field]);
}
