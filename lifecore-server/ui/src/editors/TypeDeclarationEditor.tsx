import { useId, useRef, useState } from "react";
import type {
  Body,
  FieldDecl,
  FieldDecls,
  FieldStatus,
  Shape,
  TypeSchema,
} from "../contract.ts";
import {
  DECLARATION_KEYS,
  type DeclarationReading,
  declarationBody,
  declarationForForm,
  MAX_SHAPE_DEPTH,
  PRESENCE_WORDS,
  schemaErrorMessage,
  schemaPathText,
  shapeDepth,
} from "../model/schema.ts";
import type { PayloadKind } from "../model/values.ts";
import {
  initialShape,
  liveDraft,
  namedKeys,
  parseTags,
  renameField,
  renameKey,
  sameMap,
  shapeOfKind,
  type TextDraft,
  withField,
  withoutField,
} from "./helpers.ts";
import { ModeToggle, ValidatedInput } from "./inputs.tsx";
import { RawBodyEditor, useRawBody } from "./RawJsonEditor.tsx";

const KINDS: readonly PayloadKind[] = [
  "text",
  "integer",
  "boolean",
  "timestamp",
  "list",
  "record",
];

function ShapeBuilder({
  field,
  shape,
  onChange,
}: {
  field: string;
  shape: Shape;
  onChange: (shape: Shape) => void;
}) {
  return (
    <div className="shape-builder">
      <select
        aria-label={`${field} shape`}
        value={shape.kind}
        onChange={(event) =>
          onChange(shapeOfKind(event.target.value as PayloadKind, shape))
        }
      >
        {KINDS.map((kind) => (
          <option key={kind} value={kind}>
            {kind}
          </option>
        ))}
      </select>
      {shape.kind === "list" && (
        <>
          <span className="muted"> of </span>
          <ShapeBuilder
            field={`${field} element`}
            shape={shape.element}
            onChange={(element) => onChange({ kind: "list", element })}
          />
        </>
      )}
      {shape.kind === "record" && (
        <FieldDeclsTable
          fields={shape.fields}
          prefix={field}
          onChange={(fields) => onChange({ kind: "record", fields })}
        />
      )}
    </div>
  );
}

// A name, a presence, and a shape, added with Enter or the button; the name
// input takes focus again for the next one.
function AddDeclaration({
  fields,
  prefix,
  onAdd,
}: {
  fields: FieldDecls;
  prefix: string | null;
  onAdd: (field: string, decl: FieldDecl) => void;
}) {
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState("");
  const [status, setStatus] = useState<FieldStatus>("optional");
  const [kind, setKind] = useState<PayloadKind>("text");
  const what =
    prefix === null
      ? "new field declaration"
      : `new field declaration in ${prefix}`;
  const error =
    name !== "" && Object.hasOwn(fields, name)
      ? `another field is named ${name}`
      : null;
  const submit = () => {
    if (name === "" || error !== null) {
      return;
    }
    onAdd(name, { status, shape: initialShape(kind) });
    setName("");
    input.current?.focus();
  };
  return (
    <div className="row add-decl">
      <label htmlFor={`${id}-name`}>{what}</label>
      <input
        ref={input}
        id={`${id}-name`}
        type="text"
        size={16}
        value={name}
        aria-invalid={error !== null}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            submit();
          }
        }}
      />
      <select
        aria-label={`${what} presence`}
        value={status}
        onChange={(event) => setStatus(event.target.value as FieldStatus)}
      >
        {PRESENCE_WORDS.map((word) => (
          <option key={word} value={word}>
            {word}
          </option>
        ))}
      </select>
      <select
        aria-label={`${what} shape`}
        value={kind}
        onChange={(event) => setKind(event.target.value as PayloadKind)}
      >
        {KINDS.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
      <button
        type="button"
        disabled={name === "" || error !== null}
        onClick={submit}
      >
        {prefix === null
          ? "Add field declaration"
          : `Add field declaration to ${prefix}`}
      </button>
      {error !== null && <span className="field-error">{error}</span>}
    </div>
  );
}

