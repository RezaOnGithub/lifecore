use jiff::{
    SignedDuration, Timestamp,
    civil::{Date, Time},
    tz::Offset,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{borrow::Cow, fmt, str::FromStr};

// An instant with the UTC offset in effect where it was observed (SPEC 6.4,
// FILES 2.1). The local date and time follow from the two without any zone
// lookup, so nothing here names or reads a time zone.
//
// Equality and hashing are field by field: the same instant at two offsets
// is two values. The offset is what RFC 3339, the only spelling, can write:
// whole minutes up to ±23:59. `new` refuses any other, so every value prints
// as text that `parse` reads back to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OffsetTimestamp {
    instant: Timestamp,
    offset: Offset,
}

const MAX_OFFSET_SECONDS: i32 = 23 * 3600 + 59 * 60;

impl OffsetTimestamp {
    pub fn new(instant: Timestamp, offset: Offset) -> Result<Self, String> {
        if offset.seconds() % 60 != 0 {
            return Err(format!(
                "the offset {offset} has seconds, which RFC 3339 cannot write"
            ));
        }
        if offset.seconds().abs() > MAX_OFFSET_SECONDS {
            return Err(format!(
                "the offset {offset} is beyond ±23:59, which RFC 3339 cannot write"
            ));
        }
        Ok(Self { instant, offset })
    }
    pub fn instant(self) -> Timestamp {
        self.instant
    }
    pub fn offset(self) -> Offset {
        self.offset
    }
    // RFC 3339's `date-time` with a numeric offset, read from the text itself
    // so nothing is rounded, clamped, or dropped on the way:
    // `YYYY-MM-DDTHH:MM:SS[.fraction]±HH:MM`, `T` or `t`, seconds required, a
    // fraction of one to nine digits after a full stop. The date follows
    // `parse_date`, so a year outside 0000-9999 takes Jiff's signed six-digit
    // form and nothing else does. Refused: `Z` and `-00:00`, which RFC 9557
    // reads as "UTC known, local offset unknown", what this kind exists to
    // avoid (`+00:00` is a known zero offset); an offset with seconds or a
    // fraction; a bracketed annotation, rather than dropped; a leap second,
    // rather than clamped to :59; and every other spelling Jiff's Temporal
    // parser would take, such as the basic format or an hour-only time.
    pub fn parse(text: &str) -> Result<Self, String> {
        let refuse = |why: &str| format!("{text:?} is not an offset timestamp: {why}");
        let Some(split) = text.find(['T', 't']) else {
            return Err(refuse(
                "write YYYY-MM-DDTHH:MM:SS with a numeric offset such as +01:00",
            ));
        };
        let date = date_part(&text[..split]).map_err(|why| refuse(&why))?;
        let rest = &text.as_bytes()[split + 1..];
        let shaped = |at: usize, len: usize| {
            rest.get(at..at + len)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
        };
        let number = |at: usize, len: usize| {
            std::str::from_utf8(&rest[at..at + len])
                .expect("ASCII digits")
                .parse::<i32>()
                .expect("ASCII digits")
        };
        if !(shaped(0, 2)
            && rest.get(2) == Some(&b':')
            && shaped(3, 2)
            && rest.get(5) == Some(&b':')
            && shaped(6, 2))
        {
            return Err(refuse("the time is HH:MM:SS, seconds included"));
        }
        let (hour, minute, second) = (number(0, 2), number(3, 2), number(6, 2));
        if second == 60 {
            return Err(refuse(
                "a leap second cannot be represented, and is not clamped to :59",
            ));
        }
        let mut at = 8;
        let mut nanos = 0;
        if rest.get(at) == Some(&b'.') {
            let digits = rest[at + 1..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits == 0 || digits > 9 {
                return Err(refuse("a fraction of a second has one to nine digits"));
            }
            nanos = number(at + 1, digits) * 10i32.pow(9 - digits as u32);
            at += 1 + digits;
        }
        let offset = match rest.get(at) {
        Some(b'Z' | b'z') => {
            return Err(refuse(
                "Z leaves the local offset unknown; write a numeric offset, +00:00 for UTC itself",
            ));
        }
        Some(sign @ (b'+' | b'-')) => {
            let negative = *sign == b'-';
            if !(shaped(at + 1, 2) && rest.get(at + 3) == Some(&b':') && shaped(at + 4, 2)) {
                return Err(refuse("the offset is ±HH:MM"));
            }
            let (hours, minutes) = (number(at + 1, 2), number(at + 4, 2));
            match rest.get(at + 6) {
            None => {}
            Some(b'[') => {
                return Err(refuse(
                    "a bracketed annotation such as a time zone is not accepted",
                ));
            }
            Some(b':' | b'.') => {
                return Err(refuse(
                    "an offset with seconds or a fraction cannot be written in RFC 3339",
                ));
            }
            Some(_) => return Err(refuse("unexpected text after the offset")),
            }
            if negative && hours == 0 && minutes == 0 {
                return Err(refuse(
                    "-00:00 leaves the local offset unknown; write +00:00 for a known zero offset",
                ));
            }
            if hours > 23 || minutes > 59 {
                return Err(refuse("the offset is at most ±23:59"));
            }
            let seconds = (hours * 60 + minutes) * 60;
            Offset::from_seconds(if negative { -seconds } else { seconds }).expect("within ±23:59")
        }
        _ => return Err(refuse("it needs a numeric offset such as +01:00")),
        };
        let time = Time::new(hour as i8, minute as i8, second as i8, nanos)
            .map_err(|error| refuse(&error.to_string()))?;
        let instant = offset
            .to_timestamp(date.to_datetime(time))
            .map_err(|error| refuse(&error.to_string()))?;
        Self::new(instant, offset).map_err(|why| refuse(&why))
    }
}

impl fmt::Display for OffsetTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.instant.display_with_offset(self.offset))
    }
}

