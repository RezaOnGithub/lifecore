import { type FocusEvent, useState } from "react";
import type {
  DateString,
  DurationString,
  OffsetTimestampString,
  Payload,
} from "../contract.ts";
import {
  durationHint,
  nowOffsetTimestamp,
  offsetTimestampAt,
  pickerDate,
  pickerTimestamp,
} from "../model/times.ts";
import { offsetTimestampHint, timeValue } from "../model/values.ts";
import { liveDraft, type TextDraft } from "./helpers.ts";
import { keepFocus, PickerInput, ValidatedInput } from "./inputs.tsx";

// Editors for the value kinds of the core round (UI §5, "Editors for the
// value kinds"). Each keeps the string it is given verbatim, as typed or as
// the server spelled it, and replaces it only with what the user typed or
// picked; the server parses, judges, and re-spells it on check (CLIENT §3).
// `value` is null for a field made present that waits for its first
// value, and `onAbandon` runs when focus leaves the editor before one
// arrives.

function nonEmpty(word: string): (text: string) => string | null {
  return (text) => (text === "" ? `a ${word} cannot be empty` : null);
}

// Focus leaving the editor as a whole, not moving between its inputs.
function onLeave(
  pending: boolean,
  onAbandon: (() => void) | undefined,
): ((event: FocusEvent<HTMLElement>) => void) | undefined {
  if (!pending || onAbandon === undefined) {
    return undefined;
  }
  return (event) => {
    const next = event.relatedTarget;
    if (!(next instanceof Node && event.currentTarget.contains(next))) {
      onAbandon();
    }
  };
}

export function DurationInput({
  label,
  value,
  onChange,
  onAbandon,
}: {
  label: string;
  value: DurationString | null;
  onChange: (value: Payload) => void;
  onAbandon?: () => void;
}) {
  return (
    <ValidatedInput
      label={label}
      value={value}
      validate={nonEmpty("duration")}
      advise={durationHint}
      title="elapsed time in ISO 8601 (PT1H30M) or Jiff's friendly form (1h 30m); the server stores the ISO spelling"
      size={18}
      onCommit={(text) => onChange(timeValue("duration", text))}
      {...(onAbandon === undefined ? {} : { onAbandon })}
    />
  );
}

// A date picker, or, for a date it cannot hold, the text itself. Once
// shown as text the editor stays text, so fixing the text to a pickable
// date does not swap the input from under the cursor.
export function DateInput({
  label,
  value,
  onChange,
  onAbandon,
}: {
  label: string;
  value: DateString | null;
  onChange: (value: Payload) => void;
  onAbandon?: () => void;
}) {
  const picked = value === null ? null : pickerDate(value);
  const [asText, setAsText] = useState(false);
  const commit = (text: string) => onChange(timeValue("date", text));
  if (asText || (value !== null && picked === null)) {
    if (!asText) {
      setAsText(true);
    }
    return (
      <>
        <ValidatedInput
          label={label}
          value={value}
          validate={nonEmpty("date")}
          size={14}
          onCommit={commit}
          {...(onAbandon === undefined ? {} : { onAbandon })}
        />
        <span className="edit-note">
          as text: a date picker holds only the years 0001 to 9999 in YYYY-MM-DD
        </span>
      </>
    );
  }
  return (
    <fieldset
      className="kind-input"
      onBlur={onLeave(value === null, onAbandon)}
    >
      <PickerInput label={label} type="date" value={picked} onCommit={commit} />
    </fieldset>
  );
}

// A datetime-local picker whose wall clock is read in the browser's zone,
// the offset filled from the browser at that instant, beside the exact
// text: the picker shows a value's own wall clock to the second, while the
// text keeps its fraction and offset and takes any other spelling.
export function OffsetTimestampInput({
  label,
  value,
  onChange,
  onAbandon,
}: {
  label: string;
  value: OffsetTimestampString | null;
  onChange: (value: Payload) => void;
  onAbandon?: () => void;
}) {
  const [note, setNote] =
    useState<TextDraft<OffsetTimestampString | null> | null>(null);
  const [failure, setFailure] =
    useState<TextDraft<OffsetTimestampString | null> | null>(null);
  const parts = value === null ? null : pickerTimestamp(value);
  const commit = (text: OffsetTimestampString) => {
    setFailure(null);
    onChange(timeValue("offset_timestamp", text));
  };
  // A refusal stays until the value changes; a note, while the value is
  // the one it explains.
  const refused = liveDraft(failure, value)?.text ?? null;
  const noted = liveDraft(note, value)?.text ?? null;
  return (
    <fieldset
      className="kind-input"
      onBlur={onLeave(value === null, onAbandon)}
    >
      <PickerInput
        label={label}
        type="datetime-local"
        step={1}
        value={parts?.wall ?? null}
        onCommit={(wall) => {
          const resolved = offsetTimestampAt(wall, parts?.fraction ?? "");
          if ("error" in resolved) {
            setFailure({ text: resolved.error, base: value });
            return;
          }
          const [day, time] = wall.split("T");
          const [, writtenTime] = resolved.wall.split("T");
          const offset = resolved.value.slice(-6);
          setNote(
            resolved.shifted
              ? {
                  text: `${time} does not occur on ${day} in this browser's zone; written as ${writtenTime} at ${offset}`,
                  base: resolved.value,
                }
              : resolved.ambiguous
                ? {
                    text: `${time} occurs twice on ${day} in this browser's zone; written as the earlier, at ${offset}`,
                    base: resolved.value,
                  }
                : null,
          );
          commit(resolved.value);
        }}
      />{" "}
      <ValidatedInput
        label={`${label} as text`}
        value={value}
        validate={nonEmpty("offset timestamp")}
        advise={(text) => (text === "" ? null : offsetTimestampHint(text))}
        title="RFC 3339 with a numeric offset, such as 2026-10-29T08:15:00+01:00"
        size={32}
        onCommit={commit}
      />{" "}
      <button
        type="button"
        aria-label={`now for ${label}`}
        title="the present instant at this browser's offset"
        onMouseDown={keepFocus}
        onClick={() => {
          const now = nowOffsetTimestamp();
          if ("error" in now) {
            setFailure({ text: now.error, base: value });
          } else {
            setNote(null);
            commit(now.value);
          }
        }}
      >
        now
      </button>
      {value !== null && parts === null && (
        <span className="edit-note">
          the picker cannot show this value; its text edits it
        </span>
      )}
      {refused !== null ? (
        <span className="field-error">{refused}</span>
      ) : (
        noted !== null && <span className="edit-note">{noted}</span>
      )}
    </fieldset>
  );
}

// The declared members, and the value itself when it is not one of them,
// so a value the declaration has dropped is shown rather than replaced.
export function EnumSelect({
  label,
  members,
  value,
  onChange,
  onAbandon,
}: {
  label: string;
  members: readonly string[];
  value: string | null;
  onChange: (value: Payload) => void;
  onAbandon?: () => void;
}) {
  return (
    <select
      aria-label={label}
      value={value ?? ""}
      onChange={(event) => onChange(event.target.value)}
      onBlur={onLeave(value === null, onAbandon)}
    >
      {value === null && (
        <option value="" disabled>
          choose…
        </option>
      )}
      {value !== null && !members.includes(value) && (
        <option value={value}>{value} (not a member)</option>
      )}
      {members.map((member) => (
        <option key={member} value={member}>
          {member}
        </option>
      ))}
    </select>
  );
}
