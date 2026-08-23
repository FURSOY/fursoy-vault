//! The platform's user-verified signing authority — the thing that proves a human approved a
//! vault operation, rather than some process acting while the machine was unattended.
//!
//! Both backends implement [`crate::crypto::capability::PlatformAuthorizer`], and both hold their
//! private key in the TPM. What differs is how the human is checked, and that difference is why
//! `SignedCapability::proof_context` is backend-defined:
//!
//! * **Windows** asks Windows Hello. The OS owns the dialog, so no secret passes through this
//!   process, and the assertion comes back carrying a user-verified flag that this code checks.
//! * **Linux** has no such service. The key is created with a TPM `authValue` — a PIN — so it
//!   cannot sign at all unless the PIN was supplied. Verification is structural rather than
//!   asserted, and there is no flag to carry.
//!
//! Nothing here decides *whether* an operation is allowed; that is the capability ledger's job.
//! This only answers "did the user approve these exact bytes, just now".

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "linux.rs")]
mod backend;

pub use backend::Authorizer;
