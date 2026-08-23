//! Protects small secrets that must survive a reboot and stay unreadable to anyone but this user
//! on this machine: the audit chain's HMAC key and the snapshot integrity key.
//!
//! Neither secret is the vault's confidentiality — that is the KEK's job. What these protect is
//! *integrity evidence*. An attacker who can read the audit key can forge a convincing audit
//! chain, which is worse than a missing one because it looks trustworthy.
//!
//! The two platforms reach the same guarantee by different means. Windows hands the secret to
//! DPAPI, which binds it to the user's credentials, so another account cannot read it and neither
//! can an offline copy of the disk. Linux has no equivalent service, and a mode-0600 file would
//! only cover the first half of that — so the secret is sealed to the TPM instead, which covers
//! both and binds it to the machine as well.
//!
//! Both are opaque blobs to the caller: it stores what `protect` returns and hands it back.

use crate::FcpResult;

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "linux.rs")]
mod backend;

pub fn protect(plaintext: &[u8]) -> FcpResult<Vec<u8>> {
    backend::protect(plaintext)
}

pub fn unprotect(ciphertext: &[u8]) -> FcpResult<Vec<u8>> {
    backend::unprotect(ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs against whichever backend was compiled in. On Linux it needs a TPM, so it is ignored
    /// unless one is present; on Windows DPAPI is always available.
    #[test]
    #[cfg_attr(unix, ignore = "requires a TPM")]
    fn round_trips_and_rejects_a_damaged_blob() {
        let plaintext = b"audit-secret-fixture";
        let protected = protect(plaintext).unwrap();
        assert_ne!(protected, plaintext);
        assert_eq!(unprotect(&protected).unwrap(), plaintext);

        // A single flipped bit must fail rather than return plausible-looking bytes: the caller
        // treats a successful unprotect as proof the material is the one it stored.
        let mut damaged = protected;
        let middle = damaged.len() / 2;
        damaged[middle] ^= 0x40;
        assert!(unprotect(&damaged).is_err());
    }
}
