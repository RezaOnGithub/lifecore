import { useState } from "react";
import type { Payload, Shape } from "../contract.ts";
import { shapeWords } from "../model/payload.ts";
import {
  boolean,
  decimalError,
  defaultValue,
  integer,
  kindOf,
  narrow,
  nowTimestamp,
  record,
  timestamp,
  timestampError,
} from "../model/values.ts";
import { moveItem } from "./helpers.ts";
import { FieldsEditor } from "./PayloadEditor.tsx";
import { RawValueEditor } from "./RawJsonEditor.tsx";

// A text input committed only while valid. Invalid text stays on screen
// with its error until it is fixed or the committed value changes under it.
export function ValidatedInput({
  label,
  value,
  validate,
  onCommit,
  inputMode,
  size,
}: {
  label: string;
  value: string;
  validate: (text: string) => string | null;
  onCommit: (text: string) => void;
  inputMode?: "numeric" | "text";
  size?: number;
}) {
  const [draft, setDraft] = useState<{ text: string; base: string } | null>(
    null,
  );
  const text = draft !== null && draft.base === value ? draft.text : value;
  const error = validate(text);
  return (
    <>
      <input
        type="text"
        aria-label={label}
        value={text}
        aria-invalid={error !== null}
        {...(inputMode === undefined ? {} : { inputMode })}
        {...(size === undefined ? {} : { size })}
        onChange={(event) => {
          const next = event.target.value;
          setDraft({ text: next, base: value });
          if (validate(next) === null) {
            onCommit(next);
          }
        }}
      />
      {error !== null && <span className="field-error">{error}</span>}
    </>
  );
}

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
  return (
    <div className="list-editor">
      {items.length === 0 && <span className="muted">empty list</span>}
      {items.map((item, index) => (
        <div
          className="row"
          // Elements are positional; a removed one shifts the rest.
          // biome-ignore lint/suspicious/noArrayIndexKey: position is the identity
          key={index}
        >
          <span className="muted">[{index}]</span>
          <ShapeValueEditor
            label={`${label}[${index}]`}
            shape={element}
            value={item}
            onChange={(changed) =>
              onChange(items.map((old, i) => (i === index ? changed : old)))
            }
          />
          <button
            type="button"
            aria-label={`move ${label}[${index}] up`}
            disabled={index === 0}
            onClick={() => onChange(moveItem(items, index, index - 1))}
          >
            up
          </button>
          <button
            type="button"
            aria-label={`move ${label}[${index}] down`}
            disabled={index === items.length - 1}
            onClick={() => onChange(moveItem(items, index, index + 1))}
          >
            down
          </button>
          <button
            type="button"
            aria-label={`remove ${label}[${index}]`}
            onClick={() => onChange(items.filter((_, i) => i !== index))}
          >
            remove
          </button>
        </div>
      ))}
      <button
        type="button"
        aria-label={`add to ${label}`}
        onClick={() => onChange([...items, defaultValue(element)])}
      >
        add
      </button>
    </div>
  );
}

// The input for a declared shape. A value of another kind than declared
// cannot be edited by that input, so it falls back to JSON with a note.
export function ShapeValueEditor({
  label,
  shape,
  value,
  onChange,
}: {
  label: string;
  shape: Shape;
  value: Payload;
  onChange: (value: Payload) => void;
}) {
  if (kindOf(value) !== shape.kind) {
    return (
      <RawValueEditor
        label={label}
        value={value}
        onChange={onChange}
        note={`${kindOf(value)} value where ${shapeWords(shape)} is declared`}
      />
    );
  }
  switch (shape.kind) {
    case "text":
      return (
        <input
          type="text"
          aria-label={label}
          size={40}
          value={narrow(value, "text") ?? ""}
          onChange={(event) => onChange(event.target.value)}
        />
      );
    case "integer":
      return (
        <ValidatedInput
          label={label}
          value={narrow(value, "integer")?.value ?? ""}
          validate={decimalError}
          inputMode="numeric"
          size={22}
          onCommit={(text) => onChange(integer(text))}
        />
      );
    case "boolean":
      return (
        <select
          aria-label={label}
          value={narrow(value, "boolean")?.value ? "true" : "false"}
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
            value={narrow(value, "timestamp")?.value ?? ""}
            validate={timestampError}
            size={32}
            onCommit={(text) => onChange(timestamp(text))}
          />{" "}
          <button
            type="button"
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
          items={narrow(value, "list") ?? []}
          onChange={onChange}
        />
      );
    case "record":
      return (
        <FieldsEditor
          body={narrow(value, "record")?.value ?? {}}
          fields={shape.fields}
          prefix={label}
          onChange={(body) => onChange(record(body))}
        />
      );
  }
}
