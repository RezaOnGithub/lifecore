import assert from "node:assert/strict";
import { test } from "node:test";
import {
  atOwnOffset,
  CALENDAR_UNIT_HINT,
  durationHint,
  nowOffsetTimestamp,
  type OffsetAt,
  offsetText,
  offsetTimestampAt,
  pickerDate,
  pickerTimestamp,
  pickerYear,
} from "./times.ts";

const fixed =
  (minutes: number): OffsetAt =>
  () =>
    minutes;

// America/New_York in 2026: EDT from 2026-03-08T07:00Z, EST again from
// 2026-11-01T06:00Z.
const newYork: OffsetAt = (ms) =>
  ms >= Date.parse("2026-03-08T07:00:00Z") &&
  ms < Date.parse("2026-11-01T06:00:00Z")
    ? -240
    : -300;

test("a date picker shows the dates it can hold, else the text is shown", () => {
  assert.equal(pickerDate("2026-11-06"), "2026-11-06");
  assert.equal(pickerDate("0001-01-01"), "0001-01-01");
  assert.equal(pickerDate("9999-12-31"), "9999-12-31");
  for (const other of [
    "0000-01-01",
    "-000001-12-31",
    "2026-02-30",
    "2026-13-01",
    "2026-1-5",
    "20261106",
    " 2026-11-06",
  ]) {
    assert.equal(pickerDate(other), null, other);
  }
});

test("a timestamp picker shows the value's own wall clock and keeps the rest aside", () => {
  assert.deepEqual(pickerTimestamp("2026-10-29T08:15:00+01:00"), {
    wall: "2026-10-29T08:15",
    fraction: "",
    offset: "+01:00",
  });
  assert.deepEqual(pickerTimestamp("2026-10-29t08:15:07.123456789-05:30"), {
    wall: "2026-10-29T08:15:07",
    fraction: ".123456789",
    offset: "-05:30",
  });
  for (const other of [
    "2026-10-29T08:15:00Z",
    "2026-10-29T08:15+01:00",
    "2026-10-29T08:15:60+01:00",
    "2026-10-29T24:00:00+01:00",
    "0000-01-01T00:00:00+00:00",
    "-000001-12-31T23:00:00-01:00",
    "2026-10-29T08:15:00+01:00[Europe/Berlin]",
  ]) {
    assert.equal(pickerTimestamp(other), null, other);
  }
});

test("offsets are whole minutes, and zero is +00:00", () => {
  assert.equal(offsetText(60), "+01:00");
  assert.equal(offsetText(-330), "-05:30");
  assert.equal(offsetText(0), "+00:00");
  assert.equal(offsetText(-0), "+00:00");
  assert.equal(offsetText(-(23 * 60 + 59)), "-23:59");
});

test("a typed wall clock takes the browser's offset at that instant", () => {
  assert.deepEqual(offsetTimestampAt("2026-10-29T08:15", "", fixed(330)), {
    value: "2026-10-29T08:15:00+05:30",
    wall: "2026-10-29T08:15",
    shifted: false,
    ambiguous: false,
  });
  // The fraction the picker does not show is kept.
  assert.deepEqual(offsetTimestampAt("2026-10-29T08:15:30", ".5", fixed(0)), {
    value: "2026-10-29T08:15:30.5+00:00",
    wall: "2026-10-29T08:15:30",
    shifted: false,
    ambiguous: false,
  });
  // The offset is the one at the instant meant, not the present one.
  const summer = offsetTimestampAt("2026-07-01T09:30", "", newYork);
  assert.ok("value" in summer);
  assert.equal(summer.value, "2026-07-01T09:30:00-04:00");
  const winter = offsetTimestampAt("2026-12-01T09:30", "", newYork);
  assert.ok("value" in winter);
  assert.equal(winter.value, "2026-12-01T09:30:00-05:00");
  // Years below 100 stay where they are.
  const early = offsetTimestampAt("0050-03-01T00:00", "", fixed(60));
  assert.ok("value" in early);
  assert.equal(early.value, "0050-03-01T00:00:00+01:00");
});

test("a wall clock in a gap moves past it; one in an overlap takes the earlier instant", () => {
  // 02:30 does not exist on 2026-03-08 in New York.
  assert.deepEqual(offsetTimestampAt("2026-03-08T02:30", "", newYork), {
    value: "2026-03-08T03:30:00-04:00",
    wall: "2026-03-08T03:30",
    shifted: true,
    ambiguous: false,
  });
  // 01:30 occurs twice on 2026-11-01.
  assert.deepEqual(offsetTimestampAt("2026-11-01T01:30", "", newYork), {
    value: "2026-11-01T01:30:00-04:00",
    wall: "2026-11-01T01:30",
    shifted: false,
    ambiguous: true,
  });
  // Either side of both changes, one reading.
  for (const [wall, value] of [
    ["2026-03-08T01:59:59", "2026-03-08T01:59:59-05:00"],
    ["2026-03-08T03:00", "2026-03-08T03:00:00-04:00"],
    ["2026-11-01T00:59:59", "2026-11-01T00:59:59-04:00"],
    ["2026-11-01T02:00", "2026-11-01T02:00:00-05:00"],
  ] as const) {
    const resolved = offsetTimestampAt(wall, "", newYork);
    assert.ok("value" in resolved, wall);
    assert.equal(resolved.value, value, wall);
    assert.ok(!resolved.shifted && !resolved.ambiguous, wall);
  }
});

