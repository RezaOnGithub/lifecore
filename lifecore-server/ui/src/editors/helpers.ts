import type {
  Body,
  FieldDecl,
  FieldDecls,
  Payload,
  ProposalEntry,
  ProposalReference,
  TypeSchema,
  TypeSummary,
} from "../contract.ts";
import { compareStrings, STATUS_ORDER } from "../model/columns.ts";
import { nameError } from "../model/values.ts";

export type NodeOperation = Extract<ProposalEntry, { kind: "node" }>;
export type EdgeOperation = Extract<ProposalEntry, { kind: "edge" }>;

// The syntax rule, then uniqueness against the other operations of the draft.
export function operationNameError(
  name: string,
  operations: readonly ProposalEntry[],
  index: number,
): string | null {
  const syntax = nameError(name);
  if (syntax !== null) {
    return syntax;
  }
  const taken = operations.some(
    (operation, i) => i !== index && operation.name === name,
  );
  return taken ? `another operation is named ${name}` : null;
}

// The node operations before `index`: the only ones an `added` reference of
// the operation at `index` may name (FILES §2.3).
export function earlierNodes(
  operations: readonly ProposalEntry[],
  index: number,
): NodeOperation[] {
  return operations
    .slice(0, index)
    .filter(
      (operation): operation is NodeOperation => operation.kind === "node",
    );
}

// The type name of an `added` endpoint, when it names one of `earlier`.
export function addedNodeType(
  reference: ProposalReference,
  earlier: readonly NodeOperation[],
): string | null {
  if (!("added" in reference)) {
    return null;
  }
  const target = earlier.find(
    (operation) => operation.name === reference.added,
  );
  return target === undefined ? null : target.ty_name;
}

export function allEdgeLabels(types: TypeSummary[] | null): string[] {
  const labels = new Set<string>();
  for (const type of types ?? []) {
    if (type.resolution.kind === "declared") {
      for (const label of type.resolution.schema.outgoing) {
        labels.add(label);
      }
      for (const label of type.resolution.schema.incoming) {
        labels.add(label);
      }
    }
  }
  return [...labels].sort(compareStrings);
}

// What the from type permits outgoing and the to type permits incoming, when
// either is known; otherwise every declared label.
export function edgeTypeSuggestions(
  from: TypeSchema | null,
  to: TypeSchema | null,
  types: TypeSummary[] | null,
): string[] {
  const labels = new Set<string>([
    ...(from?.outgoing ?? []),
    ...(to?.incoming ?? []),
  ]);
  if (labels.size === 0) {
    return allEdgeLabels(types);
  }
  return [...labels].sort(compareStrings);
}

// Type names in the order the server lists them; the built-in `Type` is
// always declarable even before the list has loaded.
export function typeNames(types: TypeSummary[] | null): string[] {
  const names = (types ?? []).map((type) => type.name);
  return names.includes("Type") ? names : [...names, "Type"];
}

export interface FieldRow {
  field: string;
  // Null for a field the schema does not declare.
  decl: FieldDecl | null;
}

// Declared fields by status then name, present or not, then the body's
// undeclared fields by name.
export function fieldRows(fields: FieldDecls | null, body: Body): FieldRow[] {
  const declared = Object.entries(fields ?? {}).map(([field, decl]) => ({
    field,
    decl,
  }));
  declared.sort(
    (a, b) =>
      STATUS_ORDER.indexOf(a.decl.status) -
        STATUS_ORDER.indexOf(b.decl.status) || compareStrings(a.field, b.field),
  );
  const undeclared = Object.keys(body)
    .filter((field) => fields === null || !(field in fields))
    .sort(compareStrings)
    .map((field) => ({ field, decl: null }));
  return [...declared, ...undeclared];
}

export function withField(body: Body, field: string, value: Payload): Body {
  return { ...body, [field]: value };
}

export function withoutField(body: Body, field: string): Body {
  const rest: Body = {};
  for (const [name, value] of Object.entries(body)) {
    if (name !== field) {
      rest[name] = value;
    }
  }
  return rest;
}

export function moveItem<T>(
  items: readonly T[],
  from: number,
  to: number,
): T[] {
  if (to < 0 || to >= items.length) {
    return [...items];
  }
  const moved = [...items];
  const [item] = moved.splice(from, 1);
  moved.splice(to, 0, item as T);
  return moved;
}

// Comma-separated tags as the canonical name list: trimmed, without empties
// or repeats, sorted as the server's sets come back.
export function parseTags(text: string): string[] {
  const tags = text
    .split(",")
    .map((tag) => tag.trim())
    .filter((tag) => tag !== "");
  return [...new Set(tags)].sort();
}

// Renames a declared field in place, keeping the table order.
export function renameField(
  fields: FieldDecls,
  from: string,
  to: string,
): FieldDecls {
  const renamed: FieldDecls = {};
  for (const [field, decl] of Object.entries(fields)) {
    renamed[field === from ? to : field] = decl;
  }
  return renamed;
}

export function withoutDecl(fields: FieldDecls, field: string): FieldDecls {
  const rest: FieldDecls = {};
  for (const [name, decl] of Object.entries(fields)) {
    if (name !== field) {
      rest[name] = decl;
    }
  }
  return rest;
}

// `field_1`, `field_2`, …: the first name not yet declared.
export function freshFieldName(fields: FieldDecls): string {
  for (let n = 1; ; n += 1) {
    const name = `field_${n}`;
    if (!(name in fields)) {
      return name;
    }
  }
}
