use std::{fmt, str::FromStr};

use lifecore::LifeCoreError;

use crate::{FieldDecl, FieldStatus, ValueType};

impl FromStr for ValueType {
    type Err = LifeCoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // VALUE_TYPE := PRIMITIVE | TEXT_LITERAL | NUMBER_LITERAL
        //             | ('Sequence' | 'OrderedSet') '(' VALUE_TYPE ')'
        //             | ('Tuple' | 'OneOf') '(' (VALUE_TYPE (',' VALUE_TYPE)*)? ')'

        fn error() -> LifeCoreError {
            LifeCoreError::Bap
        }

        fn skip_whitespace(s: &str, cursor: &mut usize) {
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_whitespace()
            {
                *cursor += c.len_utf8();
            }
        }

        fn consume(s: &str, cursor: &mut usize, expected: char) -> bool {
            skip_whitespace(s, cursor);
            if s[*cursor..].starts_with(expected) {
                *cursor += expected.len_utf8();
                true
            } else {
                false
            }
        }

        fn expect(s: &str, cursor: &mut usize, expected: char) -> Result<(), LifeCoreError> {
            if consume(s, cursor, expected) {
                Ok(())
            } else {
                Err(error())
            }
        }

        fn keyword<'a>(s: &'a str, cursor: &mut usize) -> Result<&'a str, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_ascii_alphabetic()
            {
                *cursor += c.len_utf8();
            }
            if start == *cursor {
                Err(error())
            } else {
                Ok(&s[start..*cursor])
            }
        }

        fn type_list(s: &str, cursor: &mut usize) -> Result<Vec<ValueType>, LifeCoreError> {
            let mut values = vec![];
            if consume(s, cursor, ')') {
                return Ok(values);
            }
            loop {
                values.push(value_type(s, cursor)?);
                if consume(s, cursor, ')') {
                    return Ok(values);
                }
                expect(s, cursor, ',')?;
            }
        }

        fn text_literal(s: &str, cursor: &mut usize) -> Result<ValueType, LifeCoreError> {
            skip_whitespace(s, cursor);
            expect(s, cursor, '"')?;
            let start = *cursor;
            while let Some(c) = s[*cursor..].chars().next() {
                if c == '"' {
                    let value = s[start..*cursor].to_string();
                    *cursor += c.len_utf8();
                    return Ok(ValueType::TextLiteral(value));
                }
                *cursor += c.len_utf8();
            }
            Err(error())
        }

        fn number_literal(s: &str, cursor: &mut usize) -> Result<ValueType, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            if s[*cursor..].starts_with('-') {
                *cursor += '-'.len_utf8();
            }
            let digits_start = *cursor;
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_ascii_digit()
            {
                *cursor += c.len_utf8();
            }
            if digits_start == *cursor {
                return Err(error());
            }
            s[start..*cursor]
                .parse::<i64>()
                .map(ValueType::NumberLiteral)
                .map_err(|_| error())
        }

        fn value_type(s: &str, cursor: &mut usize) -> Result<ValueType, LifeCoreError> {
            use ValueType::*;
            skip_whitespace(s, cursor);
            match s[*cursor..].chars().next() {
            Some('"') => return text_literal(s, cursor),
            Some('-' | '0'..='9') => return number_literal(s, cursor),
            _ => {}
            }
            match keyword(s, cursor)? {
            "PreciseTime" | "precisetime" => Ok(PreciseTime),
            "Text" | "text" => Ok(Text),
            "Number" | "number" => Ok(Number),
            "Sequence" | "sequence" => {
                expect(s, cursor, '(')?;
                let inner = value_type(s, cursor)?;
                expect(s, cursor, ')')?;
                Ok(Sequence(Box::new(inner)))
            }
            "OrderedSet" | "orderedset" => {
                expect(s, cursor, '(')?;
                let inner = value_type(s, cursor)?;
                expect(s, cursor, ')')?;
                Ok(OrderedSet(Box::new(inner)))
            }
            "Tuple" | "tuple" => {
                expect(s, cursor, '(')?;
                Ok(Tuple(type_list(s, cursor)?))
            }
            "OneOf" | "oneof" => {
                expect(s, cursor, '(')?;
                Ok(OneOf(type_list(s, cursor)?))
            }
            _ => Err(error()),
            }
        }

        let mut cursor = 0;
        let parsed = value_type(s, &mut cursor)?;
        skip_whitespace(s, &mut cursor);
        if cursor == s.len() {
            Ok(parsed)
        } else {
            Err(error())
        }
    }
}

