//! The key-encryption key: a hardware-held asymmetric key that wraps each vault's DEK, so the DEK
//! never rests on disk in the clear and cannot be recovered by copying the vault file alone.
//!
//! Both platforms use RSA-2048 with OAEP-SHA256 and hold the private key in the TPM, which is what
//! keeps the wrapped form exactly [`WRAPPED_DEK_BYTES`] on either one — the vault record format
//! requires that length, so a backend producing a different size would be a format break rather
//! than an implementation detail.
//!
//! Where they differ is custody. Windows lets NCrypt's Platform Crypto Provider own the key under
//! a name and hand back a handle; there is no key material for this code to store. Linux has no
//! such OS-managed store, so the TPM-wrapped key blobs live in a file, per profile — see
//! [`crate::paths::DataPaths::kek_key`].

use std::path::{Path, PathBuf};

use crate::FcpResult;
use crate::crypto::aead::SecretDek;

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "linux.rs")]
mod backend;

// Renamed pre-launch (2026-08-08, ADR-023) while it is still free to do so: KEK_KEY_ID is stored
// alongside every wrapped DEK to identify which key wrapped it. Changing it orphans any existing
// TPM-backed key and permanently undecrypts every already-vaulted session — deliberately accepted
// then because no real user data existed yet; do not rename after launch without a migration plan.
pub const KEK_KEY_ID: [u8; 16] = *b"VAULT-KEK-v1\0\0\0\0";
pub const RSA_BITS: u32 = 2048;
pub const WRAPPED_DEK_BYTES: usize = 256;

pub struct PlatformKek {
    /// Where the backend keeps its key material, when the platform makes that this code's problem.
    /// Unused on Windows, where the provider owns the key under [`backend::KEK_NAME`].
    #[cfg_attr(windows, allow(dead_code))]
    key_path: PathBuf,
}

impl PlatformKek {
    /// Names the profile's KEK without touching the TPM. Construction is deliberately inert:
    /// every host start builds one, and making that a hardware round-trip would let a momentarily
    /// busy TPM stop the host from starting at all — including for work that never needs the key.
    pub fn at(key_path: &Path) -> Self {
        Self {
            key_path: key_path.to_path_buf(),
        }
    }

    /// Creates the key if this profile has none yet. Called just before the first wrap, so a
    /// machine that cannot produce a KEK fails while nothing has been committed.
    pub fn ensure_exists(&self) -> FcpResult<()> {
        backend::ensure_exists(&self.key_path)
    }

    pub(crate) fn wrap_dek(&self, dek: &SecretDek) -> FcpResult<Vec<u8>> {
        backend::wrap_dek(&self.key_path, dek)
    }

    /// Callers reaching an inject path must consume a user-verified capability first; enrollment,
    /// eviction and reconciliation deliberately use this silent primitive with no prompt.
    pub(crate) fn unwrap_dek(&self, wrapped: &[u8]) -> FcpResult<SecretDek> {
        backend::unwrap_dek(&self.key_path, wrapped)
    }
}
