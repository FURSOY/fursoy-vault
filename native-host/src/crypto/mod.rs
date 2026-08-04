pub mod aead;
pub mod capability;
pub mod hello;
pub mod platform_kek;

use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};

use crate::{FcpError, FcpResult};

pub fn fill_random(bytes: &mut [u8]) -> FcpResult<()> {
    unsafe {
        BCryptGenRandom(None, bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
            .ok()
            .map_err(FcpError::from)
    }
}
