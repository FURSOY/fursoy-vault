use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::{FcpError, FcpResult};

pub const CAPABILITY_DOMAIN: &[u8; 8] = b"FCPHCAP1";
pub const CAPABILITY_NONCE_BYTES: usize = 32;
pub const CAPABILITY_MAX_LIFETIME_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum CapabilityOperation {
    Inject = 1,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Nonce32(pub [u8; CAPABILITY_NONCE_BYTES]);

impl Serialize for Nonce32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&encode_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for Nonce32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        decode_nonce(&encoded).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPayload {
    pub account_group_id: Uuid,
    pub operation: CapabilityOperation,
    pub expiry_unix_ms: u64,
    pub monotonic_sequence: u64,
    pub nonce: Nonce32,
}

impl CapabilityPayload {
    /// Canonical bytes signed by Windows Hello. JSON is deliberately not signed because different
    /// encoders can produce equivalent but byte-distinct representations.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            CAPABILITY_DOMAIN.len() + 16 + 1 + size_of::<u64>() * 2 + CAPABILITY_NONCE_BYTES,
        );
        bytes.extend_from_slice(CAPABILITY_DOMAIN);
        bytes.extend_from_slice(self.account_group_id.as_bytes());
        bytes.push(self.operation as u8);
        bytes.extend_from_slice(&self.expiry_unix_ms.to_le_bytes());
        bytes.extend_from_slice(&self.monotonic_sequence.to_le_bytes());
        bytes.extend_from_slice(&self.nonce.0);
        bytes
    }

    pub fn validate_shape(&self) -> FcpResult<()> {
        if self.account_group_id.is_nil() {
            return Err(FcpError::Capability(
                "account_group_id must not be nil".into(),
            ));
        }
        if self.monotonic_sequence == 0 {
            return Err(FcpError::Capability(
                "monotonic_sequence must start at one".into(),
            ));
        }
        if self.nonce.0.iter().all(|byte| *byte == 0) {
            return Err(FcpError::Capability("nonce must not be all-zero".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCapability {
    pub payload: CapabilityPayload,
    /// RSA PKCS#1 SHA-256 signature produced by KeyCredential.RequestSignAsync.
    pub signature: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookiePartitionKey {
    pub top_level_site: Option<String>,
    pub has_cross_site_ancestor: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SameSiteStatus {
    NoRestriction,
    Lax,
    Strict,
    Unspecified,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookieRecord {
    pub domain: String,
    pub expiration_date: Option<f64>,
    pub host_only: bool,
    pub http_only: bool,
    pub name: String,
    pub partition_key: Option<CookiePartitionKey>,
    pub path: String,
    pub same_site: SameSiteStatus,
    pub secure: bool,
    pub session: bool,
    pub store_id: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handshake {
    pub protocol_version: u16,
    pub extension_id: String,
    pub config_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupState {
    Uninitialized,
    Sealed,
    Unlocking,
    Leased,
    Evicting,
    Degraded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeAck {
    pub protocol_version: u16,
    pub config_digest: String,
    pub groups: Vec<HandshakeGroupState>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeGroupState {
    pub account_group_id: Uuid,
    pub group_state: GroupState,
    pub reconciliation_required: bool,
    pub lease_id: Option<Uuid>,
    pub lease_expiry_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeasePurpose {
    Inject,
    Enroll,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseRequest {
    pub account_group_id: Uuid,
    pub purpose: LeasePurpose,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseGrant {
    pub account_group_id: Uuid,
    pub lease_id: Uuid,
    pub expiry_unix_ms: u64,
    pub capability_sequence: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseDeny {
    pub account_group_id: Uuid,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookiesInject {
    pub account_group_id: Uuid,
    pub lease_id: Uuid,
    pub cookies: Vec<CookieRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InjectResult {
    pub account_group_id: Uuid,
    pub lease_id: Uuid,
    pub success: bool,
    pub health_check: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictPhase {
    Begin,
    SnapshotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvictRequest {
    pub account_group_id: Uuid,
    pub lease_id: Option<Uuid>,
    pub operation_id: Uuid,
    pub phase: EvictPhase,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookiesSnapshot {
    pub account_group_id: Uuid,
    pub lease_id: Option<Uuid>,
    pub operation_id: Uuid,
    pub cookies: Vec<CookieRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookieDisposition {
    Remove,
    RetainLeased,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvictConfirmed {
    pub account_group_id: Uuid,
    pub lease_id: Option<Uuid>,
    pub operation_id: Uuid,
    pub vault_sequence: u64,
    pub cookie_disposition: CookieDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvictResult {
    pub account_group_id: Uuid,
    pub lease_id: Option<Uuid>,
    pub operation_id: Uuid,
    pub success: bool,
    pub remaining_cookie_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionInvalidationReason {
    ExternalLogout,
    RestoreRejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInvalidate {
    pub account_group_id: Uuid,
    pub lease_id: Option<Uuid>,
    pub reason: SessionInvalidationReason,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInvalidated {
    pub account_group_id: Uuid,
    pub reason: SessionInvalidationReason,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthCacheClear {
    pub account_group_id: Uuid,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Message {
    #[serde(rename = "handshake")]
    Handshake(Handshake),
    #[serde(rename = "handshake.ack")]
    HandshakeAck(HandshakeAck),
    #[serde(rename = "lease.request")]
    LeaseRequest(LeaseRequest),
    #[serde(rename = "lease.grant")]
    LeaseGrant(LeaseGrant),
    #[serde(rename = "lease.deny")]
    LeaseDeny(LeaseDeny),
    #[serde(rename = "cookies.inject")]
    CookiesInject(CookiesInject),
    #[serde(rename = "inject.result")]
    InjectResult(InjectResult),
    #[serde(rename = "evict.request")]
    EvictRequest(EvictRequest),
    #[serde(rename = "cookies.snapshot")]
    CookiesSnapshot(CookiesSnapshot),
    #[serde(rename = "evict.confirmed")]
    EvictConfirmed(EvictConfirmed),
    #[serde(rename = "evict.result")]
    EvictResult(EvictResult),
    #[serde(rename = "session.invalidate")]
    SessionInvalidate(SessionInvalidate),
    #[serde(rename = "session.invalidated")]
    SessionInvalidated(SessionInvalidated),
    #[serde(rename = "auth.cache.clear")]
    AuthCacheClear(AuthCacheClear),
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_nonce(encoded: &str) -> Result<Nonce32, String> {
    if encoded.len() != CAPABILITY_NONCE_BYTES * 2 {
        return Err(format!(
            "nonce must contain {} lowercase hexadecimal characters",
            CAPABILITY_NONCE_BYTES * 2
        ));
    }
    let mut nonce = [0u8; CAPABILITY_NONCE_BYTES];
    for (index, slot) in nonce.iter_mut().enumerate() {
        let offset = index * 2;
        let high = decode_nibble(encoded.as_bytes()[offset])?;
        let low = decode_nibble(encoded.as_bytes()[offset + 1])?;
        *slot = high << 4 | low;
    }
    Ok(Nonce32(nonce))
}

fn decode_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err("nonce must use lowercase hexadecimal encoding".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_capability_binds_every_security_field() {
        let base = CapabilityPayload {
            account_group_id: Uuid::from_u128(1),
            operation: CapabilityOperation::Inject,
            expiry_unix_ms: 10,
            monotonic_sequence: 11,
            nonce: Nonce32([7; 32]),
        };
        let canonical = base.canonical_bytes();

        let variants = [
            CapabilityPayload {
                account_group_id: Uuid::from_u128(2),
                ..base.clone()
            },
            CapabilityPayload {
                expiry_unix_ms: 12,
                ..base.clone()
            },
            CapabilityPayload {
                monotonic_sequence: 13,
                ..base.clone()
            },
            CapabilityPayload {
                nonce: Nonce32([8; 32]),
                ..base.clone()
            },
        ];

        for variant in variants {
            assert_ne!(canonical, variant.canonical_bytes());
        }
    }

    #[test]
    fn nonce_json_is_fixed_lowercase_hex() {
        let nonce = Nonce32([0xab; 32]);
        let encoded = serde_json::to_string(&nonce).expect("serialize nonce");
        assert_eq!(encoded, format!("\"{}\"", "ab".repeat(32)));
        assert_eq!(serde_json::from_str::<Nonce32>(&encoded).unwrap(), nonce);
    }

    #[test]
    fn enrollment_and_eviction_dispositions_are_wire_distinct() {
        assert_eq!(
            serde_json::to_string(&CookieDisposition::RetainLeased).unwrap(),
            "\"retain_leased\""
        );
        assert_eq!(
            serde_json::to_string(&CookieDisposition::Remove).unwrap(),
            "\"remove\""
        );
    }

    #[test]
    fn evict_is_not_a_hello_capability_operation() {
        assert!(serde_json::from_str::<CapabilityOperation>("\"evict\"").is_err());
        assert_eq!(
            serde_json::to_string(&CapabilityOperation::Inject).unwrap(),
            "\"inject\""
        );
    }

    #[test]
    fn noninteractive_lease_grant_has_no_capability_sequence() {
        let grant = LeaseGrant {
            account_group_id: Uuid::from_u128(1),
            lease_id: Uuid::from_u128(2),
            expiry_unix_ms: 3,
            capability_sequence: None,
        };
        let encoded = serde_json::to_value(grant).unwrap();
        assert!(encoded["capability_sequence"].is_null());
    }

    #[test]
    fn session_invalidation_reason_is_a_closed_redacted_vocabulary() {
        assert_eq!(
            serde_json::to_string(&SessionInvalidationReason::ExternalLogout).unwrap(),
            "\"external_logout\""
        );
        assert!(serde_json::from_str::<SessionInvalidationReason>("\"unknown\"").is_err());
    }
}
