import type {
  Body,
  EntryId,
  FieldDecl,
  FieldDecls,
  FieldStatus,
  Payload,
  PayloadKind,
  RecordPayload,
  SchemaError,
  SchemaErrorKind,
  Shape,
  TypeSchema,
  TypeSummary,
} from "../contract.ts";
import { entryKey } from "./labels.ts";
import { compareCodePoints, kindOf, setOwn } from "./values.ts";

// The presence words of FILES §2.6, in the order fields are listed.
export const PRESENCE_WORDS: readonly FieldStatus[] = [
  "mandatory",
  "encouraged",
  "optional",
  "deprecated",
];

export const DECLARATION_KEYS: readonly string[] = [
  "name",
  "fields",
  "outgoing",
  "incoming",
  "successors",
];

// The server's bound on nested `list` and `record` words, counted across
// nested records (FILES §2.6).
export const MAX_SHAPE_DEPTH = 64;

// Keys outside the five are reported together, so the raw editor can name
// them all; the server reports the first in code point order.
export type DeclarationReading =
  | { kind: "ok"; schema: TypeSchema }
  | { kind: "unknown_keys"; keys: string[] }
  | { kind: "malformed"; error: SchemaError };

type Part = SchemaError["path"][number];

class Malformed extends Error {
  readonly problem: SchemaErrorKind;
  readonly path: Part[];
  constructor(problem: SchemaErrorKind, path: Part[] = []) {
    super(problem.kind);
    this.problem = problem;
    this.path = path;
  }
  // Errors gain their path from the inside out, as the server's do.
  at(part: Part): Malformed {
    return new Malformed(this.problem, [part, ...this.path]);
  }
}

function wrongShape(expected: PayloadKind, value: Payload): Malformed {
  return new Malformed({
    kind: "wrong_shape",
    expected,
    actual: kindOf(value),
  });
}

// Reads a `Type` declaration payload (FILES §2.6) the way the server's
// `Type::from_body` does, in the same order: keys, then `name`, `fields`,
// and the three permission lists, each record's fields in code point order.
// A failure is the error the server reports for the payload.
export function readDeclaration(payload: Body): DeclarationReading {
  const unknown = Object.keys(payload)
    .filter((key) => !DECLARATION_KEYS.includes(key))
    .sort(compareCodePoints);
  if (unknown.length > 0) {
    return { kind: "unknown_keys", keys: unknown };
  }
  try {
    return {
      kind: "ok",
      schema: {
        name: readName(payload),
        fields: Object.hasOwn(payload, "fields")
          ? within("fields", () => readFieldDecls(payload.fields as Payload, 0))
          : {},
        outgoing: readNames(payload, "outgoing"),
        incoming: readNames(payload, "incoming"),
        successors: readNames(payload, "successors"),
      },
    };
  } catch (failure) {
    if (failure instanceof Malformed) {
      return {
        kind: "malformed",
        error: { path: failure.path, ...failure.problem },
      };
    }
    throw failure;
  }
}

// The structured schema, or null for anything the server would reject.
export function parseDeclaration(payload: Body): TypeSchema | null {
  const reading = readDeclaration(payload);
  return reading.kind === "ok" ? reading.schema : null;
}

// What the declaration form edits: a new `Type` node starts with an empty
// payload, so a missing `name` reads as empty, and the form's first edit
// writes all five keys.
export function declarationForForm(payload: Body): DeclarationReading {
  return readDeclaration(
    Object.hasOwn(payload, "name") ? payload : { name: "", ...payload },
  );
}

function within<T>(part: Part, read: () => T): T {
  try {
    return read();
  } catch (failure) {
    throw failure instanceof Malformed ? failure.at(part) : failure;
  }
}

function readName(payload: Body): string {
  if (!Object.hasOwn(payload, "name")) {
    throw new Malformed({ kind: "missing_field" }, ["name"]);
  }
  const name = payload.name as Payload;
  if (typeof name !== "string") {
    throw wrongShape("text", name).at("name");
  }
  return name;
}

