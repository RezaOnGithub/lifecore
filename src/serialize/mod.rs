//! Explicit typed-body serialization for LifeCore.
//!
//! The traits here convert values to stable storage forms without resolving a
//! node's historical schema. Callers that need schema policy should do so in a
//! context-aware analyzer.

mod error;
mod field;
mod life_core;
mod type_body;

pub use error::Error;
pub use field::NodeField;
pub use life_core::LifeCoreSerializeExt;
pub use type_body::NodeBody;
