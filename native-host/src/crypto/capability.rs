use crate::crypto::fill_random;
use crate::protocol::messages::{
    CAPABILITY_MAX_LIFETIME_MS, CapabilityOperation, CapabilityPayload, Nonce32, SignedCapability,
};
use crate::{FcpError, FcpResult};
use uuid::Uuid;

pub trait CapabilitySigner {
    fn sign(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability>;
}

pub trait CapabilityVerifier {
    fn verify_signature(&self, capability: &SignedCapability) -> FcpResult<()>;
}

/// A platform's user-verified signing authority: Windows Hello today, a TPM key gated by a PIN on
/// Linux (prototyped under `poc/linux-authorizer`).
///
/// Signing and verifying are the two halves above. What this adds is the credential lifecycle,
/// which is the part the dispatcher used to have to know platform details about: it previously
/// recognised a Windows-specific error string to decide when re-enrolment was safe. That decision
/// belongs to whoever knows what the platform's errors mean, so it lives here.
///
/// Implementations must hold this line: a signature may only be produced after the platform has
/// actually verified the user. How that is proven differs — Windows Hello asserts it in the signed
/// `proof_context` and checks the flag on the way back; a TPM key with an authValue simply cannot
/// sign without the PIN — but an implementation that can sign without user verification breaks the
/// guarantee every caller here relies on.
pub trait PlatformAuthorizer: CapabilitySigner + CapabilityVerifier + Sized {
    /// Opens the enrolled credential, enrolling one if none exists yet.
    fn open_or_create(credential_path: &std::path::Path) -> FcpResult<Self>;

    /// Opens an already-enrolled credential and never creates one. Recovery uses this: silently
    /// enrolling a fresh credential would turn a missing or corrupt registry into an
    /// authorization bypass, because the new credential would authorize anyone holding the device.
    fn open_existing(credential_path: &std::path::Path) -> FcpResult<Self>;

    /// Recovers from the platform having discarded the credential on its own — a Hello reset,
    /// account recovery, TPM maintenance. Returns whether re-enrolment happened, so a caller can
    /// retry exactly once.
    ///
    /// Only the backend can tell that case apart from a user cancelling or failing verification,
    /// and the difference matters: re-enrolling after a *refusal* would let a failed check be
    /// retried into a fresh credential. Implementations must re-enrol only for a genuine
    /// not-found signal from the platform.
    fn recover_if_credential_vanished(
        &mut self,
        error: &FcpError,
        credential_path: &std::path::Path,
    ) -> FcpResult<bool>;
}

pub fn new_payload(
    account_group_id: Uuid,
    operation: CapabilityOperation,
    now_unix_ms: u64,
    lifetime_ms: u64,
    monotonic_sequence: u64,
) -> FcpResult<CapabilityPayload> {
    if lifetime_ms == 0 || lifetime_ms > CAPABILITY_MAX_LIFETIME_MS {
        return Err(FcpError::Capability(format!(
            "lifetime must be within 1..={CAPABILITY_MAX_LIFETIME_MS} ms"
        )));
    }
    let expiry_unix_ms = now_unix_ms
        .checked_add(lifetime_ms)
        .ok_or_else(|| FcpError::Capability("expiry overflow".into()))?;
    let mut nonce = [0u8; 32];
    fill_random(&mut nonce)?;
    let payload = CapabilityPayload {
        account_group_id,
        operation,
        expiry_unix_ms,
        monotonic_sequence,
        nonce: Nonce32(nonce),
    };
    payload.validate_shape()?;
    Ok(payload)
}