// Permission lists come back in code point order, as the server's
// `BTreeSet` would hold them; a repeated name is malformed.
function readNames(payload: Body, key: string): string[] {
  if (!Object.hasOwn(payload, key)) {
    return [];
  }
  const value = payload[key] as Payload;
  if (!Array.isArray(value)) {
    throw wrongShape("list", value).at(key);
  }
  const names = new Set<string>();
  value.forEach((item, index) => {
    if (typeof item !== "string") {
      throw wrongShape("text", item).at(index).at(key);
    }
    if (names.has(item)) {
      throw new Malformed({ kind: "duplicate_permission", name: item }, [
        key,
        index,
      ]);
    }
    names.add(item);
  });
  return [...names].sort(compareCodePoints);
}

// Fields are read in code point order, the server's `BTreeMap` order, so
// both report the same field when several are malformed.
function readFieldDecls(value: Payload, depth: number): FieldDecls {
  if (kindOf(value) !== "record") {
    throw wrongShape("record", value);
  }
  const fields = (value as RecordPayload).value;
  const decls: FieldDecls = {};
  for (const field of Object.keys(fields).sort(compareCodePoints)) {
    setOwn(
      decls,
      field,
      within(field, () => readFieldDecl(fields[field] as Payload, depth)),
    );
  }
  return decls;
}

// `[presence, shape words...]`; a `record` word takes the next element as
// its field declarations, so it ends the list.
function readFieldDecl(value: Payload, depth: number): FieldDecl {
  if (!Array.isArray(value)) {
    throw wrongShape("list", value);
  }
  const status = word(value, 0, { kind: "missing_presence" });
  if (!PRESENCE_WORDS.includes(status as FieldStatus)) {
    throw new Malformed({ kind: "unknown_presence", word: status }, [0]);
  }
  const [shape, end] = readShape(value, 1, depth);
  if (end < value.length) {
    throw new Malformed({ kind: "trailing_elements" }, [end]);
  }
  return { status: status as FieldStatus, shape };
}

function word(
  words: readonly Payload[],
  index: number,
  missing: SchemaErrorKind,
): string {
  const value = words[index];
  if (value === undefined) {
    throw new Malformed(missing, [index]);
  }
  if (typeof value !== "string") {
    throw wrongShape("text", value).at(index);
  }
  return value;
}

// The shape starting at `start` and the index just after it. `depth`
// counts the `list` and `record` words enclosing it, across records.
function readShape(
  words: readonly Payload[],
  start: number,
  depth: number,
): [Shape, number] {
  const head = word(words, start, { kind: "missing_shape" });
  if ((head === "list" || head === "record") && depth >= MAX_SHAPE_DEPTH) {
    throw new Malformed({ kind: "too_deep" }, [start]);
  }
  switch (head) {
    case "text":
    case "integer":
    case "boolean":
    case "timestamp":
      return [{ kind: head }, start + 1];
    case "list": {
      const [element, end] = readShape(words, start + 1, depth + 1);
      return [{ kind: "list", element }, end];
    }
    case "record": {
      const next = start + 1;
      const envelope = words[next];
      if (envelope === undefined) {
        throw new Malformed({ kind: "missing_record_fields" }, [next]);
      }
      const fields = within(next, () => readFieldDecls(envelope, depth + 1));
      return [{ kind: "record", fields }, next + 1];
    }
    default:
      throw new Malformed({ kind: "unknown_shape", word: head }, [start]);
  }
}

// ---- Words for declaration errors ----

// The client words every declaration error it shows, whether its own
// parse or a server resolution found it; Rust's `Display` words the same
// errors where the server writes text, as in TypeCheck findings.
const KIND_WORDS: { [K in PayloadKind]: string } = {
  text: "text",
  integer: "an integer",
  boolean: "a boolean",
  timestamp: "a timestamp",
  list: "a list",
  record: "a record",
};

export function schemaErrorMessage(error: SchemaErrorKind): string {
  switch (error.kind) {
    case "missing_field":
      return "required but missing";
    case "unknown_field":
      return `not a declaration key; a declaration has ${DECLARATION_KEYS.join(", ")}`;
    case "wrong_shape":
      return `expected ${KIND_WORDS[error.expected]}, found ${KIND_WORDS[error.actual]}`;
    case "missing_presence":
      return "missing a presence word";
    case "unknown_presence":
      return `unknown presence ${JSON.stringify(error.word)}; expected ${PRESENCE_WORDS.join(", ")}`;
    case "missing_shape":
      return "missing a shape word";
    case "unknown_shape":
      return `unknown shape word ${JSON.stringify(error.word)}; expected text, integer, boolean, timestamp, list, or record`;
    case "missing_record_fields":
      return "record must be followed by a record envelope of field declarations";
    case "trailing_elements":
      return "unexpected element after the shape";
    case "too_deep":
      return `more than ${MAX_SHAPE_DEPTH} nested list and record words`;
    case "duplicate_permission":
      return `${JSON.stringify(error.name)} is repeated`;
  }
}

