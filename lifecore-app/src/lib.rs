mod error;
pub mod proposal;
pub mod rpc;
pub mod service;
pub mod storage;

pub use error::{AppError, ErrorObject};
pub use service::Service;

#[cfg(test)]
mod test;
