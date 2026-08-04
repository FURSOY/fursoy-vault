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