const PLAIN_FIELD = /^[A-Za-z_][A-Za-z0-9_-]*$/;

// `fields.a[1]`. A field name outside the identifier grammar is quoted, so
// a dot, bracket, or space inside a name cannot read as structure.
export function schemaPathText(path: readonly Part[]): string {
  let text = "";
  for (const part of path) {
    if (typeof part === "number") {
      text += `[${part}]`;
    } else {
      text += `${text === "" ? "" : "."}${PLAIN_FIELD.test(part) ? part : JSON.stringify(part)}`;
    }
  }
  return text;
}

// `fields.a[1]: unknown shape word "txt"; …`, or the words alone for the
// payload as a whole.
export function schemaErrorText(error: SchemaError): string {
  const path = schemaPathText(error.path);
  const message = schemaErrorMessage(error);
  return path === "" ? message : `${path}: ${message}`;
}

// The server's reading of each current declaration `types.list` resolved,
// by entry key: a declared or ambiguous name's parsed claimants and its
// malformed ones. A stale declaration, or one without a text `name`, claims
// no name and is not covered.
export function serverReadings(
  types: readonly TypeSummary[] | null,
): Map<string, DeclarationReading> {
  const readings = new Map<string, DeclarationReading>();
  for (const { resolution } of types ?? []) {
    switch (resolution.kind) {
      case "declared":
        readings.set(entryKey(resolution.declaration), {
          kind: "ok",
          schema: resolution.schema,
        });
        break;
      case "malformed":
        readings.set(entryKey(resolution.declaration), {
          kind: "malformed",
          error: resolution.error,
        });
        break;
      case "ambiguous":
        for (const claimant of resolution.claimants) {
          readings.set(
            entryKey(claimant.declaration),
            claimant.kind === "parsed"
              ? { kind: "ok", schema: claimant.schema }
              : { kind: "malformed", error: claimant.error },
          );
        }
        break;
      default:
        break;
    }
  }
  return readings;
}

// A `Type` node's declaration as the client shows it, in the inspector and
// the `Type` table alike: the server's reading when `types.list` covers the
// node, else the client's own.
export function declarationOf(
  server: ReadonlyMap<string, DeclarationReading>,
  node: { id: EntryId; payload: Body },
): DeclarationReading {
  return server.get(entryKey(node.id)) ?? readDeclaration(node.payload);
}

// The deepest run of nested `list` and `record` words in a shape.
export function shapeDepth(shape: Shape): number {
  switch (shape.kind) {
    case "list":
      return 1 + shapeDepth(shape.element);
    case "record":
      return (
        1 +
        Math.max(
          0,
          ...Object.values(shape.fields).map((decl) => shapeDepth(decl.shape)),
        )
      );
    default:
      return 0;
  }
}

// Writes the structured schema as the word encoding of FILES §2.6, the
// inverse of `parseDeclaration`. All five keys are written, empty where
// nothing is declared, so a form-built declaration equals a hand-written
// canonical one. Permission lists come out in code point order and without
// repeats, as the server's sets would; a repeated permission is malformed.
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
  return [...new Set(names)].sort(compareCodePoints);
}

function fieldDeclsEnvelope(decls: FieldDecls): RecordPayload {
  const value: Body = {};
  for (const [field, decl] of Object.entries(decls)) {
    setOwn(value, field, [decl.status, ...shapeWordList(decl.shape)]);
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
export function declarationSummary(reading: DeclarationReading): string {
  switch (reading.kind) {
    case "unknown_keys":
      return `unreadable declaration: ${reading.keys.join(", ")} ${reading.keys.length === 1 ? "is not a key" : "are not keys"}`;
    case "malformed":
      return `unreadable declaration: ${schemaErrorText(reading.error)}`;
    case "ok":
      break;
  }
  const { schema } = reading;
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
