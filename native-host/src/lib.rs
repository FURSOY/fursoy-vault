#![forbid(unsafe_op_in_unsafe_fn)]

mod atomic_file;

#[cfg(test)]
pub(crate) mod test_support;

pub mod audit;
pub mod config;
pub mod crypto;
pub mod dispatcher;
pub mod error;
pub mod host_loop;
pub mod instance_lock;
pub mod lease;
pub mod local_secret;
pub mod monitor;
pub(crate) mod operation;
pub mod paths;
pub mod protocol;
pub mod transaction;
/// Companion self-update. Windows-only by design: there the app owns its own installation and
/// Velopack applies updates in place. On Linux the package manager owns the binary, so the host
/// must never try to replace itself.
#[cfg(windows)]
pub mod update;
pub mod vault;

pub use error::{FcpError, FcpResult};

pub const WIKIPEDIA_ACCOUNT_GROUP_ID: uuid::Uuid =
    uuid::uuid!("7a144677-3f5c-4a86-a767-16fd3ca315b8");
