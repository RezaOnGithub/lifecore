import { useId, useState } from "react";
import type { Body, Payload } from "../contract.ts";
import {
  validateCanonicalPayload,
  validateCanonicalValue,
} from "../model/values.ts";
import { liveDraft, type TextDraft } from "./helpers.ts";

type Parsed<T> = { ok: T } | { error: string };

function rowsFor(text: string): number {
  return Math.min(12, Math.max(2, text.split("\n").length));
}

function parseBody(text: string): Parsed<Body> {
  const result = validateCanonicalPayload(text);
  return "error" in result ? result : { ok: result.payload };
}

// A JSON textarea applied on blur. Invalid text stays on screen with its
// error and is not applied; the committed value is untouched until the
// text is canonical. The draft text is the caller's, so it can outlive
// this textarea (a mode switch) and the caller can see it is unapplied.
function RawJsonEditor<T>({
  label,
  value,
  parse,
  onChange,
  draft,
  onDraft,
  note,
}: {
  label: string;
  value: T;
  parse: (text: string) => Parsed<T>;
  onChange: (value: T) => void;
  draft: TextDraft | null;
  onDraft: (draft: TextDraft | null) => void;
  note?: string;
}) {
  const id = useId();
  // Compact JSON identifies the committed value; the textarea shows it
  // indented.
  const committed = JSON.stringify(value);
  const live = liveDraft(draft, committed);
  const text = live?.text ?? JSON.stringify(value, null, 2);
  const result = live === null ? null : parse(live.text);
  const error = result !== null && "error" in result ? result.error : null;
  return (
    <div className="raw-editor">
      {note !== undefined && <div className="edit-hint">{note}</div>}
      <textarea
        aria-label={label}
        value={text}
        rows={rowsFor(text)}
        cols={60}
        spellCheck={false}
        aria-invalid={error !== null}
        aria-describedby={error === null ? undefined : `${id}-error`}
        onChange={(event) =>
          onDraft({ text: event.target.value, base: committed })
        }
        onBlur={() => {
          if (result !== null && "ok" in result) {
            onDraft(null);
            if (JSON.stringify(result.ok) !== committed) {
              onChange(result.ok);
            }
          }
        }}
      />
      {error !== null && (
        <div id={`${id}-error`} className="field-error">
          not applied: {error}
        </div>
      )}
    </div>
  );
}

// The raw text of a whole payload, held by the payload or declaration
// editor so it survives the switch between modes.
export interface RawBody {
  draft: TextDraft | null;
  setDraft: (draft: TextDraft | null) => void;
  // Unapplied text that is not a canonical payload: the form cannot show
  // it, so leaving raw mode would lose it.
  blocked: boolean;
  // Applies unapplied text that parses. False when the text cannot be
  // applied, and raw mode must stay.
  flush: () => boolean;
}

export function useRawBody(
  payload: Body,
  onChange: (payload: Body) => void,
): RawBody {
  const [draft, setDraft] = useState<TextDraft | null>(null);
  const committed = JSON.stringify(payload);
  const live = liveDraft(draft, committed);
  const result = live === null ? null : parseBody(live.text);
  return {
    draft,
    setDraft,
    blocked: result !== null && "error" in result,
    flush: () => {
      if (result === null) {
        return true;
      }
      if ("error" in result) {
        return false;
      }
      setDraft(null);
      if (JSON.stringify(result.ok) !== committed) {
        onChange(result.ok);
      }
      return true;
    },
  };
}

export function RawBodyEditor({
  payload,
  onChange,
  raw,
  onType,
  note,
}: {
  payload: Body;
  onChange: (payload: Body) => void;
  raw: RawBody;
  // Called on each keystroke, beside `raw.setDraft`.
  onType?: () => void;
  note?: string;
}) {
  return (
    <RawJsonEditor
      label="payload JSON"
      value={payload}
      parse={parseBody}
      onChange={onChange}
      draft={raw.draft}
      onDraft={(draft) => {
        raw.setDraft(draft);
        if (draft !== null) {
          onType?.();
        }
      }}
      {...(note === undefined ? {} : { note })}
    />
  );
}

// One undeclared field, or a value of another kind than declared.
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
  const [draft, setDraft] = useState<TextDraft | null>(null);
  return (
    <RawJsonEditor
      label={label}
      value={value}
      parse={(text) => {
        const result = validateCanonicalValue(text, label);
        return "error" in result ? result : { ok: result.value };
      }}
      onChange={onChange}
      draft={draft}
      onDraft={setDraft}
      {...(note === undefined ? {} : { note })}
    />
  );
}
