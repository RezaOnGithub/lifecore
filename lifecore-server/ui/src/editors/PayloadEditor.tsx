import { Fragment, useId, useState } from "react";
import type { Body, FieldDecls, TypeSchema } from "../contract.ts";
import { shapeWords } from "../model/payload.ts";
import { defaultValue, kindOf } from "../model/values.ts";
import { StatusBadge } from "../views/Payload.tsx";
import { fieldRows, withField, withoutField } from "./helpers.ts";
import { RawBodyEditor, RawValueEditor } from "./RawJsonEditor.tsx";
import { ShapeValueEditor } from "./ValueEditor.tsx";

export function ModeToggle({
  raw,
  onToggle,
  disabled = false,
}: {
  raw: boolean;
  onToggle: () => void;
  disabled?: boolean;
}) {
  return (
    <button type="button" disabled={disabled} onClick={onToggle}>
      {raw ? "Form" : "Raw JSON"}
    </button>
  );
}

// The rows of one body: every declared field, present or absent, then the
// undeclared ones as JSON. A record's body nests the same editor with the
// field path as `prefix`, so labels read `details.owner`.
export function FieldsEditor({
  body,
  fields,
  prefix,
  onChange,
}: {
  body: Body;
  fields: FieldDecls | null;
  prefix: string | null;
  onChange: (body: Body) => void;
}) {
  const id = useId();
  const [newField, setNewField] = useState("");
  const rows = fieldRows(fields, body);
  const labelOf = (field: string) =>
    prefix === null ? field : `${prefix}.${field}`;
  const addError =
    newField !== "" && body[newField] !== undefined
      ? `${newField} is already present`
      : null;
  const add = () => {
    if (newField === "" || addError !== null) {
      return;
    }
    // A declared name becomes present with its default; any other name
    // starts as empty text to be edited as JSON.
    const decl = fields?.[newField];
    onChange(withField(body, newField, decl ? defaultValue(decl.shape) : ""));
    setNewField("");
  };
  const firstUndeclared = rows.findIndex((row) => row.decl === null);
  return (
    <div className={prefix === null ? "fields-editor" : "fields-editor nested"}>
      {rows.length === 0 ? (
        <p className="muted">no fields</p>
      ) : (
        <table className="fields">
          <thead>
            <tr>
              <th scope="col">field</th>
              {fields !== null && <th scope="col">status</th>}
              <th scope="col">shape</th>
              <th scope="col">present</th>
              <th scope="col">value</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ field, decl }, index) => {
              const label = labelOf(field);
              const value = body[field];
              const row = (
                <tr key={field} className={decl ? undefined : "undeclared"}>
                  <th scope="row">{field}</th>
                  {fields !== null && (
                    <td>
                      {decl ? (
                        <StatusBadge status={decl.status} />
                      ) : (
                        <span className="badge warn">undeclared</span>
                      )}
                    </td>
                  )}
                  <td className="muted">
                    {decl
                      ? shapeWords(decl.shape)
                      : value === undefined
                        ? ""
                        : kindOf(value)}
                  </td>
                  <td>
                    {decl ? (
                      <input
                        type="checkbox"
                        aria-label={`${label} present`}
                        checked={value !== undefined}
                        onChange={(event) =>
                          onChange(
                            event.target.checked
                              ? withField(body, field, defaultValue(decl.shape))
                              : withoutField(body, field),
                          )
                        }
                      />
                    ) : (
                      <button
                        type="button"
                        aria-label={`remove ${label}`}
                        onClick={() => onChange(withoutField(body, field))}
                      >
                        remove
                      </button>
                    )}
                  </td>
                  <td className="value">
                    {value === undefined ? (
                      <span className="muted">absent</span>
                    ) : decl ? (
                      <ShapeValueEditor
                        label={label}
                        shape={decl.shape}
                        value={value}
                        onChange={(changed) =>
                          onChange(withField(body, field, changed))
                        }
                      />
                    ) : (
                      <RawValueEditor
                        label={label}
                        value={value}
                        onChange={(changed) =>
                          onChange(withField(body, field, changed))
                        }
                      />
                    )}
                  </td>
                </tr>
              );
              // With a schema, the undeclared rows sit under their own
              // heading; without one every row is undeclared and the
              // heading would say nothing.
              if (fields !== null && index === firstUndeclared) {
                return (
                  <Fragment key={field}>
                    <tr className="group">
                      <th scope="rowgroup" colSpan={5}>
                        undeclared
                      </th>
                    </tr>
                    {row}
                  </Fragment>
                );
              }
              return row;
            })}
          </tbody>
        </table>
      )}
      <div className="row">
        <label htmlFor={`${id}-new`}>
          {prefix === null ? "new field" : `new field in ${prefix}`}
        </label>
        <input
          id={`${id}-new`}
          type="text"
          size={16}
          value={newField}
          aria-invalid={addError !== null}
          onChange={(event) => setNewField(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              add();
            }
          }}
        />
        <button
          type="button"
          disabled={newField === "" || addError !== null}
          onClick={add}
        >
          {prefix === null ? "Add field" : `Add field to ${prefix}`}
        </button>
        {addError !== null && <span className="field-error">{addError}</span>}
      </div>
    </div>
  );
}

// A node payload: schema-driven rows when the type resolves to a
// declaration, else undeclared rows; or the whole body as JSON text.
export function PayloadEditor({
  payload,
  schema,
  onChange,
}: {
  payload: Body;
  schema: TypeSchema | null;
  onChange: (payload: Body) => void;
}) {
  const [raw, setRaw] = useState(false);
  return (
    <section className="payload-editor">
      <div className="row">
        <h4>payload</h4>
        {schema === null && (
          <span className="muted">
            no declared schema for this type; fields are edited as JSON
          </span>
        )}
        <ModeToggle raw={raw} onToggle={() => setRaw(!raw)} />
      </div>
      {raw ? (
        <RawBodyEditor payload={payload} onChange={onChange} />
      ) : (
        <FieldsEditor
          body={payload}
          fields={schema === null ? null : schema.fields}
          prefix={null}
          onChange={onChange}
        />
      )}
    </section>
  );
}
