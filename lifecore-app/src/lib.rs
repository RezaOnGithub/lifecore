mod error;
pub mod proposal;
pub mod query;
pub mod rpc;
pub mod schema;
pub mod service;
pub mod storage;

pub use error::{AppError, ErrorObject};
pub use service::Service;

#[cfg(test)]
mod test;
