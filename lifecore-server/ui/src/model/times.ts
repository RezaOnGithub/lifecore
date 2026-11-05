import type { DateString, OffsetTimestampString } from "../contract.ts";

// What the editors of the time kinds need besides the strings they keep
// verbatim (CLIENT §3): the forms the browser's pickers show, and the
// offset the browser's zone gives a wall-clock time (UI §5, "Editors for
// the value kinds"). The server still parses, judges, and re-spells every
// value on check.

const MINUTE = 60_000;
const DAY = 86_400_000;

// The UTC offset in effect at an instant (milliseconds since the epoch), in
// minutes east of UTC. The browser's comes from `getTimezoneOffset`, which
// counts west; an offset with seconds, as in a zone's local mean time,
// comes out fractional.
export type OffsetAt = (epochMs: number) => number;

export const browserOffsetAt: OffsetAt = (epochMs) =>
  -new Date(epochMs).getTimezoneOffset();

interface Wall {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
}

function pad(value: number, width = 2): string {
  return String(value).padStart(width, "0");
}

// The wall clock as if it were UTC, so its fields can be read and compared
// without a zone. `setUTCFullYear` keeps the years 0001–0099, which
// `Date.UTC` would move to the 1900s.
function wallMs(wall: Wall): number {
  const date = new Date(0);
  date.setUTCFullYear(wall.year, wall.month - 1, wall.day);
  date.setUTCHours(wall.hour, wall.minute, wall.second, 0);
  return date.getTime();
}

function wallAt(ms: number): Wall {
  const date = new Date(ms);
  return {
    year: date.getUTCFullYear(),
    month: date.getUTCMonth() + 1,
    day: date.getUTCDate(),
    hour: date.getUTCHours(),
    minute: date.getUTCMinutes(),
    second: date.getUTCSeconds(),
  };
}

// The years an HTML date or datetime-local input can hold.
function pickable(wall: Wall): boolean {
  return wall.year >= 1 && wall.year <= 9999;
}

// Whether the fields name a real day and time: February 30 or 24:00 come
// back as other fields.
function real(wall: Wall): boolean {
  const back = wallAt(wallMs(wall));
  return (
    back.year === wall.year &&
    back.month === wall.month &&
    back.day === wall.day &&
    back.hour === wall.hour &&
    back.minute === wall.minute &&
    back.second === wall.second
  );
}

function dateText(wall: Wall): string {
  return `${pad(wall.year, 4)}-${pad(wall.month)}-${pad(wall.day)}`;
}

// The normalized value of a datetime-local input, which leaves out zero
// seconds; the input reads back the same text it was given.
function pickerWallText(wall: Wall): string {
  const time = `${pad(wall.hour)}:${pad(wall.minute)}`;
  return `${dateText(wall)}T${time}${wall.second === 0 ? "" : `:${pad(wall.second)}`}`;
}

function wallTimeText(wall: Wall): string {
  return `${dateText(wall)}T${pad(wall.hour)}:${pad(wall.minute)}:${pad(wall.second)}`;
}

// ---- Dates ----

const DATE = /^(\d{4})-(\d{2})-(\d{2})$/;

// The value as a date input holds it, or null when the input cannot show
// it: a year outside 0001–9999 (Jiff's six-digit form for years before
// 0000, or year 0000 itself), a day that does not exist, or text in another
// form. The editor then shows the text instead, so no value is replaced
// by what a picker could hold.
export function pickerDate(value: DateString): string | null {
  const match = DATE.exec(value);
  if (match === null) {
    return null;
  }
  const wall = {
    year: Number(match[1]),
    month: Number(match[2]),
    day: Number(match[3]),
    hour: 0,
    minute: 0,
    second: 0,
  };
  return pickable(wall) && real(wall) ? value : null;
}

// ---- Offset timestamps ----

const OFFSET_TIMESTAMP =
  /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(\.\d{1,9})?([+-]\d{2}:\d{2})$/;

