use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::config::{AccountGroupsConfig, PolicyLevel};
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
    /// Raw (r||s) ECDSA P-256 signature produced by WebAuthNAuthenticatorGetAssertion.
    pub signature: Vec<u8>,
    /// The authenticatorData bytes returned alongside `signature`. Verification needs these
    /// because the WebAuthn assertion signs `authenticatorData || SHA-256(clientDataJSON)`, not
    /// the payload bytes directly, and `authenticatorData` carries a signCount that cannot be
    /// recomputed from the payload alone.
    pub authenticator_data: Vec<u8>,
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
    pub extension_version: String,
    pub min_host_version: String,
    pub capabilities: Vec<String>,
    /// Digest of the config the extension has cached from a previous session, or `None` on a
    /// first run. ADR-020/Q24: the host owns the config, so this is a cache hint only — a
    /// mismatch is answered by sending the authoritative config down, not by failing closed.
    pub cached_config_digest: Option<String>,
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
    /// The authoritative account-group config. The extension keeps no config file of its own;
    /// it validates and caches whatever the host sends here (ADR-020 slice 2, Q24).
    pub config: AccountGroupsConfig,
    pub groups: Vec<HandshakeGroupState>,
    /// Cargo.toml's package version (e.g. "0.3.1") — lets the extension check it against the
    /// latest GitHub release and surface an update notice. The host never fetches anything
    /// itself; it only reports what it is.
    pub host_version: String,
    pub min_extension_version: String,
    pub capabilities: Vec<String>,
}

/// Extension → host: protect a new scope. The host assigns the UUID and persists the config;
/// the extension never invents group identities.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupAdd {
    pub scope: String,
    pub display_name: String,
    pub policy_level: PolicyLevel,
}

/// Extension → host: change an existing group's policy level. The scope and vault are untouched.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupSetPolicy {
    pub account_group_id: Uuid,
    pub policy_level: PolicyLevel,
}

