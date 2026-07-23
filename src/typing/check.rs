use crate::{Payload, serialize::Error};

use super::ValueType;

impl ValueType {
    /// Checks whether a payload has this declared shape.
    pub fn check(&self, payload: &Payload) -> Result<(), Error> {
        match (self, payload) {
        (Self::PreciseTime, Payload::PreciseTime(_))
        | (Self::Text, Payload::Text(_))
        | (Self::Number, Payload::Number(_)) => Ok(()),
        (Self::TextLiteral(expected), Payload::Text(actual)) if expected == actual => Ok(()),
        (Self::NumberLiteral(expected), Payload::Number(actual)) if expected == actual => Ok(()),
        (Self::Sequence(expected), Payload::Sequence(actual)) => {
            actual.iter().enumerate().try_for_each(|(index, value)| {
                expected
                    .check(value)
                    .map_err(|source| Error::invalid_element(index, source))
            })
        }
        (Self::Tuple(expected), Payload::Sequence(actual)) => {
            if expected.len() != actual.len() {
                return Err(Error::InvalidTupleArity {
                    expected: expected.len(),
                    actual: actual.len(),
                });
            }
            expected
                .iter()
                .zip(actual)
                .enumerate()
                .try_for_each(|(index, (expected, actual))| {
                    expected
                        .check(actual)
                        .map_err(|source| Error::invalid_element(index, source))
                })
        }
        (Self::OrderedSet(expected), Payload::OrderedSet(actual)) => {
            actual.iter().enumerate().try_for_each(|(index, value)| {
                expected
                    .check(value)
                    .map_err(|source| Error::invalid_element(index, source))
            })
        }
        (Self::OneOf(expected), actual)
            if expected
                .iter()
                .any(|expected| expected.check(actual).is_ok()) =>
        {
            Ok(())
        }
        _ => Err(Error::unexpected_payload(self.clone(), payload)),
        }
    }
}