export interface PickerTimestamp {
  // The value's own wall clock, at its own offset, as a datetime-local
  // input holds it: to the second.
  wall: string;
  // What the picker does not show: the fraction of a second, with its
  // full stop, or "", and the offset.
  fraction: string;
  offset: string;
}

// The parts of an offset timestamp a datetime-local picker shows, or null
// when it cannot show the value: a form other than RFC 3339's with a
// numeric offset, a time that does not exist (a leap second included), or
// a year outside 0001–9999.
export function pickerTimestamp(
  value: OffsetTimestampString,
): PickerTimestamp | null {
  const match = OFFSET_TIMESTAMP.exec(value);
  if (match === null) {
    return null;
  }
  const wall = {
    year: Number(match[1]),
    month: Number(match[2]),
    day: Number(match[3]),
    hour: Number(match[4]),
    minute: Number(match[5]),
    second: Number(match[6]),
  };
  if (!pickable(wall) || !real(wall)) {
    return null;
  }
  return {
    wall: pickerWallText(wall),
    fraction: match[7] ?? "",
    offset: match[8] ?? "",
  };
}

// `+01:00`, `-05:30`, and `+00:00` for zero: `-00:00` would say the offset
// is unknown, which the server refuses (FILES §2.1).
export function offsetText(minutesEast: number): string {
  const sign = minutesEast < 0 ? "-" : "+";
  const minutes = Math.abs(minutesEast);
  return `${sign}${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`;
}

// An offset with seconds, for a message: `+00:19:32`.
function offsetWords(minutesEast: number): string {
  const seconds = Math.round(Math.abs(minutesEast) * 60);
  const sign = minutesEast < 0 ? "-" : "+";
  return `${sign}${pad(Math.floor(seconds / 3600))}:${pad(Math.floor(seconds / 60) % 60)}:${pad(seconds % 60)}`;
}

const MAX_OFFSET = 23 * 60 + 59;

function offsetProblem(minutesEast: number): string | null {
  if (!Number.isInteger(minutesEast)) {
    return `this browser's offset at that instant is ${offsetWords(minutesEast)}, not whole minutes, which an offset timestamp needs; type the value as text`;
  }
  if (Math.abs(minutesEast) > MAX_OFFSET) {
    return `this browser's offset at that instant, ${offsetWords(minutesEast)}, is beyond ±23:59; type the value as text`;
  }
  return null;
}

export type Resolved =
  | {
      value: OffsetTimestampString;
      // The wall clock written, as the picker holds it.
      wall: string;
      // The typed wall clock does not exist in the browser's zone, in the
      // gap a change of offset skips: the instant is read with the offset
      // before the gap, as `Date` does, which writes a later wall clock.
      shifted: boolean;
      // The typed wall clock occurs twice, in the overlap a change of
      // offset repeats; the earlier instant is written, as `Date` does.
      ambiguous: boolean;
    }
  | { error: string };

const PICKER_WALL = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/;

// The wall clock a datetime-local picker holds, or null for anything else
// (incomplete, or a day that does not exist).
function pickedWall(pickerWall: string): Wall | null {
  const match = PICKER_WALL.exec(pickerWall);
  if (match === null) {
    return null;
  }
  const wall = {
    year: Number(match[1]),
    month: Number(match[2]),
    day: Number(match[3]),
    hour: Number(match[4]),
    minute: Number(match[5]),
    second: Number(match[6] ?? "0"),
  };
  return real(wall) ? wall : null;
}

// The year of a picker's value, or null when it holds none. A year typed
// digit by digit passes through 0002, 0020, and 0202 on its way to 2026.
export function pickerYear(pickerWall: string): number | null {
  const match = /^(\d{4})-/.exec(pickerWall);
  return match === null ? null : Number(match[1]);
}