/// Extension → host: stop protecting a scope. The vault and lease state are discarded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupRemove {
    pub account_group_id: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigUpdated {
    pub config_digest: String,
    pub config: AccountGroupsConfig,
    pub groups: Vec<HandshakeGroupState>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRejected {
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationError {
    pub request_id: Uuid,
    pub account_group_id: Option<Uuid>,
    pub code: String,
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
pub struct CookiesInjectChunk {
    pub account_group_id: Uuid,
    pub lease_id: Uuid,
    pub chunk_index: u32,
    pub chunk_count: u32,
    pub cookie_count: u32,
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
pub struct CookiesSnapshotChunk {
    pub account_group_id: Uuid,
    pub lease_id: Option<Uuid>,
    pub operation_id: Uuid,
    pub chunk_index: u32,
    pub chunk_count: u32,
    pub cookie_count: u32,
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
    /// ADR-020: at eviction time the protected scope held no cookies, so there is nothing to
    /// vault. The vault is discarded instead of sealing an empty jar that would demand a
    /// pointless Hello gesture on the next visit.
    ScopeEmpty,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorSeverity {
    Info,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorSignal {
    RemoteDebuggingPort,
    RemoteDebuggingPipe,
    ProcessInspectionWmiConnectAccessDenied,
    ProcessInspectionWmiConnectFailed,
    ProcessInspectionWmiPollAccessDenied,
    ProcessInspectionWmiPollFailed,
    ProcessInspectionCommandLineAccessDenied,
    ProcessInspectionCommandLineUnavailable,
    HostDisconnect,
    HostDisconnectActiveLease,
    ReconnectSuccess,
    ReconciliationFailed,
    AuditIntegrityRecovered,
    LeaseOutsideCookieCreated,
    SelectorChanged,
    MonitorQueueOverflow,
}

impl MonitorSignal {
    pub fn severity(self) -> MonitorSeverity {
        match self {
            Self::RemoteDebuggingPort
            | Self::RemoteDebuggingPipe
            | Self::HostDisconnectActiveLease
            | Self::ReconciliationFailed
            | Self::AuditIntegrityRecovered => MonitorSeverity::High,
            Self::LeaseOutsideCookieCreated | Self::MonitorQueueOverflow => MonitorSeverity::Medium,
            Self::HostDisconnect
            | Self::ReconnectSuccess
            | Self::SelectorChanged
            | Self::ProcessInspectionWmiConnectAccessDenied
            | Self::ProcessInspectionWmiConnectFailed
            | Self::ProcessInspectionWmiPollAccessDenied
            | Self::ProcessInspectionWmiPollFailed
            | Self::ProcessInspectionCommandLineAccessDenied
            | Self::ProcessInspectionCommandLineUnavailable => MonitorSeverity::Info,
        }
    }

    pub fn audit_code(self) -> &'static str {
        match self {
            Self::RemoteDebuggingPort => "remote_debugging_port",
            Self::RemoteDebuggingPipe => "remote_debugging_pipe",
            Self::ProcessInspectionWmiConnectAccessDenied => {
                "process_inspection_wmi_connect_wbem_access_denied_0x80041003"
            }
            Self::ProcessInspectionWmiConnectFailed => "process_inspection_wmi_connect_failed",
            Self::ProcessInspectionWmiPollAccessDenied => {
                "process_inspection_wmi_poll_wbem_access_denied_0x80041003"
            }
            Self::ProcessInspectionWmiPollFailed => "process_inspection_wmi_poll_failed",
            Self::ProcessInspectionCommandLineAccessDenied => {
                "process_inspection_command_line_wbem_access_denied_0x80041003"
            }
            Self::ProcessInspectionCommandLineUnavailable => {
                "process_inspection_command_line_unavailable"
            }
            Self::HostDisconnect => "host_disconnect",
            Self::HostDisconnectActiveLease => "host_disconnect_active_lease",
            Self::ReconnectSuccess => "reconnect_success",
            Self::ReconciliationFailed => "reconciliation_failed",
            Self::AuditIntegrityRecovered => "audit_integrity_recovered",
            Self::LeaseOutsideCookieCreated => "lease_outside_cookie_created",
            Self::SelectorChanged => "selector_changed",
            Self::MonitorQueueOverflow => "monitor_queue_overflow",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorSource {
    Extension,
    NativeHost,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorEvent {
    pub event_id: Uuid,
    pub observed_at_unix_ms: u64,
    pub source: MonitorSource,
    pub signal: MonitorSignal,
    pub severity: MonitorSeverity,
    pub account_group_id: Option<Uuid>,
    pub occurrence_count: u32,
}

impl MonitorEvent {
    pub fn validate(&self) -> FcpResult<()> {
        if self.event_id.is_nil() {
            return Err(FcpError::Protocol(
                "monitor event id must not be nil".into(),
            ));
        }
        if self.severity != self.signal.severity() {
            return Err(FcpError::Protocol(
                "monitor severity does not match signal".into(),
            ));
        }
        if self.occurrence_count == 0 || self.occurrence_count > 1_000 {
            return Err(FcpError::Protocol(
                "monitor occurrence count is out of range".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorPoll {
    pub max_events: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorAlert {
    pub event: MonitorEvent,
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
    #[serde(rename = "cookies.inject.chunk")]
    CookiesInjectChunk(CookiesInjectChunk),
    #[serde(rename = "inject.result")]
    InjectResult(InjectResult),
    #[serde(rename = "evict.request")]
    EvictRequest(EvictRequest),
    #[serde(rename = "cookies.snapshot.chunk")]
    CookiesSnapshotChunk(CookiesSnapshotChunk),
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
    #[serde(rename = "monitor.event")]
    MonitorEvent(MonitorEvent),
    #[serde(rename = "monitor.poll")]
    MonitorPoll(MonitorPoll),
    #[serde(rename = "monitor.alert")]
    MonitorAlert(MonitorAlert),
    #[serde(rename = "group.add")]
    GroupAdd(GroupAdd),
    #[serde(rename = "group.remove")]
    GroupRemove(GroupRemove),
    #[serde(rename = "group.set_policy")]
    GroupSetPolicy(GroupSetPolicy),
    #[serde(rename = "config.updated")]
    ConfigUpdated(ConfigUpdated),
    #[serde(rename = "config.rejected")]
    ConfigRejected(ConfigRejected),
    #[serde(rename = "operation.error")]
    OperationError(OperationError),
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

    #[test]
    fn monitor_severity_is_derived_from_a_closed_signal_vocabulary() {
        assert_eq!(
            MonitorSignal::RemoteDebuggingPort.severity(),
            MonitorSeverity::High
        );
        let mut event = MonitorEvent {
            event_id: Uuid::new_v4(),
            observed_at_unix_ms: 1,
            source: MonitorSource::Extension,
            signal: MonitorSignal::SelectorChanged,
            severity: MonitorSeverity::Info,
            account_group_id: None,
            occurrence_count: 1,
        };
        assert!(event.validate().is_ok());
        event.severity = MonitorSeverity::High;
        assert!(event.validate().is_err());
    }
}
