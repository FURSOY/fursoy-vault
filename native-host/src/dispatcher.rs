use std::collections::BTreeMap;

use uuid::Uuid;

use crate::audit::{AuditLogger, unix_ms};
use crate::config::{LoadedConfig, PolicyLevel};
use crate::crypto::hello::HelloAuthorizer;
use crate::lease::metadata::{LeaseMetadata, LeaseMetadataStore};
use crate::lease::store::FileCapabilityLedgerStore;
use crate::paths::DataPaths;
use crate::protocol::envelope::PROTOCOL_VERSION;
use crate::protocol::messages::{
    AuthCacheClear, CookieDisposition, CookiesInject, CookiesSnapshot, EvictConfirmed, EvictPhase,
    EvictRequest, EvictResult, GroupState, Handshake, HandshakeAck, HandshakeGroupState,
    InjectResult, LeaseDeny, LeaseGrant, LeasePurpose, LeaseRequest, Message, SessionInvalidate,
    SessionInvalidated, SessionInvalidationReason,
};
use crate::transaction::VaultTransactions;
use crate::vault::store::VaultStore;
use crate::{FcpError, FcpResult, WIKIPEDIA_ACCOUNT_GROUP_ID};

pub const NATIVE_HOST_NAME: &str = "com.fursoy.cookie_protector";
pub const PRODUCT_EXTENSION_ID: &str = "ikodegbaomnahbjiokfogpedaoifhbde";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingKind {
    Enrollment,
    Eviction,
    Reconciliation,
}

struct PendingOperation {
    kind: PendingKind,
    operation_id: Uuid,
    lease_id: Option<Uuid>,
    confirmed: bool,
    snapshot_cookie_count: Option<u32>,
}

struct GroupRuntime {
    id: Uuid,
    policy: PolicyLevel,
    transactions: VaultTransactions,
    lease_store: LeaseMetadataStore,
    lease: LeaseMetadata,
    pending: Option<PendingOperation>,
    hello_cache_expires_at: Option<u64>,
}

pub struct NativeHostApp {
    groups: BTreeMap<Uuid, GroupRuntime>,
    audit: AuditLogger,
    config_digest: String,
    handshake_complete: bool,
    last_message_group: Option<Uuid>,
    hello_authorizer: Option<HelloAuthorizer>,
}

impl NativeHostApp {
    pub fn open(paths: &DataPaths) -> FcpResult<Self> {
        let loaded = LoadedConfig::load(&paths.account_groups_config)?;
        paths.migrate_phase5_group(WIKIPEDIA_ACCOUNT_GROUP_ID)?;
        let mut groups = BTreeMap::new();
        for definition in &loaded.config.groups {
            let vault_store = VaultStore::new(&paths.vault_groups);
            let vault_exists = vault_store.path_for(definition.id).exists();
            let capability_store =
                FileCapabilityLedgerStore::new(paths.capability_path(definition.id));
            let transactions =
                VaultTransactions::open(definition.id, vault_store, capability_store)?;
            let lease_store = LeaseMetadataStore::new(paths.lease_path(definition.id));
            let lease = lease_store.load_or_initialize(definition.id, vault_exists)?;
            groups.insert(
                definition.id,
                GroupRuntime {
                    id: definition.id,
                    policy: definition.policy_level,
                    transactions,
                    lease_store,
                    lease,
                    pending: None,
                    hello_cache_expires_at: None,
                },
            );
        }
        Ok(Self {
            groups,
            audit: AuditLogger::new(&paths.audit_directory),
            config_digest: loaded.digest,
            handshake_complete: false,
            last_message_group: None,
            hello_authorizer: None,
        })
    }

