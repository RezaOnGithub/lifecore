import { Fragment, useId, useState } from "react";
import type {
  Body,
  FieldDecl,
  FieldDecls,
  Payload,
  Shape,
} from "../contract.ts";
import { shapeWords } from "../model/payload.ts";
import {
  boolean,
  decimalError,
  defaultValue,
  getOwn,
  integer,
  kindOf,
  matchesShape,
  narrow,
  nowTimestamp,
  record,
  timestamp,
  timestampError,
  timestampHint,
} from "../model/values.ts";
import { StatusBadge } from "../views/Payload.tsx";
import { withItem, withMember } from "./collections.ts";
import {
  fieldRows,
  fitKeys,
  hasValueEditor,
  moveItem,
  nextKey,
  sameList,
  withField,
  withoutField,
} from "./helpers.ts";
import {
  keepFocus,
  NoEditor,
  TextValueInput,
  ValidatedInput,
} from "./inputs.tsx";
import { RawValueEditor } from "./RawJsonEditor.tsx";

// ShapeValueEditor, ListEditor, and FieldsEditor recurse into each other
// (a record holds fields, a list holds values), so they share this module.

function ListEditor({
  label,
  element,
  items,
  onChange,
}: {
  label: string;
  element: Shape;
  items: Payload[];
  onChange: (items: Payload[]) => void;
}) {
  const [keys, setKeys] = useState<readonly number[]>(() =>
    fitKeys([], items.length),
  );
  // An element being added that has no value yet (an integer or a
  // timestamp): it is appended once valid text is typed. Its key is the one
  // the appended element takes, so the input keeps focus across that.
  const [pendingKey, setPendingKey] = useState<number | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const rowKeys = fitKeys(keys, items.length);
  if (!sameList(rowKeys, keys)) {
    setKeys(rowKeys);
  }
  const commit = (next: Payload[], nextKeys: number[]) => {
    setKeys(nextKeys);
    setNote(null);
    onChange(next);
  };
  const add = () => {
    const key = nextKey(rowKeys);
    const initial = defaultValue(element);
    if (initial === null) {
      setPendingKey(key);
      setNote(null);
    } else {
      commit([...items, initial], [...rowKeys, key]);
    }
  };
  const rows = items.map((item, index) => ({
    key: rowKeys[index] ?? index,
    index,
    value: item as Payload | null,
  }));
  if (pendingKey !== null) {
    rows.push({ key: pendingKey, index: items.length, value: null });
  }
  return (
    <div className="list-editor">
      {rows.length === 0 && <span className="muted">empty list</span>}
      {rows.map(({ key, index, value }) => {
        const itemLabel = `${label}[${index}]`;
        const pending = value === null;
        return (
          <div className="row" key={key}>
            <span className="muted">[{index}]</span>
            <ShapeValueEditor
              label={itemLabel}
              shape={element}
              value={value}
              onChange={(changed) => {
                if (pending) {
                  setPendingKey(null);
                  commit([...items, changed], [...rowKeys, key]);
                } else {
                  onChange(
                    items.map((old, i) => (i === index ? changed : old)),
                  );
                }
              }}
              onAbandon={() => {
                setPendingKey(null);
                setNote(`reverted: ${itemLabel} was not added`);
              }}
            />
            <button
              type="button"
              aria-label={`move ${itemLabel} up`}
              disabled={pending || index === 0}
              onClick={() =>
                commit(
                  moveItem(items, index, index - 1),
                  moveItem(rowKeys, index, index - 1),
                )
              }
            >
              up
            </button>
            <button
              type="button"
              aria-label={`move ${itemLabel} down`}
              disabled={pending || index === items.length - 1}
              onClick={() =>
                commit(
                  moveItem(items, index, index + 1),
                  moveItem(rowKeys, index, index + 1),
                )
              }
            >
              down
            </button>
            <button
              type="button"
              aria-label={`remove ${itemLabel}`}
              onMouseDown={pending ? keepFocus : undefined}
              onClick={() => {
                if (pending) {
                  setPendingKey(null);
                } else {
                  commit(
                    items.filter((_, i) => i !== index),
                    rowKeys.filter((_, i) => i !== index),
                  );
                }
              }}
            >
              remove
            </button>
          </div>
        );
      })}
      <button
        type="button"
        aria-label={`add to ${label}`}
        disabled={pendingKey !== null}
        onClick={add}
      >
        add
      </button>
      {note !== null && <span className="edit-note">{note}</span>}
    </div>
  );
}

const INTEGER_PATTERN = "0|-?[1-9][0-9]*";

