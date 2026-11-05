import { type MouseEvent, useId, useState } from "react";
import { lineCount, liveDraft, type TextDraft } from "./helpers.ts";

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
    <button
      type="button"
      disabled={disabled}
      title={
        disabled
          ? "the JSON text is not applied; fix or undo it to return to the form"
          : undefined
      }
      onClick={onToggle}
    >
      {raw ? "Form" : "Raw JSON"}
    </button>
  );
}

// For buttons acting on a field whose input has focus: a mouse press would
// otherwise blur the input first, and its blur handling (a revert, or a
// pending field going back to absent) would run before the click.
export function keepFocus(event: MouseEvent) {
  event.preventDefault();
}

// Text values keep their newlines: a textarea that grows a row per line.
export function TextValueInput({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <textarea
      className="text-value"
      aria-label={label}
      value={value}
      rows={lineCount(value)}
      cols={40}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

// A text input committed on each keystroke while valid. Invalid text stays
// on screen with its error while the input has focus; when focus leaves,
// the input goes back to the committed value with a note, so what the
// screen shows is what Check and Apply send. `value` is null while nothing
// is committed, for a field made present that waits for its first valid
// value; leaving it then calls `onAbandon`.
export function ValidatedInput({
  label,
  value,
  validate,
  onCommit,
  onAbandon,
  advise,
  pattern,
  title,
  size,
}: {
  label: string;
  value: string | null;
  validate: (text: string) => string | null;
  onCommit: (text: string) => void;
  onAbandon?: () => void;
  // Advice on valid text, never blocking it.
  advise?: (text: string) => string | null;
  pattern?: string;
  title?: string;
  size?: number;
}) {
  const id = useId();
  const [draft, setDraft] = useState<TextDraft<string | null> | null>(null);
  const [note, setNote] = useState<TextDraft<string | null> | null>(null);
  const live = liveDraft(draft, value);
  const text = live?.text ?? value ?? "";
  const error =
    live !== null
      ? validate(live.text)
      : value !== null
        ? validate(value)
        : null;
  const advice = error === null && advise !== undefined ? advise(text) : null;
  const shownNote = liveDraft(note, value)?.text ?? null;
  const messages: [string, string, string][] = [];
  if (error !== null) {
    messages.push([`${id}-error`, "field-error", error]);
  }
  if (value === null && live === null) {
    messages.push([`${id}-pending`, "muted", "absent until a value is typed"]);
  }
  if (advice !== null) {
    messages.push([`${id}-advice`, "edit-hint", advice]);
  }
  if (shownNote !== null) {
    messages.push([`${id}-note`, "edit-note", shownNote]);
  }
  return (
    <>
      <input
        type="text"
        aria-label={label}
        value={text}
        aria-invalid={error !== null}
        aria-describedby={
          messages.length === 0
            ? undefined
            : messages.map(([messageId]) => messageId).join(" ")
        }
        inputMode="text"
        {...(pattern === undefined ? {} : { pattern })}
        {...(title === undefined ? {} : { title })}
        {...(size === undefined ? {} : { size })}
        onChange={(event) => {
          const next = event.target.value;
          setNote(null);
          if (validate(next) === null) {
            setDraft(null);
            if (next !== value) {
              onCommit(next);
            }
          } else {
            setDraft({ text: next, base: value });
          }
        }}
        onBlur={() => {
          if (value === null) {
            setDraft(null);
            onAbandon?.();
          } else if (live !== null) {
            setDraft(null);
            setNote({ text: `reverted to ${value}`, base: value });
          }
        }}
      />
      {messages.map(([messageId, className, message]) => (
        <span key={messageId} id={messageId} className={className}>
          {message}
        </span>
      ))}
    </>
  );
}

// A value or declared shape the form has no editor for: its canonical JSON
// in a read-only input, saying where it is edited instead (UI §5, "Kinds
// without an editor"). `what` names it, as in "duration" or "enum day |
// week".
export function NoEditor({
  label,
  json,
  what,
}: {
  label: string;
  json: unknown;
  what: string;
}) {
  const text = JSON.stringify(json);
  return (
    <span className="no-editor">
      <input
        type="text"
        readOnly
        aria-label={`${label} (read-only)`}
        value={text}
        size={Math.min(Math.max(text.length, 8), 48)}
      />{" "}
      <span className="edit-note">
        read-only: the form has no editor for {what} yet; edit it in the raw
        JSON
      </span>
    </span>
  );
}
