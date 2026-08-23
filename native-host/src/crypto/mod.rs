pub mod aead;
pub mod authorizer;
pub mod capability;
pub mod platform_kek;
pub mod webauthn_codec;

use crate::{FcpError, FcpResult};

/// Both backends draw from the OS CSPRNG rather than a userspace generator, so entropy quality is
/// the kernel's problem and a fork or a VM snapshot cannot replay a seeded stream.
#[cfg(windows)]
pub fn fill_random(bytes: &mut [u8]) -> FcpResult<()> {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    unsafe {
        BCryptGenRandom(None, bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
            .ok()
            .map_err(FcpError::from)
    }
}

/// `getrandom` reads the kernel CSPRNG through `getrandom(2)`, blocking only until the pool is
/// first initialised. Failure is surfaced rather than degraded: filling a nonce or a key with
/// anything less would silently weaken everything built on it.
#[cfg(unix)]
pub fn fill_random(bytes: &mut [u8]) -> FcpResult<()> {
    getrandom::fill(bytes).map_err(|error| {
        FcpError::Io(std::io::Error::other(format!(
            "the operating system random source failed: {error}"
        )))
    })
}