    pub fn handle(&mut self, message: Message) -> FcpResult<Vec<Message>> {
        if !self.handshake_complete {
            return match message {
                Message::Handshake(handshake) => self.handle_handshake(handshake),
                _ => Err(FcpError::Protocol(
                    "handshake must be the first application message".into(),
                )),
            };
        }
        if matches!(message, Message::Handshake(_) | Message::HandshakeAck(_)) {
            return Err(FcpError::Protocol(
                "handshake cannot be repeated on one connection".into(),
            ));
        }
        let group_id = message_group_id(&message).ok_or_else(|| {
            FcpError::Protocol("message direction is host-to-extension only".into())
        })?;
        self.last_message_group = Some(group_id);
        let runtime = self
            .groups
            .get_mut(&group_id)
            .ok_or_else(|| FcpError::Protocol("unknown account group".into()))?;
        runtime.handle(message, &self.audit, &mut self.hello_authorizer)
    }

    pub fn deny_for_error(&self, _error: &FcpError) -> Message {
        let group_id = self
            .last_message_group
            .or_else(|| self.groups.keys().next().copied())
            .unwrap_or(WIKIPEDIA_ACCOUNT_GROUP_ID);
        Message::LeaseDeny(LeaseDeny {
            account_group_id: group_id,
            reason: "host_error_fail_closed".into(),
        })
    }

    fn handle_handshake(&mut self, handshake: Handshake) -> FcpResult<Vec<Message>> {
        if handshake.protocol_version != PROTOCOL_VERSION {
            return Err(FcpError::Protocol(
                "handshake protocol version mismatch".into(),
            ));
        }
        if handshake.extension_id != PRODUCT_EXTENSION_ID {
            return Err(FcpError::Protocol("unexpected extension id".into()));
        }
        if handshake.config_digest != self.config_digest {
            return Err(FcpError::Protocol(
                "account-group config digest mismatch".into(),
            ));
        }
        self.handshake_complete = true;
        let states = self
            .groups
            .values()
            .map(|runtime| HandshakeGroupState {
                account_group_id: runtime.id,
                group_state: runtime.lease.state.clone(),
                reconciliation_required: matches!(
                    runtime.lease.state,
                    GroupState::Unlocking | GroupState::Evicting | GroupState::Degraded
                ),
                lease_id: runtime.lease.lease_id,
                lease_expiry_unix_ms: runtime.lease.expires_at_unix_ms,
            })
            .collect();
        for group_id in self.groups.keys().copied() {
            self.audit
                .record(group_id, "handshake", "success", None, None)?;
        }
        Ok(vec![Message::HandshakeAck(HandshakeAck {
            protocol_version: PROTOCOL_VERSION,
            config_digest: self.config_digest.clone(),
            groups: states,
        })])
    }
}

impl GroupRuntime {
    fn handle(
        &mut self,
        message: Message,
        audit: &AuditLogger,
        hello_authorizer: &mut Option<HelloAuthorizer>,
    ) -> FcpResult<Vec<Message>> {
        match message {
            Message::LeaseRequest(request) => {
                self.handle_lease_request(request, audit, hello_authorizer)
            }
            Message::InjectResult(result) => self.handle_inject_result(result, audit),
            Message::EvictRequest(request) => {
                self.handle_evict_begin(request, audit, hello_authorizer)
            }
            Message::CookiesSnapshot(snapshot) => self.handle_snapshot(snapshot, audit),
            Message::EvictResult(result) => self.handle_evict_result(result, audit),
            Message::SessionInvalidate(request) => self.handle_session_invalidate(request, audit),
            Message::AuthCacheClear(request) => {
                self.handle_auth_cache_clear(request, audit, hello_authorizer)
            }
            _ => Err(FcpError::Protocol(
                "message direction is host-to-extension only".into(),
            )),
        }
    }

    fn handle_lease_request(
        &mut self,
        request: LeaseRequest,
        audit: &AuditLogger,
        hello_authorizer: &mut Option<HelloAuthorizer>,
    ) -> FcpResult<Vec<Message>> {
        if self.pending.is_some() {
            return Ok(vec![self.lease_deny("operation_pending")]);
        }
        let policy = self.policy.parameters();
        if policy.monitoring_only {
            return Ok(vec![self.lease_deny("monitoring_policy_has_no_lease")]);
        }
        match request.purpose {
            LeasePurpose::Enroll => self.begin_enrollment(policy.lease_duration_ms, audit),
            LeasePurpose::Inject => {
                self.begin_inject(policy.lease_duration_ms, audit, hello_authorizer)
            }
        }
    }

