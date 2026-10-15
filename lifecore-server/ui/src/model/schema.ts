import type {
  Body,
  FieldDecl,
  FieldDecls,
  FieldStatus,
  Payload,
  RecordPayload,
  Shape,
  TypeSchema,
} from "../contract.ts";

const STATUSES: readonly string[] = [
  "mandatory",
  "encouraged",
  "deprecated",
  "optional",
];

// Reads a `Type` declaration payload (FILES §2.6) into the structured schema
// for declarations the server does not resolve, such as superseded ones.
// Anything outside the grammar yields null; judging a declaration malformed
// is the server's job, this reader only displays.
export function parseDeclaration(payload: Body): TypeSchema | null {
  const { name } = payload;
  if (typeof name !== "string") {
    return null;
  }
  const fields =
    payload.fields === undefined ? {} : parseFieldDecls(payload.fields);
  const outgoing = parseNames(payload.outgoing);
  const incoming = parseNames(payload.incoming);
  const successors = parseNames(payload.successors);
  if (
    fields === null ||
    outgoing === null ||
    incoming === null ||
    successors === null
  ) {
    return null;
  }
  return { name, fields, outgoing, incoming, successors };
}

// Edge and type names come back sorted, as the server's BTreeSet would.
function parseNames(value: Payload | undefined): string[] | null {
  if (value === undefined) {
    return [];
  }
  if (!Array.isArray(value)) {
    return null;
  }
  const names: string[] = [];
  for (const item of value) {
    if (typeof item !== "string") {
      return null;
    }
    names.push(item);
  }
  names.sort();
  return names;
}

function parseFieldDecls(value: Payload): FieldDecls | null {
  if (
    typeof value !== "object" ||
    Array.isArray(value) ||
    value.kind !== "record"
  ) {
    return null;
  }
  const decls: FieldDecls = {};
  for (const [field, words] of Object.entries(value.value)) {
    const decl = parseFieldDecl(words);
    if (decl === null) {
      return null;
    }
    decls[field] = decl;
  }
  return decls;
}

function parseFieldDecl(words: Payload): FieldDecl | null {
  if (!Array.isArray(words)) {
    return null;
  }
  const [status, ...rest] = words;
  if (typeof status !== "string" || !STATUSES.includes(status)) {
    return null;
  }
  const shape = parseShape(rest);
  return shape === null ? null : { status: status as FieldStatus, shape };
}

// The words must be consumed entirely: one scalar word, `list` and the
// element's words, or `record` and exactly one envelope.
function parseShape(words: Payload[]): Shape | null {
  const [head, ...tail] = words;
  switch (head) {
    case "text":
    case "integer":
    case "boolean":
    case "timestamp":
      return tail.length === 0 ? { kind: head } : null;
    case "list": {
      const element = parseShape(tail);
      return element === null ? null : { kind: "list", element };
    }
    case "record": {
      const [envelope, ...extra] = tail;
      if (envelope === undefined || extra.length > 0) {
        return null;
      }
      const fields = parseFieldDecls(envelope);
      return fields === null ? null : { kind: "record", fields };
    }
    default:
      return null;
  }
}

// Writes the structured schema as the word encoding of FILES §2.6, the
// inverse of `parseDeclaration`. All five keys are written, empty where
// nothing is declared, so a form-built declaration equals a hand-written
// canonical one. Permission lists come out sorted and without repeats, as
// the server's sets would; a repeated permission would be malformed.
export function declarationBody(schema: TypeSchema): Body {
  return {
    name: schema.name,
    fields: fieldDeclsEnvelope(schema.fields),
    outgoing: canonicalNames(schema.outgoing),
    incoming: canonicalNames(schema.incoming),
    successors: canonicalNames(schema.successors),
  };
}

function canonicalNames(names: readonly string[]): string[] {
  return [...new Set(names)].sort();
}

function fieldDeclsEnvelope(decls: FieldDecls): RecordPayload {
  const value: Body = {};
  for (const [field, decl] of Object.entries(decls)) {
    value[field] = [decl.status, ...shapeWordList(decl.shape)];
  }
  return { kind: "record", value };
}

// `record` ends the list with its envelope; `list` is followed by the
// element's own words.
function shapeWordList(shape: Shape): Payload[] {
  switch (shape.kind) {
    case "text":
    case "integer":
    case "boolean":
    case "timestamp":
      return [shape.kind];
    case "list":
      return ["list", ...shapeWordList(shape.element)];
    case "record":
      return ["record", fieldDeclsEnvelope(shape.fields)];
  }
}

// The summary column of the `Type` table.
export function declarationSummary(payload: Body): string {
  const schema = parseDeclaration(payload);
  if (schema === null) {
    return "unreadable declaration";
  }
  const count = Object.keys(schema.fields).length;
  const names = (list: string[]) =>
    list.length === 0 ? "none" : list.join(", ");
  return [
    `${count} field${count === 1 ? "" : "s"}`,
    `out ${names(schema.outgoing)}`,
    `in ${names(schema.incoming)}`,
    `successors ${names(schema.successors)}`,
  ].join("; ");
}
