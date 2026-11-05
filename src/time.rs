use jiff::{
    SignedDuration, Timestamp,
    civil::Date,
    fmt::temporal::{Pieces, PiecesOffset},
    tz::Offset,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{borrow::Cow, fmt, str::FromStr};

// An instant with the UTC offset in effect where it was observed (SPEC 6,
// FILES 2.1). The local date and time follow from the two without any zone
// lookup, so nothing here names or reads a time zone.
//
// Equality and hashing are field by field: the same instant at two offsets
// is two values. The offset is whole minutes, because RFC 3339, the only
// spelling, has no syntax for seconds; `new` refuses any other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OffsetTimestamp {
    instant: Timestamp,
    offset: Offset,
}

impl OffsetTimestamp {
    pub fn new(instant: Timestamp, offset: Offset) -> Result<Self, String> {
        if offset.seconds() % 60 != 0 {
            return Err(format!(
                "the offset {offset} has seconds, which RFC 3339 cannot write"
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
    // RFC 3339 with a numeric offset. `Z` and `-00:00` mean "UTC known, local
    // offset unknown" (RFC 9557), which is what this kind exists to avoid, so
    // both are refused; `+00:00` is a known zero offset. Any bracketed
    // annotation is refused rather than dropped, including one Jiff would
    // ignore, so no input is silently narrowed. Otherwise Jiff's Temporal
    // parser decides, with its normalization: canonical output re-spells.
    pub fn parse(text: &str) -> Result<Self, String> {
        let refuse = |why: &str| Err(format!("{text:?} is not an offset timestamp: {why}"));
        if text.contains('[') {
            return refuse("a bracketed annotation such as a time zone is not accepted");
        }
        let pieces = match Pieces::parse(text) {
        Ok(pieces) => pieces,
        Err(error) => return refuse(&error.to_string()),
        };
        let Some(time) = pieces.time() else {
            return refuse("it has no time of day");
        };
        let offset = match pieces.offset() {
        Some(PiecesOffset::Numeric(numeric)) => {
            if numeric.is_negative() && numeric.offset().is_zero() {
                return refuse(
                    "-00:00 leaves the local offset unknown; write +00:00 for a known zero offset",
                );
            }
            numeric.offset()
        }
        Some(PiecesOffset::Zulu) => {
            return refuse(
                "Z leaves the local offset unknown; write a numeric offset, +00:00 for UTC itself",
            );
        }
        _ => return refuse("it needs a numeric offset such as +01:00"),
        };
        let instant = match offset.to_timestamp(pieces.date().to_datetime(time)) {
        Ok(instant) => instant,
        Err(error) => return refuse(&error.to_string()),
        };
        Self::new(instant, offset).or_else(|why| refuse(&why))
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

// `YYYY-MM-DD` only. Jiff would also read a basic `20261106`, a six-digit
// year, or a datetime whose time it drops.
pub(crate) fn parse_date(text: &str) -> Result<Date, String> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 => *b == b'-',
        _ => b.is_ascii_digit(),
        });
    if !shaped {
        return Err(format!("{text:?} is not a date: write YYYY-MM-DD"));
    }
    text.parse()
        .map_err(|error| format!("{text:?} is not a date: {error}"))
}