    fn begin_enrollment(
        &mut self,
        duration_ms: u64,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        if self.lease.state != GroupState::Uninitialized || self.transactions.vault_exists() {
            return Ok(vec![self.lease_deny("group_already_enrolled")]);
        }
        let now = unix_ms()?;
        let expiry = now
            .checked_add(duration_ms)
            .ok_or_else(|| FcpError::Protocol("lease expiry overflow".into()))?;
        let lease_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        self.lease.state = GroupState::Evicting;
        self.lease.lease_id = Some(lease_id);
        self.lease.granted_at_unix_ms = Some(now);
        self.lease.expires_at_unix_ms = Some(expiry);
        self.lease.pending_operation_id = Some(operation_id);
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        self.pending = Some(PendingOperation {
            kind: PendingKind::Enrollment,
            operation_id,
            lease_id: Some(lease_id),
            confirmed: false,
            snapshot_cookie_count: None,
        });
        audit.record(self.id, "enrollment", "started", Some(operation_id), None)?;
        Ok(vec![
            Message::LeaseGrant(LeaseGrant {
                account_group_id: self.id,
                lease_id,
                expiry_unix_ms: expiry,
                capability_sequence: None,
            }),
            Message::EvictRequest(EvictRequest {
                account_group_id: self.id,
                lease_id: Some(lease_id),
                operation_id,
                phase: EvictPhase::SnapshotRequired,
                reason: "initial_enrollment".into(),
            }),
        ])
    }

    fn begin_inject(
        &mut self,
        duration_ms: u64,
        audit: &AuditLogger,
        hello_authorizer: &mut Option<HelloAuthorizer>,
    ) -> FcpResult<Vec<Message>> {
        if self.lease.state != GroupState::Sealed || !self.transactions.vault_exists() {
            return Ok(vec![self.lease_deny("group_not_sealed")]);
        }
        let now = unix_ms()?;
        let cache_duration = self.policy.parameters().hello_cache_ms.unwrap_or(0);
        if hello_authorizer.is_none() {
            *hello_authorizer = Some(HelloAuthorizer::open_or_create()?);
        }
        let authorizer = hello_authorizer.as_ref().ok_or_else(|| {
            FcpError::Capability("Windows Hello authorizer was not initialized".into())
        })?;
        let use_cached = cache_duration > 0
            && self
                .hello_cache_expires_at
                .is_some_and(|expiry| expiry > now)
            && authorizer.has_cached_handle(self.id);
        let authorization = self.transactions.authorize_inject(authorizer, use_cached)?;
        let capability_sequence = authorization.monotonic_sequence();
        let payload = self.transactions.read_for_inject(authorization)?;
        if cache_duration > 0 {
            self.hello_cache_expires_at = now.checked_add(cache_duration);
        } else {
            self.hello_cache_expires_at = None;
            authorizer.clear_cached_handle(self.id);
        }
        let expiry = now
            .checked_add(duration_ms)
            .ok_or_else(|| FcpError::Protocol("lease expiry overflow".into()))?;
        let lease_id = Uuid::new_v4();
        self.lease.state = GroupState::Leased;
        self.lease.lease_id = Some(lease_id);
        self.lease.granted_at_unix_ms = Some(now);
        self.lease.expires_at_unix_ms = Some(expiry);
        self.lease.pending_operation_id = None;
        self.lease.vault_sequence = payload.vault_sequence;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        audit.record(
            self.id,
            "inject",
            "authorized",
            None,
            Some(if use_cached {
                "hello_cached"
            } else {
                "hello_fresh"
            }),
        )?;
        Ok(vec![
            Message::LeaseGrant(LeaseGrant {
                account_group_id: self.id,
                lease_id,
                expiry_unix_ms: expiry,
                capability_sequence: Some(capability_sequence),
            }),
            Message::CookiesInject(CookiesInject {
                account_group_id: self.id,
                lease_id,
                cookies: payload.cookies,
            }),
        ])
    }