impl FromStr for OffsetTimestamp {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        Self::parse(text)
    }
}

// The same string inside a payload envelope and as a known field such as
// `EvaluationInputs.evaluation_time`.
impl Serialize for OffsetTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for OffsetTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<str>::deserialize(deserializer)?;
        Self::parse(&text).map_err(de::Error::custom)
    }
}

// Jiff's parser, ISO 8601 (`PT90M`) or its friendly form (`1h 30m`). Jiff
// refuses days and larger units for a `SignedDuration`, as a day is not 24
// hours across a DST change; that refusal is the exact/civil split.
pub(crate) fn parse_duration(text: &str) -> Result<SignedDuration, String> {
    text.parse()
        .map_err(|error| format!("{text:?} is not a duration: {error}"))
}

pub(crate) fn parse_date(text: &str) -> Result<Date, String> {
    date_part(text).map_err(|why| format!("{text:?} is not a date: {why}"))
}

// RFC 3339's `full-date`, `YYYY-MM-DD`, for years 0000-9999. A year before
// 0000, which four digits cannot write, takes the form Jiff prints for it, a
// minus sign and six digits (`-000001-01-01`), so every date has exactly one
// accepted spelling, its canonical one. Refused: that form for any other
// year, a plus sign, the basic `20261106`, and a datetime, though Jiff would
// read them all.
fn date_part(text: &str) -> Result<Date, String> {
    let bytes = text.as_bytes();
    let (negative, digits) = match bytes.first() {
    Some(b'-') => (true, 6),
    _ => (false, 4),
    };
    let start = usize::from(negative);
    let year_end = start + digits;
    let shaped = bytes.len() == year_end + 6
        && bytes[start..year_end].iter().all(u8::is_ascii_digit)
        && bytes[year_end] == b'-'
        && bytes[year_end + 1..year_end + 3]
            .iter()
            .all(u8::is_ascii_digit)
        && bytes[year_end + 3] == b'-'
        && bytes[year_end + 4..].iter().all(u8::is_ascii_digit);
    if !shaped {
        return Err("write YYYY-MM-DD".into());
    }
    let field = |range: std::ops::Range<usize>| text[range].parse::<i32>().expect("ASCII digits");
    let year = field(start..year_end);
    if negative && year == 0 {
        return Err("year 0000 is written with four digits".into());
    }
    let year = if negative { -year } else { year };
    let (month, day) = (
        field(year_end + 1..year_end + 3),
        field(year_end + 4..text.len()),
    );
    let year = i16::try_from(year).map_err(|_| "the year is out of range".to_string())?;
    Date::new(year, month as i8, day as i8).map_err(|error| error.to_string())
}
