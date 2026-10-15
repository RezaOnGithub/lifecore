import { useState } from "react";
import type { Body, Payload } from "../contract.ts";
import {
  validateCanonicalPayload,
  validateCanonicalValue,
} from "../model/values.ts";

interface Draft {
  text: string;
  // The committed JSON the draft was typed over; a different committed
  // value means someone else changed it and the draft is stale.
  base: string;
  error: string | null;
}

function rowsFor(text: string): number {
  return Math.min(12, Math.max(2, text.split("\n").length));
}

// A JSON textarea applied on blur. Invalid text stays on screen with its
// error; the committed value is untouched until the text is canonical.
function RawJsonEditor<T>({
  label,
  value,
  parse,
  onChange,
  note,
}: {
  label: string;
  value: T;
  parse: (text: string) => { ok: T } | { error: string };
  onChange: (value: T) => void;
  note?: string;
}) {
  const committed = JSON.stringify(value);
  const [draft, setDraft] = useState<Draft | null>(null);
  const live = draft !== null && draft.base === committed ? draft : null;
  const text = live === null ? JSON.stringify(value, null, 2) : live.text;
  return (
    <div className="raw-editor">
      {note !== undefined && <div className="muted">{note}</div>}
      <textarea
        aria-label={label}
        value={text}
        rows={rowsFor(text)}
        cols={60}
        spellCheck={false}
        aria-invalid={live !== null && live.error !== null}
        onChange={(event) =>
          setDraft({ text: event.target.value, base: committed, error: null })
        }
        onBlur={() => {
          if (live === null) {
            return;
          }
          const result = parse(live.text);
          if ("error" in result) {
            setDraft({ ...live, error: result.error });
          } else {
            setDraft(null);
            onChange(result.ok);
          }
        }}
      />
      {live !== null && live.error !== null && (
        <div className="field-error" role="alert">
          {live.error}
        </div>
      )}
    </div>
  );
}

export function RawBodyEditor({
  payload,
  onChange,
  note,
}: {
  payload: Body;
  onChange: (payload: Body) => void;
  note?: string;
}) {
  return (
    <RawJsonEditor
      label="payload JSON"
      value={payload}
      parse={(text) => {
        const result = validateCanonicalPayload(text);
        return "error" in result ? result : { ok: result.payload };
      }}
      onChange={onChange}
      {...(note === undefined ? {} : { note })}
    />
  );
}

export function RawValueEditor({
  label,
  value,
  onChange,
  note,
}: {
  label: string;
  value: Payload;
  onChange: (value: Payload) => void;
  note?: string;
}) {
  return (
    <RawJsonEditor
      label={label}
      value={value}
      parse={(text) => {
        const result = validateCanonicalValue(text, label);
        return "error" in result ? result : { ok: result.value };
      }}
      onChange={onChange}
      {...(note === undefined ? {} : { note })}
    />
  );
}