// The input for a declared shape. `value` is null for an integer or a
// timestamp made present without a value yet; nothing is committed until
// one is typed, and `onAbandon` runs if focus leaves first. A value of
// another kind than declared cannot be edited by that input, so it falls
// back to JSON with a note. A shape the form has no editor for shows its
// value read-only; the draft's raw JSON edits it.
export function ShapeValueEditor({
  label,
  shape,
  value,
  onChange,
  onAbandon,
}: {
  label: string;
  shape: Shape;
  value: Payload | null;
  onChange: (value: Payload) => void;
  onAbandon?: () => void;
}) {
  const abandon = onAbandon === undefined ? {} : { onAbandon };
  if (value !== null && !matchesShape(value, shape)) {
    return (
      <RawValueEditor
        label={label}
        value={value}
        onChange={onChange}
        note={`${kindOf(value)} value where ${shapeWords(shape)} is declared`}
      />
    );
  }
  if (!hasValueEditor(shape)) {
    return <NoEditor label={label} json={value} what={shapeWords(shape)} />;
  }
  switch (shape.kind) {
    case "text":
      return (
        <TextValueInput
          label={label}
          value={value === null ? "" : (narrow(value, "text") ?? "")}
          onChange={onChange}
        />
      );
    case "integer":
      return (
        <ValidatedInput
          label={label}
          value={
            value === null ? null : (narrow(value, "integer")?.value ?? "")
          }
          validate={decimalError}
          pattern={INTEGER_PATTERN}
          title="a decimal integer such as -42, within the signed 64-bit range"
          size={22}
          onCommit={(text) => onChange(integer(text))}
          {...abandon}
        />
      );
    case "boolean":
      return (
        <select
          aria-label={label}
          value={
            value !== null && narrow(value, "boolean")?.value ? "true" : "false"
          }
          onChange={(event) => onChange(boolean(event.target.value === "true"))}
        >
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      );
    case "timestamp":
      return (
        <>
          <ValidatedInput
            label={label}
            value={
              value === null ? null : (narrow(value, "timestamp")?.value ?? "")
            }
            validate={timestampError}
            advise={timestampHint}
            size={32}
            onCommit={(text) => onChange(timestamp(text))}
            {...abandon}
          />{" "}
          <button
            type="button"
            aria-label={`now (UTC) for ${label}`}
            onMouseDown={keepFocus}
            onClick={() => onChange(timestamp(nowTimestamp()))}
          >
            now (UTC)
          </button>
        </>
      );
    case "list":
      return (
        <ListEditor
          label={label}
          element={shape.element}
          items={value === null ? [] : (narrow(value, "list") ?? [])}
          onChange={onChange}
        />
      );
    case "record":
      return (
        <FieldsEditor
          body={value === null ? {} : (narrow(value, "record")?.value ?? {})}
          fields={shape.fields}
          prefix={label}
          onChange={(body) => onChange(record(body))}
        />
      );
    default:
      // `hasValueEditor` has sent every other shape to `NoEditor`.
      return null;
  }
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
  // Declared fields made present that wait for a first valid value; the
  // body does not have them until then.
  const [pending, setPending] = useState<ReadonlySet<string>>(() => new Set());
  // The value a field had when it was made absent, restored when it is
  // made present again.
  const [remembered, setRemembered] = useState<ReadonlyMap<string, Payload>>(
    () => new Map(),
  );
  const [notes, setNotes] = useState<ReadonlyMap<string, string>>(
    () => new Map(),
  );
  const rows = fieldRows(fields, body);
  const labelOf = (field: string) =>
    prefix === null ? field : `${prefix}.${field}`;
  const makePresent = (field: string, decl: FieldDecl) => {
    const initial = remembered.get(field) ?? defaultValue(decl.shape);
    setNotes(withMember(notes, field, null));
    if (initial === null) {
      setPending(withItem(pending, field, true));
    } else {
      onChange(withField(body, field, initial));
    }
  };
  const makeAbsent = (field: string) => {
    const value = getOwn(body, field);
    setPending(withItem(pending, field, false));
    setNotes(withMember(notes, field, null));
    if (value !== undefined) {
      setRemembered(withMember(remembered, field, value));
      onChange(withoutField(body, field));
    }
  };
  const commit = (field: string, value: Payload) => {
    setPending(withItem(pending, field, false));
    setNotes(withMember(notes, field, null));
    onChange(withField(body, field, value));
  };
  const abandon = (field: string) => {
    setPending(withItem(pending, field, false));
    setNotes(withMember(notes, field, "reverted to absent"));
  };
  const newDecl =
    fields === null || newField === "" ? undefined : getOwn(fields, newField);
  const addError =
    newField !== "" && (Object.hasOwn(body, newField) || pending.has(newField))
      ? `${newField} is already present`
      : newDecl !== undefined && !hasValueEditor(newDecl.shape)
        ? `${newField} is ${shapeWords(newDecl.shape)}, which the form cannot edit yet; add it in the raw JSON`
        : null;
  const add = () => {
    if (newField === "" || addError !== null) {
      return;
    }
    // A declared name becomes present as its checkbox would make it; any
    // other name starts as empty text to be edited as JSON.
    const decl = fields === null ? undefined : getOwn(fields, newField);
    if (decl === undefined) {
      onChange(withField(body, newField, ""));
    } else {
      makePresent(newField, decl);
    }
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
              const value = getOwn(body, field);
              const shown =
                value !== undefined || (decl !== null && pending.has(field));
              const note = notes.get(field);
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
                        checked={shown}
                        // Presence of a shape without an editor is changed
                        // in the raw JSON, with its value.
                        disabled={!hasValueEditor(decl.shape)}
                        title={
                          hasValueEditor(decl.shape)
                            ? undefined
                            : "read-only: edit this field in the raw JSON"
                        }
                        onMouseDown={keepFocus}
                        onChange={(event) =>
                          event.target.checked
                            ? makePresent(field, decl)
                            : makeAbsent(field)
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
                    {decl !== null && shown ? (
                      <ShapeValueEditor
                        label={label}
                        shape={decl.shape}
                        value={value ?? null}
                        onChange={(changed) => commit(field, changed)}
                        onAbandon={() => abandon(field)}
                      />
                    ) : decl === null && value !== undefined ? (
                      <RawValueEditor
                        label={label}
                        value={value}
                        onChange={(changed) =>
                          onChange(withField(body, field, changed))
                        }
                      />
                    ) : (
                      <span className="muted">absent</span>
                    )}
                    {note !== undefined && (
                      <span className="edit-note"> {note}</span>
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
