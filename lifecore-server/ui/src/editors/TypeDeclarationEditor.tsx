import { useId, useState } from "react";
import type {
  Body,
  FieldDecls,
  FieldStatus,
  Shape,
  TypeSchema,
} from "../contract.ts";
import { declarationBody, parseDeclaration } from "../model/schema.ts";
import type { PayloadKind } from "../model/values.ts";
import {
  freshFieldName,
  parseTags,
  renameField,
  withoutDecl,
} from "./helpers.ts";
import { ModeToggle } from "./PayloadEditor.tsx";
import { RawBodyEditor } from "./RawJsonEditor.tsx";
import { ValidatedInput } from "./ValueEditor.tsx";

const STATUSES: readonly FieldStatus[] = [
  "mandatory",
  "encouraged",
  "optional",
  "deprecated",
];
const KINDS: readonly PayloadKind[] = [
  "text",
  "integer",
  "boolean",
  "timestamp",
  "list",
  "record",
];

// Choosing `list` wraps the current shape, so "list of" nests; a record
// starts empty; a scalar replaces whatever was there.
function shapeOfKind(kind: PayloadKind, previous: Shape): Shape {
  switch (kind) {
    case "list":
      return { kind, element: previous };
    case "record":
      return { kind, fields: {} };
    default:
      return { kind };
  }
}

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
            {entries.map(([field, decl], index) => (
              <tr
                // Rows keep their position across a rename, so the name
                // input keeps focus while its key changes.
                // biome-ignore lint/suspicious/noArrayIndexKey: position is the identity
                key={index}
              >
                <td>
                  <ValidatedInput
                    label={`${labelOf(field)} name`}
                    value={field}
                    size={16}
                    validate={(text) =>
                      text === ""
                        ? "name required"
                        : text !== field && text in fields
                          ? `another field is named ${text}`
                          : null
                    }
                    onCommit={(text) =>
                      onChange(renameField(fields, field, text))
                    }
                  />
                </td>
                <td>
                  <select
                    aria-label={`${labelOf(field)} presence`}
                    value={decl.status}
                    onChange={(event) =>
                      onChange({
                        ...fields,
                        [field]: {
                          ...decl,
                          status: event.target.value as FieldStatus,
                        },
                      })
                    }
                  >
                    {STATUSES.map((status) => (
                      <option key={status} value={status}>
                        {status}
                      </option>
                    ))}
                  </select>
                </td>
                <td>
                  <ShapeBuilder
                    field={labelOf(field)}
                    shape={decl.shape}
                    onChange={(shape) =>
                      onChange({ ...fields, [field]: { ...decl, shape } })
                    }
                  />
                </td>
                <td>
                  <button
                    type="button"
                    aria-label={`remove ${labelOf(field)}`}
                    onClick={() => onChange(withoutDecl(fields, field))}
                  >
                    remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <button
        type="button"
        onClick={() =>
          onChange({
            ...fields,
            [freshFieldName(fields)]: {
              status: "optional",
              shape: { kind: "text" },
            },
          })
        }
      >
        {prefix === null
          ? "Add field declaration"
          : `Add field declaration to ${prefix}`}
      </button>
    </div>
  );
}

// Comma-separated names. The text is local while typing, since the
// committed list comes back sorted and without the comma being typed.
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
  const [draft, setDraft] = useState<{ text: string; base: string } | null>(
    null,
  );
  const base = values.join(",");
  const text =
    draft !== null && draft.base === base ? draft.text : values.join(", ");
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
          setDraft({ text: next, base: parsed.join(",") });
          onChange(parsed);
        }}
      />
    </>
  );
}

function DeclarationForm({
  schema,
  onChange,
}: {
  schema: TypeSchema;
  onChange: (schema: TypeSchema) => void;
}) {
  const id = useId();
  return (
    <>
      <div className="row">
        <label htmlFor={`${id}-name`}>declared name</label>
        <input
          id={`${id}-name`}
          type="text"
          size={24}
          value={schema.name}
          onChange={(event) =>
            onChange({ ...schema, name: event.target.value })
          }
        />
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

// The payload of a `Type` node (FILES §2.6), edited structurally and written
// back as the word encoding. A payload outside the grammar can only be
// edited as JSON until it parses.
export function TypeDeclarationEditor({
  payload,
  onChange,
}: {
  payload: Body;
  onChange: (payload: Body) => void;
}) {
  const [raw, setRaw] = useState(false);
  const schema = parseDeclaration(payload);
  const showRaw = raw || schema === null;
  return (
    <section className="payload-editor">
      <div className="row">
        <h4>type declaration</h4>
        <ModeToggle
          raw={showRaw}
          disabled={schema === null}
          onToggle={() => setRaw(!showRaw)}
        />
      </div>
      {schema === null ? (
        <RawBodyEditor
          payload={payload}
          onChange={onChange}
          note="this payload is outside the declaration grammar (FILES §2.6); the form returns once it parses"
        />
      ) : showRaw ? (
        <RawBodyEditor payload={payload} onChange={onChange} />
      ) : (
        <DeclarationForm
          schema={schema}
          onChange={(changed) => onChange(declarationBody(changed))}
        />
      )}
    </section>
  );
}