    fn handle_inject_result(
        &mut self,
        result: InjectResult,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        if self.lease.state != GroupState::Leased || self.lease.lease_id != Some(result.lease_id) {
            return Err(FcpError::Protocol("inject result lease mismatch".into()));
        }
        if result.success && result.health_check == "authenticated" {
            audit.record(self.id, "inject", "success", None, None)?;
            return Ok(Vec::new());
        }
        let detail_code = inject_failure_code(&result.health_check);
        audit.record(self.id, "inject", "failed", None, Some(detail_code))?;
        if matches!(
            result.health_check.as_str(),
            "logged_out" | "invalid_session"
        ) {
            return self.invalidate_session(SessionInvalidationReason::RestoreRejected, audit);
        }
        self.lease.state = GroupState::Degraded;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        Ok(Vec::new())
    }

    fn handle_session_invalidate(
        &mut self,
        request: SessionInvalidate,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        if self.pending.is_some() {
            return Err(FcpError::Protocol(
                "session invalidation refused while an operation is pending".into(),
            ));
        }
        if self.lease.state == GroupState::Uninitialized {
            return Ok(vec![Message::SessionInvalidated(SessionInvalidated {
                account_group_id: self.id,
                reason: request.reason,
            })]);
        }
        if request.lease_id.is_some() && request.lease_id != self.lease.lease_id {
            return Err(FcpError::Protocol(
                "session invalidation lease mismatch".into(),
            ));
        }
        self.invalidate_session(request.reason, audit)
    }

    fn handle_auth_cache_clear(
        &mut self,
        request: AuthCacheClear,
        audit: &AuditLogger,
        hello_authorizer: &mut Option<HelloAuthorizer>,
    ) -> FcpResult<Vec<Message>> {
        if request.reason != "locked" && request.reason != "policy_changed" {
            return Err(FcpError::Protocol(
                "unsupported auth cache clear reason".into(),
            ));
        }
        self.hello_cache_expires_at = None;
        if let Some(authorizer) = hello_authorizer.as_ref() {
            authorizer.clear_cached_handle(self.id);
        }
        audit.record(
            self.id,
            "auth_cache",
            "cleared",
            None,
            Some("lock_or_policy"),
        )?;
        Ok(Vec::new())
    }

    fn invalidate_session(
        &mut self,
        reason: SessionInvalidationReason,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        self.transactions.invalidate()?;
        self.lease.state = GroupState::Uninitialized;
        self.lease.lease_id = None;
        self.lease.granted_at_unix_ms = None;
        self.lease.expires_at_unix_ms = None;
        self.lease.pending_operation_id = None;
        self.lease.vault_sequence = 0;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        let detail = match reason {
            SessionInvalidationReason::ExternalLogout => "external_logout",
            SessionInvalidationReason::RestoreRejected => "restore_rejected",
        };
        audit.record(
            self.id,
            "session_invalidation",
            "success",
            None,
            Some(detail),
        )?;
        Ok(vec![Message::SessionInvalidated(SessionInvalidated {
            account_group_id: self.id,
            reason,
        })])
    }