function FieldDeclsTable({
  fields,
  prefix,
  onChange,
}: {
  fields: FieldDecls;
  prefix: string | null;
  onChange: (fields: FieldDecls) => void;
}) {
  const entries = Object.entries(fields);
  // Rows are keyed by a local id that follows a rename, so the name input
  // keeps focus while its field's name, and maybe its place, changes.
  const [keys, setKeys] = useState<ReadonlyMap<string, number>>(
    () => new Map(),
  );
  const rowKeys = namedKeys(
    entries.map(([field]) => field),
    keys,
  );
  if (!sameMap(rowKeys, keys)) {
    setKeys(rowKeys);
  }
  const labelOf = (field: string) =>
    prefix === null ? field : `${prefix}.${field}`;
  return (
    <div className={prefix === null ? "decls-editor" : "decls-editor nested"}>
      {entries.length === 0 ? (
        <span className="muted">no fields</span>
      ) : (
        <table className="fields">
          <thead>
            <tr>
              <th scope="col">field</th>
              <th scope="col">presence</th>
              <th scope="col">shape</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {entries.map(([field, decl]) => (
              <tr key={rowKeys.get(field)}>
                <td>
                  <ValidatedInput
                    label={`${labelOf(field)} name`}
                    value={field}
                    size={16}
                    validate={(text) =>
                      text === ""
                        ? "name required"
                        : text !== field && Object.hasOwn(fields, text)
                          ? `another field is named ${text}`
                          : null
                    }
                    onCommit={(text) => {
                      setKeys(renameKey(rowKeys, field, text));
                      onChange(renameField(fields, field, text));
                    }}
                  />
                </td>
                <td>
                  <select
                    aria-label={`${labelOf(field)} presence`}
                    value={decl.status}
                    onChange={(event) =>
                      onChange(
                        withField(fields, field, {
                          ...decl,
                          status: event.target.value as FieldStatus,
                        }),
                      )
                    }
                  >
                    {PRESENCE_WORDS.map((word) => (
                      <option key={word} value={word}>
                        {word}
                      </option>
                    ))}
                  </select>
                </td>
                <td>
                  <ShapeBuilder
                    field={labelOf(field)}
                    shape={decl.shape}
                    onChange={(shape) =>
                      onChange(withField(fields, field, { ...decl, shape }))
                    }
                  />
                  {prefix === null &&
                    shapeDepth(decl.shape) > MAX_SHAPE_DEPTH && (
                      <span className="field-error">
                        more than {MAX_SHAPE_DEPTH} nested list and record
                        words; the server rejects the declaration
                      </span>
                    )}
                </td>
                <td>
                  <button
                    type="button"
                    aria-label={`remove ${labelOf(field)}`}
                    onClick={() => onChange(withoutField(fields, field))}
                  >
                    remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <AddDeclaration
        fields={fields}
        prefix={prefix}
        onAdd={(field, decl) => onChange(withField(fields, field, decl))}
      />
    </div>
  );
}

// Comma-separated names. The text is local while typing, since the
// committed list comes back sorted and without the comma being typed; when
// focus leaves, the input shows the committed list.
function TagInput({
  label,
  values,
  onChange,
}: {
  label: string;
  values: string[];
  onChange: (values: string[]) => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState<TextDraft | null>(null);
  const [note, setNote] = useState<TextDraft | null>(null);
  const committed = values.join(",");
  const live = liveDraft(draft, committed);
  const text = live?.text ?? values.join(", ");
  const repeated = live === null ? [] : parseTags(live.text).repeated;
  const shownNote =
    repeated.length > 0
      ? `${repeated.join(", ")} repeated; written once`
      : (liveDraft(note, committed)?.text ?? null);
  return (
    <>
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="text"
        size={28}
        value={text}
        placeholder="comma-separated"
        onChange={(event) => {
          const next = event.target.value;
          const parsed = parseTags(next);
          setDraft({ text: next, base: parsed.tags.join(",") });
          setNote(null);
          onChange(parsed.tags);
        }}
        onBlur={() => {
          if (live !== null) {
            setDraft(null);
            if (repeated.length > 0) {
              setNote({
                text: `removed repeated ${repeated.join(", ")}`,
                base: committed,
              });
            }
          }
        }}
      />
      {shownNote !== null && <span className="edit-note">{shownNote}</span>}
    </>
  );
}

function nameProblem(name: string): string | null {
  if (name === "") {
    return "a declaration needs a name";
  }
  return name === "Type"
    ? "Type is the built-in metatype; a declaration may not claim it"
    : null;
}

function DeclarationForm({
  schema,
  named,
  onChange,
}: {
  schema: TypeSchema;
  // False for a payload without `name` yet, such as a new `Type` node's.
  named: boolean;
  onChange: (schema: TypeSchema) => void;
}) {
  const id = useId();
  const problem = nameProblem(schema.name);
  return (
    <>
      {!named && (
        <p className="muted">
          A new declaration: its payload is empty until the first edit, which
          writes all five keys.
        </p>
      )}
      <div className="row">
        <label htmlFor={`${id}-name`}>declared name</label>
        <input
          id={`${id}-name`}
          type="text"
          size={24}
          value={schema.name}
          aria-invalid={problem !== null}
          onChange={(event) =>
            onChange({ ...schema, name: event.target.value })
          }
        />
        {problem !== null && <span className="field-error">{problem}</span>}
      </div>
      <FieldDeclsTable
        fields={schema.fields}
        prefix={null}
        onChange={(fields) => onChange({ ...schema, fields })}
      />
      <div className="row">
        <TagInput
          label="outgoing"
          values={schema.outgoing}
          onChange={(outgoing) => onChange({ ...schema, outgoing })}
        />
        <TagInput
          label="incoming"
          values={schema.incoming}
          onChange={(incoming) => onChange({ ...schema, incoming })}
        />
        <TagInput
          label="successors"
          values={schema.successors}
          onChange={(successors) => onChange({ ...schema, successors })}
        />
      </div>
    </>
  );
}

function rawNote(reading: DeclarationReading): string | undefined {
  switch (reading.kind) {
    case "ok":
      return undefined;
    case "unknown_keys":
      return `The payload has ${reading.keys.length === 1 ? "a key" : "keys"} outside the declaration: ${reading.keys.join(", ")}. A declaration has only ${DECLARATION_KEYS.join(", ")} (FILES §2.6), and the form would drop the rest, so it returns once they are gone.`;
    case "malformed": {
      const path = schemaPathText(reading.error.path);
      return `Outside the declaration grammar at ${path === "" ? "the payload" : path}: ${schemaErrorMessage(reading.error)} (FILES §2.6). The form returns once it parses.`;
    }
  }
}

// The payload of a `Type` node (FILES §2.6), edited structurally and written
// back as the word encoding. A payload the form cannot represent exactly
// (outside the grammar, or with other keys) is edited as JSON.
export function TypeDeclarationEditor({
  payload,
  onChange,
}: {
  payload: Body;
  onChange: (payload: Body) => void;
}) {
  const [raw, setRaw] = useState(false);
  const rawBody = useRawBody(payload, onChange);
  const reading = declarationForForm(payload);
  const showRaw = raw || reading.kind !== "ok";
  return (
    <section className="payload-editor">
      <div className="row">
        <h4>type declaration</h4>
        <ModeToggle
          raw={showRaw}
          disabled={showRaw && (reading.kind !== "ok" || rawBody.blocked)}
          onToggle={() => {
            if (!showRaw) {
              setRaw(true);
            } else if (rawBody.flush()) {
              setRaw(false);
            }
          }}
        />
      </div>
      {reading.kind === "ok" && !showRaw ? (
        <DeclarationForm
          schema={reading.schema}
          named={Object.hasOwn(payload, "name")}
          onChange={(changed) => onChange(declarationBody(changed))}
        />
      ) : (
        <RawBodyEditor
          payload={payload}
          onChange={onChange}
          raw={rawBody}
          // Typing in raw mode keeps it, so applying text that happens to
          // parse does not swap the textarea for the form mid-edit.
          onType={() => setRaw(true)}
          {...(reading.kind === "ok" ? {} : { note: rawNote(reading) ?? "" })}
        />
      )}
    </section>
  );
}