impl FromStr for FieldStatus {
    type Err = LifeCoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
        "Mandatory" | "mandatory" => Ok(Self::Mandatory),
        "Encouraged" | "encouraged" => Ok(Self::Encouraged),
        "Deprecated" | "deprecated" => Ok(Self::Deprecated),
        "Optional" | "optional" => Ok(Self::Optional),
        _ => Err(LifeCoreError::Bap),
        }
    }
}

impl fmt::Display for FieldStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
        Self::Mandatory => "Mandatory",
        Self::Encouraged => "Encouraged",
        Self::Deprecated => "Deprecated",
        Self::Optional => "Optional",
        })
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn list(f: &mut fmt::Formatter<'_>, values: &[ValueType]) -> fmt::Result {
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{value}")?;
            }
            Ok(())
        }

        match self {
        ValueType::PreciseTime => f.write_str("PreciseTime"),
        ValueType::Text => f.write_str("Text"),
        ValueType::Number => f.write_str("Number"),
        ValueType::TextLiteral(s) => write!(f, "\"{}\"", s.as_str()),
        ValueType::NumberLiteral(i) => write!(f, "{}", i),
        ValueType::Sequence(inner) => write!(f, "Sequence({inner})"),
        ValueType::Tuple(values) => {
            f.write_str("Tuple(")?;
            list(f, values)?;
            f.write_str(")")
        }
        ValueType::OrderedSet(inner) => write!(f, "OrderedSet({inner})"),
        ValueType::OneOf(values) => {
            f.write_str("OneOf(")?;
            list(f, values)?;
            f.write_str(")")
        }
        }
    }
}

impl FromStr for FieldDecl {
    type Err = LifeCoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // FIELD_DECL := FIELD_NAME '=' VALUE_TYPE ':' FIELD_STATUS
        fn error() -> LifeCoreError {
            LifeCoreError::Bap
        }

        fn skip_whitespace(s: &str, cursor: &mut usize) {
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_whitespace()
            {
                *cursor += c.len_utf8();
            }
        }

        fn consume(s: &str, cursor: &mut usize, expected: char) -> bool {
            skip_whitespace(s, cursor);
            if s[*cursor..].starts_with(expected) {
                *cursor += expected.len_utf8();
                true
            } else {
                false
            }
        }

        fn expect(s: &str, cursor: &mut usize, expected: char) -> Result<(), LifeCoreError> {
            if consume(s, cursor, expected) {
                Ok(())
            } else {
                Err(error())
            }
        }

        fn field_name(s: &str, cursor: &mut usize) -> Result<String, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            let Some(first) = s[*cursor..].chars().next() else {
                return Err(error());
            };
            if !first.is_ascii_lowercase() {
                return Err(error());
            }
            *cursor += first.len_utf8();
            while let Some(c) = s[*cursor..].chars().next()
                && (c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                *cursor += c.len_utf8();
            }
            Ok(s[start..*cursor].to_string())
        }

        fn value_type<'a>(s: &'a str, cursor: &mut usize) -> Result<&'a str, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            let mut depth = 0usize;
            let mut in_text_literal = false;
            while let Some(c) = s[*cursor..].chars().next() {
                if c == '"' {
                    in_text_literal = !in_text_literal;
                    *cursor += c.len_utf8();
                    continue;
                }
                if in_text_literal {
                    *cursor += c.len_utf8();
                    continue;
                }
                match c {
                '(' => depth += 1,
                ')' => {
                    depth = depth.checked_sub(1).ok_or_else(error)?;
                }
                ':' if depth == 0 => break,
                _ => {}
                }
                *cursor += c.len_utf8();
            }
            if start == *cursor || depth != 0 || in_text_literal {
                Err(error())
            } else {
                Ok(s[start..*cursor].trim())
            }
        }

        fn status(s: &str, cursor: &mut usize) -> Result<FieldStatus, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_ascii_alphabetic()
            {
                *cursor += c.len_utf8();
            }
            s[start..*cursor].parse().map_err(|_| error())
        }

        let mut cursor = 0;
        let name = field_name(s, &mut cursor)?;
        expect(s, &mut cursor, '=')?;
        let ty = value_type(s, &mut cursor)?
            .parse::<ValueType>()
            .map_err(|_| error())?;
        expect(s, &mut cursor, ':')?;
        let status = status(s, &mut cursor)?;
        skip_whitespace(s, &mut cursor);
        if cursor != s.len() {
            return Err(error());
        }
        Ok(Self { name, ty, status })
    }
}

impl fmt::Display for FieldDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} = {} : {}", self.name, self.ty, self.status)
    }
}