    fn handle_evict_begin(
        &mut self,
        request: EvictRequest,
        audit: &AuditLogger,
        hello_authorizer: &mut Option<HelloAuthorizer>,
    ) -> FcpResult<Vec<Message>> {
        if request.phase != EvictPhase::Begin {
            return Err(FcpError::Protocol(
                "extension may send only evict.request phase=begin".into(),
            ));
        }
        if request.reason == "locked" {
            self.hello_cache_expires_at = None;
            if let Some(authorizer) = hello_authorizer.as_ref() {
                authorizer.clear_cached_handle(self.id);
            }
        }
        if self.pending.is_some() {
            return Err(FcpError::Protocol(
                "another operation is pending for group".into(),
            ));
        }
        let is_reconciliation = request.reason == "startup_reconciliation"
            && self.lease.state != GroupState::Uninitialized;
        let is_active_lease = self.lease.state == GroupState::Leased
            && self.lease.lease_id == request.lease_id
            && request.lease_id.is_some();
        let is_sealed_recreation = self.lease.state == GroupState::Sealed
            && request.lease_id.is_none()
            && request.reason == "site_cookie_recreated";
        let is_degraded_recovery = self.lease.state == GroupState::Degraded
            && self.lease.lease_id == request.lease_id
            && request.lease_id.is_some()
            && request.reason == "degraded_cookie_detected";
        if !is_reconciliation && !is_active_lease && !is_sealed_recreation && !is_degraded_recovery
        {
            return Err(FcpError::Protocol(
                "eviction request is not valid for current group lease".into(),
            ));
        }
        self.lease.state = GroupState::Evicting;
        self.lease.pending_operation_id = Some(request.operation_id);
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        self.pending = Some(PendingOperation {
            kind: if is_reconciliation {
                PendingKind::Reconciliation
            } else {
                PendingKind::Eviction
            },
            operation_id: request.operation_id,
            lease_id: request.lease_id,
            confirmed: false,
            snapshot_cookie_count: None,
        });
        audit.record(
            self.id,
            if is_reconciliation {
                "reconciliation"
            } else {
                "eviction"
            },
            "started",
            Some(request.operation_id),
            None,
        )?;
        Ok(vec![Message::EvictRequest(EvictRequest {
            account_group_id: self.id,
            lease_id: request.lease_id,
            operation_id: request.operation_id,
            phase: EvictPhase::SnapshotRequired,
            reason: request.reason,
        })])
    }

    fn handle_snapshot(
        &mut self,
        snapshot: CookiesSnapshot,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        let mut pending = self.pending.take().ok_or_else(|| {
            FcpError::Protocol("snapshot received without pending eviction".into())
        })?;
        if pending.operation_id != snapshot.operation_id || pending.lease_id != snapshot.lease_id {
            self.pending = Some(pending);
            return Err(FcpError::Protocol(
                "snapshot operation binding mismatch".into(),
            ));
        }
        if pending.confirmed {
            self.pending = Some(pending);
            return Err(FcpError::Protocol("duplicate snapshot received".into()));
        }
        if pending.kind == PendingKind::Enrollment && snapshot.cookies.is_empty() {
            self.pending = Some(pending);
            return Err(FcpError::Protocol(
                "enrollment snapshot must contain at least one cookie".into(),
            ));
        }
        let snapshot_cookie_count = u32::try_from(snapshot.cookies.len())
            .map_err(|_| FcpError::Protocol("snapshot cookie count exceeds u32".into()))?;
        let vault_sequence = if snapshot.cookies.is_empty() {
            self.lease.vault_sequence
        } else if self.transactions.vault_exists() {
            self.transactions.update_after_snapshot(snapshot.cookies)?
        } else {
            self.transactions.enroll(snapshot.cookies)?
        };
        pending.confirmed = true;
        pending.snapshot_cookie_count = Some(snapshot_cookie_count);
        self.lease.vault_sequence = vault_sequence;
        self.lease.pending_operation_id = Some(snapshot.operation_id);
        self.lease_store.persist(&self.lease)?;
        audit.record(
            self.id,
            "vault_snapshot",
            "verified",
            Some(snapshot.operation_id),
            None,
        )?;
        let confirmed = Message::EvictConfirmed(EvictConfirmed {
            account_group_id: self.id,
            lease_id: snapshot.lease_id,
            operation_id: snapshot.operation_id,
            vault_sequence,
            cookie_disposition: if pending.kind == PendingKind::Enrollment {
                CookieDisposition::RetainLeased
            } else {
                CookieDisposition::Remove
            },
        });
        self.pending = Some(pending);
        Ok(vec![confirmed])
    }

