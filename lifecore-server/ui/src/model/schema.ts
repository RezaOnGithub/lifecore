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

export type DeclarationReading =
  | { kind: "ok"; schema: TypeSchema }
  | { kind: "unknown_keys"; keys: string[] }
  | { kind: "malformed"; path: string; message: string };

// A path segment as the server reports it: a field name or a list position.
type Part = string | number;

class Malformed extends Error {
  readonly path: Part[];
  constructor(message: string, path: Part[] = []) {
    super(message);
    this.path = path;
  }
  // Errors gain their path from the inside out, as the server's do.
  at(part: Part): Malformed {
    return new Malformed(this.message, [part, ...this.path]);
  }
}

function formatPath(path: readonly Part[]): string {
  let text = "";
  for (const part of path) {
    text +=
      typeof part === "number" ? `[${part}]` : text === "" ? part : `.${part}`;
  }
  return text;
}

function describe(value: Payload): string {
  const kind = kindOf(value);
  return kind === "text" ? `text ${JSON.stringify(value)}` : kind;
}

// Reads a `Type` declaration payload (FILES §2.6) the way the server's
// `Type::from_body` does, in the same order: keys, then `name`, `fields`,
// and the three permission lists. A failure names the offending element's
// path, with list positions for words.
export function readDeclaration(payload: Body): DeclarationReading {
  const unknown = Object.keys(payload).filter(
    (key) => !DECLARATION_KEYS.includes(key),
  );
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
        path: formatPath(failure.path),
        message: failure.message,
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
    throw new Malformed("name is required", ["name"]);
  }
  const name = payload.name as Payload;
  if (typeof name !== "string") {
    throw new Malformed(`expected text, found ${describe(name)}`, ["name"]);
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
    throw new Malformed(`expected a list, found ${describe(value)}`, [key]);
  }
  const names = new Set<string>();
  value.forEach((item, index) => {
    if (typeof item !== "string") {
      throw new Malformed(`expected text, found ${describe(item)}`, [
        key,
        index,
      ]);
    }
    if (names.has(item)) {
      throw new Malformed(`${JSON.stringify(item)} is repeated`, [key, index]);
    }
    names.add(item);
  });
  return [...names].sort(compareCodePoints);
}

function readFieldDecls(value: Payload, depth: number): FieldDecls {
  if (typeof value !== "object" || Array.isArray(value)) {
    throw new Malformed(`expected a record envelope, found ${describe(value)}`);
  }
  if (value.kind !== "record") {
    throw new Malformed(`expected a record envelope, found ${value.kind}`);
  }
  const decls: FieldDecls = {};
  for (const [field, words] of Object.entries(value.value)) {
    setOwn(
      decls,
      field,
      within(field, () => readFieldDecl(words, depth)),
    );
  }
  return decls;
}

// `[presence, shape words...]`; a `record` word takes the next element as
// its field declarations, so it ends the list.
function readFieldDecl(value: Payload, depth: number): FieldDecl {
  if (!Array.isArray(value)) {
    throw new Malformed(`expected a list of words, found ${describe(value)}`);
  }
  const status = word(value, 0, "a presence word");
  if (!PRESENCE_WORDS.includes(status as FieldStatus)) {
    throw new Malformed(
      `unknown presence ${JSON.stringify(status)}; expected ${PRESENCE_WORDS.join(", ")}`,
      [0],
    );
  }
  const [shape, end] = readShape(value, 1, depth);
  if (end < value.length) {
    throw new Malformed("unexpected element after the shape", [end]);
  }
  return { status: status as FieldStatus, shape };
}

function word(words: readonly Payload[], index: number, what: string): string {
  const value = words[index];
  if (value === undefined) {
    throw new Malformed(`missing ${what}`, [index]);
  }
  if (typeof value !== "string") {
    throw new Malformed(`expected ${what}, found ${describe(value)}`, [index]);
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
  const head = word(words, start, "a shape word");
  if ((head === "list" || head === "record") && depth >= MAX_SHAPE_DEPTH) {
    throw new Malformed(
      `more than ${MAX_SHAPE_DEPTH} nested list and record words`,
      [start],
    );
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
        throw new Malformed(
          "record must be followed by a record envelope of field declarations",
          [next],
        );
      }
      const fields = within(next, () => readFieldDecls(envelope, depth + 1));
      return [{ kind: "record", fields }, next + 1];
    }
    default:
      throw new Malformed(
        `unknown shape word ${JSON.stringify(head)}; expected text, integer, boolean, timestamp, list, or record`,
        [start],
      );
  }
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
