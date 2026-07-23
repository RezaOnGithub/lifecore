//! Typed node-schema vocabulary.
//!
//! This module owns the value-language used by stored type declarations. It
//! deliberately describes payload shape only; semantic policy remains an
//! analyzer concern.

mod boilerplate;
mod check;
mod model;
mod parse;

pub use model::{FieldDecl, FieldStatus, Type, ValueType};
