use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

use crate::{FcpError, FcpResult};

// Renamed pre-launch (2026-08-08, ADR-023) while it is still free to do so: this is DPAPI
// entropy, not a display string. Changing it changes the derived key, making every existing
// DPAPI-protected audit blob (audit-key.dpapi, audit-anchor.dpapi) undecryptable on next start —
// deliberately accepted now because no real user data exists yet; do not rename again after
// launch without a real migration plan.
const AUDIT_ENTROPY: &[u8] = b"FURSOY.Vault.Audit.v2";

pub fn protect(plaintext: &[u8]) -> FcpResult<Vec<u8>> {
    transform(plaintext, true)
}

pub fn unprotect(ciphertext: &[u8]) -> FcpResult<Vec<u8>> {
    transform(ciphertext, false)
}

fn transform(input: &[u8], encrypt: bool) -> FcpResult<Vec<u8>> {
    let input_len = u32::try_from(input.len())
        .map_err(|_| FcpError::Format("DPAPI input exceeds u32".into()))?;
    let entropy_len = u32::try_from(AUDIT_ENTROPY.len())
        .map_err(|_| FcpError::Format("DPAPI entropy exceeds u32".into()))?;
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input_len,
        pbData: input.as_ptr().cast_mut(),
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy_len,
        pbData: AUDIT_ENTROPY.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if encrypt {
            CryptProtectData(
                &input_blob,
                windows::core::w!("FURSOY audit integrity material"),
                Some(&entropy_blob),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        } else {
            CryptUnprotectData(
                &input_blob,
                None,
                Some(&entropy_blob),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        }
    }
    if output.pbData.is_null() && output.cbData != 0 {
        return Err(FcpError::Crypto("DPAPI returned an invalid output blob"));
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_user_dpapi_round_trip_and_wrong_blob_rejection() {
        let plaintext = b"audit-secret-fixture";
        let protected = protect(plaintext).unwrap();
        assert_ne!(protected, plaintext);
        assert_eq!(unprotect(&protected).unwrap(), plaintext);
        let mut damaged = protected;
        let middle = damaged.len() / 2;
        damaged[middle] ^= 0x40;
        assert!(unprotect(&damaged).is_err());
    }
}
