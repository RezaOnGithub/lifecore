use serde::{Deserialize, Deserializer, Serializer, de::Error};
use std::{borrow::Cow, fmt::Display, str::FromStr};

//A Large control integers travel as decimal strings so JavaScript clients never
//A round them. Use with `#[serde(with = "lifecore::decimal")]`.
pub fn serialize<T: Display, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

pub fn deserialize<'de, T: FromStr, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error> {
    let text = Cow::<str>::deserialize(deserializer)?;
    parse(&text).map_err(D::Error::custom)
}

//A Accept only the canonical display spelling: `0`, or an optional minus sign
//A and ASCII digits without a leading zero. Positions are identities, so
//A `042` and `-0` must not name the same entry as `42` and `0`. `FromStr` alone
//A would accept all of these and a leading plus sign.
pub fn parse<T: FromStr>(text: &str) -> Result<T, String> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && text != "-0";
    if !canonical {
        return Err(format!(
            "{text:?} is not a canonical decimal integer string"
        ));
    }
    text.parse()
        .map_err(|_| format!("{text:?} is outside the permitted integer range"))
}
