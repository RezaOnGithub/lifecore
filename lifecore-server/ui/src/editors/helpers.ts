import type {
  Body,
  FieldDecl,
  FieldDecls,
  ProposalEntry,
  ProposalReference,
  Shape,
  TypeSchema,
  TypeSummary,
} from "../contract.ts";
import { compareStrings, STATUS_ORDER } from "../model/columns.ts";
import {
  compareCodePoints,
  nameError,
  type PayloadKind,
  setOwn,
} from "../model/values.ts";

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
  return [...labels].sort(compareCodePoints);
}

export interface EdgeSuggestions {
  labels: string[];
  // Why there is nothing to suggest although an end's type is known.
  note: string | null;
}

// The labels both known ends permit: the from type's `outgoing` and the to
// type's `incoming`, intersected when both are known. A known end that
// permits nothing is said so rather than hidden behind every label; only
// when neither end is known are all declared labels offered.
export function edgeTypeSuggestions(
  from: TypeSchema | null,
  to: TypeSchema | null,
  types: TypeSummary[] | null,
): EdgeSuggestions {
  if (from === null && to === null) {
    return { labels: allEdgeLabels(types), note: null };
  }
  if (from !== null && from.outgoing.length === 0) {
    return { labels: [], note: `${from.name} permits no outgoing edges` };
  }
  if (to !== null && to.incoming.length === 0) {
    return { labels: [], note: `${to.name} permits no incoming edges` };
  }
  const outgoing = from === null ? null : new Set(from.outgoing);
  const incoming = to === null ? null : new Set(to.incoming);
  const candidates = [...(outgoing ?? incoming ?? [])];
  const labels = candidates
    .filter(
      (label) =>
        (outgoing === null || outgoing.has(label)) &&
        (incoming === null || incoming.has(label)),
    )
    .sort(compareCodePoints);
  return {
    labels,
    note:
      labels.length === 0 && from !== null && to !== null
        ? `no label is both outgoing from ${from.name} and incoming to ${to.name}`
        : null,
  };
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
    .filter((field) => fields === null || !Object.hasOwn(fields, field))
    .sort(compareStrings)
    .map((field) => ({ field, decl: null }));
  return [...declared, ...undeclared];
}

// A copy with `field` set, in place when it exists and last otherwise.
export function withField<T>(
  object: { readonly [field: string]: T },
  field: string,
  value: T,
): { [field: string]: T } {
  const copy: { [field: string]: T } = {};
  for (const [name, old] of Object.entries(object)) {
    setOwn(copy, name, name === field ? value : old);
  }
  if (!Object.hasOwn(object, field)) {
    setOwn(copy, field, value);
  }
  return copy;
}

export function withoutField<T>(
  object: { readonly [field: string]: T },
  field: string,
): { [field: string]: T } {
  const rest: { [field: string]: T } = {};
  for (const [name, value] of Object.entries(object)) {
    if (name !== field) {
      setOwn(rest, name, value);
    }
  }
  return rest;
}

// Renames a declared field in place, keeping the table order. A name that
// is already declared is refused: the fields come back unchanged rather
// than one declaration overwriting another.
export function renameField(
  fields: FieldDecls,
  from: string,
  to: string,
): FieldDecls {
  if (
    from === to ||
    !Object.hasOwn(fields, from) ||
    Object.hasOwn(fields, to)
  ) {
    return fields;
  }
  const renamed: FieldDecls = {};
  for (const [field, decl] of Object.entries(fields)) {
    setOwn(renamed, field === from ? to : field, decl);
  }
  return renamed;
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

export interface Tags {
  // Trimmed, without empties or repeats, in code point order as the
  // server's sets come back.
  tags: string[];
  // Names typed more than once, each listed once.
  repeated: string[];
}

export function parseTags(text: string): Tags {
  const seen = new Set<string>();
  const repeated = new Set<string>();
  for (const tag of text.split(",").map((part) => part.trim())) {
    if (tag === "") {
      continue;
    }
    if (seen.has(tag)) {
      repeated.add(tag);
    }
    seen.add(tag);
  }
  return {
    tags: [...seen].sort(compareCodePoints),
    repeated: [...repeated].sort(compareCodePoints),
  };
}

// ---- Declaration shapes ----

// The shape for a kind chosen in the shape builder, keeping what nests:
// `list` wraps the current shape; a kind matching a list's element unwraps
// it; a record chosen over a record keeps its fields; a scalar replaces.
export function shapeOfKind(kind: PayloadKind, previous: Shape): Shape {
  if (previous.kind === kind) {
    return previous;
  }
  if (previous.kind === "list" && previous.element.kind === kind) {
    return previous.element;
  }
  switch (kind) {
    case "list":
      return { kind, element: previous };
    case "record":
      return { kind, fields: {} };
    default:
      return { kind };
  }
}

// A fresh field declaration's shape: a list starts as a list of text.
export function initialShape(kind: PayloadKind): Shape {
  switch (kind) {
    case "list":
      return { kind, element: { kind: "text" } };
    case "record":
      return { kind, fields: {} };
    default:
      return { kind };
  }
}

// ---- Text typed over a committed value ----

// Text being typed is kept with the committed value it was typed over.
// While that value is still the committed one, the text is what the input
// shows; once the value changes under it (another edit, an undo, a reload),
// the text is stale and the committed value shows instead.
export interface TextDraft<B = string> {
  text: string;
  base: B;
}

export function liveDraft<B>(
  draft: TextDraft<B> | null,
  committed: B,
): TextDraft<B> | null {
  return draft !== null && draft.base === committed ? draft : null;
}

export function lineCount(text: string): number {
  return text.split("\n").length;
}

// ---- Stable row keys ----

// Keys for rows that have no identity of their own, such as list elements:
// `keys` follows the editor's own moves and removals; when the list changes
// elsewhere, it is cut or extended to fit, with fresh keys at the end.
export function fitKeys(keys: readonly number[], count: number): number[] {
  if (keys.length >= count) {
    return keys.slice(0, count);
  }
  const fitted = [...keys];
  let next = nextKey(keys);
  while (fitted.length < count) {
    fitted.push(next);
    next += 1;
  }
  return fitted;
}

export function nextKey(keys: Iterable<number>): number {
  let next = 0;
  for (const key of keys) {
    next = Math.max(next, key + 1);
  }
  return next;
}

// Keys for named rows, such as declared fields, so a row keeps its key
// across a rename: the names present keep theirs, new names get fresh ones.
export function namedKeys(
  names: readonly string[],
  keys: ReadonlyMap<string, number>,
): Map<string, number> {
  const fitted = new Map<string, number>();
  let next = nextKey(keys.values());
  for (const name of names) {
    const key = keys.get(name);
    if (key === undefined) {
      fitted.set(name, next);
      next += 1;
    } else {
      fitted.set(name, key);
    }
  }
  return fitted;
}

export function renameKey(
  keys: ReadonlyMap<string, number>,
  from: string,
  to: string,
): Map<string, number> {
  const renamed = new Map<string, number>();
  for (const [name, key] of keys) {
    renamed.set(name === from ? to : name, key);
  }
  return renamed;
}

export function sameList<T>(a: readonly T[], b: readonly T[]): boolean {
  return a.length === b.length && a.every((value, i) => value === b[i]);
}

export function sameMap<K, V>(
  a: ReadonlyMap<K, V>,
  b: ReadonlyMap<K, V>,
): boolean {
  if (a.size !== b.size) {
    return false;
  }
  for (const [key, value] of a) {
    if (!b.has(key) || b.get(key) !== value) {
      return false;
    }
  }
  return true;
}