test("an offset with seconds or beyond ±23:59 is refused, not rounded", () => {
  // Europe/Berlin's local mean time, +00:53:28.
  const lmt = fixed(53 + 28 / 60);
  const refused = offsetTimestampAt("1850-01-01T12:00", "", lmt);
  assert.ok("error" in refused);
  assert.match(refused.error, /\+00:53:28, not whole minutes/);
  const now = nowOffsetTimestamp(0, lmt);
  assert.ok("error" in now);
  assert.ok("error" in offsetTimestampAt("2026-01-01T00:00", "", fixed(1440)));
  for (const bad of ["", "2026-02-30T00:00", "2026-10-29 08:15", "08:15"]) {
    assert.ok("error" in offsetTimestampAt(bad, "", fixed(0)), bad);
  }
});

test("now is the present instant at the browser's offset, to the millisecond", () => {
  const instant = Date.parse("2026-11-05T16:29:14.120Z");
  assert.deepEqual(nowOffsetTimestamp(instant, fixed(60)), {
    value: "2026-11-05T17:29:14.12+01:00",
  });
  assert.deepEqual(
    nowOffsetTimestamp(Date.parse("2026-07-01T12:00:00Z"), newYork),
    { value: "2026-07-01T08:00:00-04:00" },
  );
  assert.deepEqual(nowOffsetTimestamp(instant - 120, fixed(0)), {
    value: "2026-11-05T16:29:14+00:00",
  });
});

test("the duration hint names calendar units in either spelling", () => {
  for (const ok of ["PT1H30M", "-PT1H", "PT0.5S", "1h 30m", "90m", "5ms"]) {
    assert.equal(durationHint(ok), null, ok);
  }
  for (const civil of ["P1D", "P1W", "P1Y2M", "1d", "2 weeks", "1mo"]) {
    assert.equal(durationHint(civil), CALENDAR_UNIT_HINT, civil);
  }
  assert.match(durationHint("") ?? "", /PT1H30M/);
});

// An existing value's pick is read at its own offset, whatever the
// browser's zone: these are the cases where reading it in the browser's
// zone moved the instant and dropped the observed offset.
function repick(value: string, wall: string): string {
  const own = pickerTimestamp(value);
  assert.ok(own !== null, value);
  const kept = atOwnOffset(wall, own);
  assert.ok("value" in kept, wall);
  return kept.value;
}

test("a pick over an existing value keeps the offset it observed", () => {
  // In Kolkata (+05:30), a minute's correction to a +01:00 value.
  assert.equal(
    repick("2026-10-29T08:15:00+01:00", "2026-10-29T08:16"),
    "2026-10-29T08:16:00+01:00",
  );
  // In Berlin, a minute's correction to a -08:00 value late in the day.
  assert.equal(
    repick("2026-10-29T23:30:00-08:00", "2026-10-29T23:31"),
    "2026-10-29T23:31:00-08:00",
  );
  // In New York, the second reading of the repeated 01:30 stays the second.
  assert.equal(
    repick("2026-11-01T01:30:00-05:00", "2026-11-01T01:30:05"),
    "2026-11-01T01:30:05-05:00",
  );
  // A wall clock New York skips is no gap at the value's own offset.
  assert.equal(
    repick("2026-03-08T01:30:00-05:00", "2026-03-08T02:30"),
    "2026-03-08T02:30:00-05:00",
  );
  // The fraction and a zero offset stay as they were.
  assert.equal(
    repick("2026-10-29t08:15:00.123456789+00:00", "2026-10-30T08:15"),
    "2026-10-30T08:15:00.123456789+00:00",
  );
  const own = pickerTimestamp("2026-10-29T08:15:00+01:00");
  assert.ok(own !== null);
  for (const bad of ["", "2026-02-30T00:00", "2026-10-29"]) {
    assert.ok("error" in atOwnOffset(bad, own), bad);
  }
});

test("a picker's year is read so a year being typed can wait", () => {
  assert.equal(pickerYear("0020-03-01"), 20);
  assert.equal(pickerYear("2026-03-01T09:30"), 2026);
  assert.equal(pickerYear(""), null);
});