    fn handle_evict_result(
        &mut self,
        result: EvictResult,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        let pending = self
            .pending
            .take()
            .ok_or_else(|| FcpError::Protocol("evict result has no pending operation".into()))?;
        if pending.operation_id != result.operation_id
            || pending.lease_id != result.lease_id
            || !pending.confirmed
        {
            self.pending = Some(pending);
            return Err(FcpError::Protocol("evict result operation mismatch".into()));
        }
        let enrollment_retained = pending.kind == PendingKind::Enrollment
            && result.success
            && pending.snapshot_cookie_count == Some(result.remaining_cookie_count)
            && result.remaining_cookie_count > 0;
        if enrollment_retained {
            self.lease.state = GroupState::Leased;
            self.lease.pending_operation_id = None;
            self.lease.advance_transition()?;
            self.lease_store.persist(&self.lease)?;
            audit.record(
                self.id,
                "enrollment",
                "success",
                Some(result.operation_id),
                None,
            )?;
            return Ok(Vec::new());
        }
        if pending.kind != PendingKind::Enrollment
            && result.success
            && result.remaining_cookie_count == 0
        {
            self.lease.state = GroupState::Sealed;
            self.lease.lease_id = None;
            self.lease.granted_at_unix_ms = None;
            self.lease.expires_at_unix_ms = None;
            self.lease.pending_operation_id = None;
            self.lease.advance_transition()?;
            self.lease_store.persist(&self.lease)?;
            let event = match pending.kind {
                PendingKind::Enrollment => unreachable!(),
                PendingKind::Eviction => "eviction",
                PendingKind::Reconciliation => "reconciliation",
            };
            audit.record(self.id, event, "success", Some(result.operation_id), None)?;
            return Ok(Vec::new());
        }
        self.lease.state = GroupState::Degraded;
        self.lease.pending_operation_id = None;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        let (event, detail) = if pending.kind == PendingKind::Enrollment {
            ("enrollment", "retained_cookie_count_mismatch")
        } else {
            ("eviction", "cookies_remain")
        };
        audit.record(
            self.id,
            event,
            "failed",
            Some(result.operation_id),
            Some(detail),
        )?;
        Ok(Vec::new())
    }

    fn lease_deny(&self, reason: &str) -> Message {
        Message::LeaseDeny(LeaseDeny {
            account_group_id: self.id,
            reason: reason.into(),
        })
    }
}

fn message_group_id(message: &Message) -> Option<Uuid> {
    match message {
        Message::LeaseRequest(value) => Some(value.account_group_id),
        Message::InjectResult(value) => Some(value.account_group_id),
        Message::EvictRequest(value) => Some(value.account_group_id),
        Message::CookiesSnapshot(value) => Some(value.account_group_id),
        Message::EvictResult(value) => Some(value.account_group_id),
        Message::SessionInvalidate(value) => Some(value.account_group_id),
        Message::AuthCacheClear(value) => Some(value.account_group_id),
        _ => None,
    }
}