// The offset timestamp for a wall clock picked over an existing value,
// read at that value's own offset: the offset observed when the value was
// recorded stays (CORE-ROUND §2.2, "store what was observed"), and the
// instant moves by exactly the change picked. No zone is consulted, so
// this never meets a gap or an overlap. The fraction is kept as well.
export function atOwnOffset(
  pickerWall: string,
  own: PickerTimestamp,
): { value: OffsetTimestampString } | { error: string } {
  const wall = pickedWall(pickerWall);
  if (wall === null) {
    return { error: `not a date and time: ${pickerWall}` };
  }
  return { value: `${wallTimeText(wall)}${own.fraction}${own.offset}` };
}

// The offset timestamp for a wall clock read in the browser's zone, for a
// new value or an explicit "this browser's offset": the offset is the one
// in effect at the instant meant, so a summer date gets the summer offset
// whatever the season now. `fraction` is kept from the value being
// edited, since the picker does not show it. An offset in seconds is refused rather than
// rounded, since rounding would move the instant.
export function offsetTimestampAt(
  pickerWall: string,
  fraction: string,
  offsetAt: OffsetAt = browserOffsetAt,
): Resolved {
  const typed = pickedWall(pickerWall);
  if (typed === null) {
    return { error: `not a date and time: ${pickerWall}` };
  }
  const local = wallMs(typed);
  // The offsets a day either side bound those that could apply: an
  // instant whose own offset puts it at this wall clock is a reading of
  // it. None is a gap; two, an overlap.
  const before = offsetAt(local - DAY);
  const after = offsetAt(local + DAY);
  const readings = [...new Set([before, after])]
    .map((offset) => local - offset * MINUTE)
    .filter((instant) => instant + offsetAt(instant) * MINUTE === local)
    .sort((a, b) => a - b);
  const instant = readings[0] ?? local - before * MINUTE;
  const offset = offsetAt(instant);
  const problem = offsetProblem(offset);
  if (problem !== null) {
    return { error: problem };
  }
  const written = wallAt(instant + offset * MINUTE);
  if (!pickable(written)) {
    return { error: "outside the years 0001–9999 the picker can write" };
  }
  return {
    value: `${wallTimeText(written)}${fraction}${offsetText(offset)}`,
    wall: pickerWallText(written),
    shifted: readings.length === 0,
    ambiguous: readings.length > 1,
  };
}

// The browser's present instant with its offset at that instant, to the
// millisecond `Date` has; no digits are invented beyond it.
export function nowOffsetTimestamp(
  now: number = Date.now(),
  offsetAt: OffsetAt = browserOffsetAt,
): { value: OffsetTimestampString } | { error: string } {
  const offset = offsetAt(now);
  const problem = offsetProblem(offset);
  if (problem !== null) {
    return { error: problem };
  }
  const local = now + offset * MINUTE;
  const millis = new Date(local).getUTCMilliseconds();
  const fraction = millis === 0 ? "" : `.${pad(millis, 3).replace(/0+$/, "")}`;
  return {
    value: `${wallTimeText(wallAt(local))}${fraction}${offsetText(offset)}`,
  };
}

// ---- Durations ----

// Days and longer, in either of Jiff's spellings: ISO 8601's date part
// (`P1D`, `P1W`, `P1Y2M`) or a friendly unit (`1d`, `2 weeks`, `1mo`).
const CALENDAR_UNITS =
  /^[+-]?\s*P[^T]*[YMWD]|\d\s*(?:y|yrs?|years?|mos?|months?|w|wks?|weeks?|d|days?)\b/i;

export const CALENDAR_UNIT_HINT =
  "days and longer are calendar units, not elapsed time; Check will refuse them";

export function durationHint(text: string): string | null {
  if (text === "") {
    return "elapsed time such as PT1H30M or 1h 30m";
  }
  return CALENDAR_UNITS.test(text) ? CALENDAR_UNIT_HINT : null;
}
