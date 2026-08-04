#![forbid(unsafe_op_in_unsafe_fn)]

mod atomic_file;

pub mod audit;
pub mod config;
pub mod crypto;
pub mod dispatcher;
pub mod error;
pub mod host_loop;
pub mod lease;
pub mod paths;
pub mod protocol;
pub mod transaction;
pub mod vault;

pub use error::{FcpError, FcpResult};

pub const WIKIPEDIA_ACCOUNT_GROUP_ID: uuid::Uuid =
    uuid::uuid!("7a144677-3f5c-4a86-a767-16fd3ca315b8");