fn inject_failure_code(value: &str) -> &'static str {
    match value {
        "logged_out" => "health_logged_out",
        "invalid_session" => "health_invalid_session",
        "invalid_health_response" => "health_invalid_response",
        "cookie_roundtrip_failed" => "cookie_roundtrip_failed",
        "cookie_disappeared_before_health" => "cookie_disappeared",
        "no_relevant_tab" => "no_relevant_tab",
        "inject_execution_failed" => "inject_execution_failed",
        "cookie_set_failed" => "cookie_set_failed",
        "cookie_set_permission" => "cookie_set_permission",
        "cookie_set_domain" => "cookie_set_domain",
        "cookie_set_samesite" => "cookie_set_samesite",
        "cookie_set_secure" => "cookie_set_secure",
        "cookie_set_path" => "cookie_set_path",
        "cookie_set_partition_key" => "cookie_set_partition_key",
        "cookie_set_store" => "cookie_set_store",
        "cookie_set_url" => "cookie_set_url",
        "cookie_set_invalid_cookie" => "cookie_set_invalid_cookie",
        "cookie_set_no_result" => "cookie_set_no_result",
        "cookie_set_unknown" => "cookie_set_unknown",
        "health_tab_query_failed" => "health_tab_query_failed",
        "health_execution_failed" => "health_execution_failed",
        "inject_cleanup_failed" => "inject_cleanup_failed",
        "navigation_gate_redirect_failed" => "navigation_gate_redirect_failed",
        _ => "health_unrecognized_result",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::protocol::messages::Handshake;

    fn test_paths(root: &std::path::Path) -> DataPaths {
        DataPaths {
            root: root.to_path_buf(),
            vault_groups: root.join("vault/groups"),
            lease_groups: root.join("leases/groups"),
            capability_ledgers: root.join("leases/capabilities"),
            legacy_lease_metadata: root.join("leases/mvp-group.json"),
            legacy_capability_ledger: root.join("leases/capability-ledger.json"),
            account_groups_config: root.join("config/account-groups.json"),
            audit_directory: root.join("audit"),
        }
    }

    #[test]
    fn handshake_reports_both_groups_and_digest() {
        let root = std::env::temp_dir().join(format!("fcp-multi-handshake-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = NativeHostApp::open(&paths).unwrap();
        let output = app
            .handle(Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: PRODUCT_EXTENSION_ID.into(),
                config_digest: app.config_digest.clone(),
            }))
            .unwrap();
        match &output[0] {
            Message::HandshakeAck(ack) => {
                assert_eq!(ack.groups.len(), 2);
                assert_eq!(ack.config_digest.len(), 64);
            }
            _ => panic!("expected handshake ack"),
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn handshake_rejects_config_digest_mismatch_before_any_group_operation() {
        let root = std::env::temp_dir().join(format!("fcp-config-mismatch-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = NativeHostApp::open(&paths).unwrap();
        let result = app.handle(Message::Handshake(Handshake {
            protocol_version: PROTOCOL_VERSION,
            extension_id: PRODUCT_EXTENSION_ID.into(),
            config_digest: "00".repeat(32),
        }));
        assert!(result.is_err());
        assert!(!app.handshake_complete);
        assert!(app.groups.values().all(|group| group.pending.is_none()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn group_pending_operations_are_isolated() {
        let root = std::env::temp_dir().join(format!("fcp-multi-isolation-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let app = NativeHostApp::open(&paths).unwrap();
        assert_eq!(app.groups.len(), 2);
        assert!(app.groups.values().all(|group| group.pending.is_none()));
        let ids: Vec<_> = app.groups.keys().copied().collect();
        assert_ne!(ids[0], ids[1]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalidating_one_group_does_not_touch_the_other_group() {
        let root = std::env::temp_dir().join(format!("fcp-multi-invalidation-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        fs::create_dir_all(&paths.vault_groups).unwrap();
        let loaded = LoadedConfig::load(&paths.account_groups_config).unwrap();
        for group in &loaded.config.groups {
            fs::write(
                paths.vault_groups.join(format!("{}.fcpv", group.id)),
                b"fixture",
            )
            .unwrap();
        }
        let mut app = NativeHostApp::open(&paths).unwrap();
        let target = loaded.config.groups[0].id;
        let other = loaded.config.groups[1].id;
        app.handle(Message::Handshake(Handshake {
            protocol_version: PROTOCOL_VERSION,
            extension_id: PRODUCT_EXTENSION_ID.into(),
            config_digest: app.config_digest.clone(),
        }))
        .unwrap();
        app.handle(Message::SessionInvalidate(SessionInvalidate {
            account_group_id: target,
            lease_id: None,
            reason: SessionInvalidationReason::ExternalLogout,
        }))
        .unwrap();
        assert!(!paths.vault_groups.join(format!("{target}.fcpv")).exists());
        assert!(paths.vault_groups.join(format!("{other}.fcpv")).exists());
        assert_eq!(app.groups[&target].lease.state, GroupState::Uninitialized);
        assert_eq!(app.groups[&other].lease.state, GroupState::Sealed);
        fs::remove_dir_all(root).unwrap();
    }
}
