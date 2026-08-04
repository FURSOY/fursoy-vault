use uuid::Uuid;

use crate::audit::{AuditLogger, unix_ms};
use crate::lease::metadata::{LeaseMetadata, LeaseMetadataStore};
use crate::lease::store::FileCapabilityLedgerStore;
use crate::paths::DataPaths;
use crate::protocol::envelope::PROTOCOL_VERSION;
use crate::protocol::messages::{
    CookieDisposition, CookiesInject, CookiesSnapshot, EvictConfirmed, EvictPhase, EvictRequest,
    EvictResult, GroupState, Handshake, HandshakeAck, InjectResult, LeaseDeny, LeaseGrant,
    LeasePurpose, LeaseRequest, Message, SessionInvalidate, SessionInvalidated,
    SessionInvalidationReason,
};
use crate::transaction::VaultTransactions;
use crate::vault::store::VaultStore;
use crate::{FcpError, FcpResult, MVP_ACCOUNT_GROUP_ID};

pub const NATIVE_HOST_NAME: &str = "com.fursoy.cookie_protector";
pub const PRODUCT_EXTENSION_ID: &str = "ikodegbaomnahbjiokfogpedaoifhbde";
const MAX_LEASE_DURATION_MS: u64 = 15 * 60 * 1_000;

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

pub struct NativeHostApp {
    transactions: VaultTransactions,
    lease_store: LeaseMetadataStore,
    lease: LeaseMetadata,
    audit: AuditLogger,
    handshake_complete: bool,
    pending: Option<PendingOperation>,
}

impl NativeHostApp {
    pub fn open(paths: &DataPaths) -> FcpResult<Self> {
        let vault_store = VaultStore::new(&paths.vault_groups);
        let vault_exists = vault_store.path_for(MVP_ACCOUNT_GROUP_ID).exists();
        let capability_store = FileCapabilityLedgerStore::new(&paths.capability_ledger);
        let transactions =
            VaultTransactions::open(MVP_ACCOUNT_GROUP_ID, vault_store, capability_store)?;
        let lease_store = LeaseMetadataStore::new(&paths.lease_metadata);
        let lease = lease_store.load_or_initialize(MVP_ACCOUNT_GROUP_ID, vault_exists)?;
        Ok(Self {
            transactions,
            lease_store,
            lease,
            audit: AuditLogger::new(&paths.audit_directory),
            handshake_complete: false,
            pending: None,
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

        match message {
            Message::Handshake(_) | Message::HandshakeAck(_) => Err(FcpError::Protocol(
                "handshake cannot be repeated on one connection".into(),
            )),
            Message::LeaseRequest(request) => self.handle_lease_request(request),
            Message::InjectResult(result) => self.handle_inject_result(result),
            Message::EvictRequest(request) => self.handle_evict_begin(request),
            Message::CookiesSnapshot(snapshot) => self.handle_snapshot(snapshot),
            Message::EvictResult(result) => self.handle_evict_result(result),
            Message::SessionInvalidate(request) => self.handle_session_invalidate(request),
            Message::LeaseGrant(_)
            | Message::LeaseDeny(_)
            | Message::CookiesInject(_)
            | Message::EvictConfirmed(_)
            | Message::SessionInvalidated(_) => Err(FcpError::Protocol(
                "message direction is host-to-extension only".into(),
            )),
        }
    }

    pub fn deny_for_error(&self, _error: &FcpError) -> Message {
        Message::LeaseDeny(LeaseDeny {
            account_group_id: MVP_ACCOUNT_GROUP_ID,
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
        self.handshake_complete = true;
        let group_state = self.lease.state.clone();
        let reconciliation_required = matches!(
            group_state,
            GroupState::Unlocking | GroupState::Evicting | GroupState::Degraded
        );
        let output = vec![Message::HandshakeAck(HandshakeAck {
            protocol_version: PROTOCOL_VERSION,
            account_group_id: MVP_ACCOUNT_GROUP_ID,
            group_state,
            reconciliation_required,
            lease_id: self.lease.lease_id,
            lease_expiry_unix_ms: self.lease.expires_at_unix_ms,
        })];
        self.audit.record("handshake", "success", None, None)?;
        Ok(output)
    }

    fn handle_lease_request(&mut self, request: LeaseRequest) -> FcpResult<Vec<Message>> {
        self.require_group(request.account_group_id)?;
        if self.pending.is_some() {
            return Ok(vec![self.lease_deny("operation_pending")]);
        }
        if request.requested_duration_ms == 0
            || request.requested_duration_ms > MAX_LEASE_DURATION_MS
        {
            return Ok(vec![self.lease_deny("invalid_lease_duration")]);
        }

        match request.purpose {
            LeasePurpose::Enroll => self.begin_enrollment(request.requested_duration_ms),
            LeasePurpose::Inject => self.begin_inject(request.requested_duration_ms),
        }
    }

    fn begin_enrollment(&mut self, duration_ms: u64) -> FcpResult<Vec<Message>> {
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
        self.audit
            .record("enrollment", "started", Some(operation_id), None)?;
        Ok(vec![
            Message::LeaseGrant(LeaseGrant {
                account_group_id: MVP_ACCOUNT_GROUP_ID,
                lease_id,
                expiry_unix_ms: expiry,
                capability_sequence: None,
            }),
            Message::EvictRequest(EvictRequest {
                account_group_id: MVP_ACCOUNT_GROUP_ID,
                lease_id: Some(lease_id),
                operation_id,
                phase: EvictPhase::SnapshotRequired,
                reason: "initial_enrollment".into(),
            }),
        ])
    }

    fn begin_inject(&mut self, duration_ms: u64) -> FcpResult<Vec<Message>> {
        if self.lease.state != GroupState::Sealed || !self.transactions.vault_exists() {
            return Ok(vec![self.lease_deny("group_not_sealed")]);
        }
        let authorization = self.transactions.authorize_inject()?;
        let capability_sequence = authorization.monotonic_sequence();
        // Security ordering: verify_and_consume persisted the capability before this call can
        // unwrap the TPM-bound DEK. read_for_inject consumes the linear token by value.
        let payload = self.transactions.read_for_inject(authorization)?;
        let now = unix_ms()?;
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
        // Persist open-lease metadata before cookie plaintext leaves the host.
        self.lease_store.persist(&self.lease)?;
        self.audit.record("inject", "authorized", None, None)?;
        Ok(vec![
            Message::LeaseGrant(LeaseGrant {
                account_group_id: MVP_ACCOUNT_GROUP_ID,
                lease_id,
                expiry_unix_ms: expiry,
                capability_sequence: Some(capability_sequence),
            }),
            Message::CookiesInject(CookiesInject {
                account_group_id: MVP_ACCOUNT_GROUP_ID,
                lease_id,
                cookies: payload.cookies,
            }),
        ])
    }

    fn handle_inject_result(&mut self, result: InjectResult) -> FcpResult<Vec<Message>> {
        self.require_group(result.account_group_id)?;
        if self.lease.state != GroupState::Leased || self.lease.lease_id != Some(result.lease_id) {
            return Err(FcpError::Protocol("inject result lease mismatch".into()));
        }
        if result.success && result.health_check == "authenticated" {
            self.audit.record("inject", "success", None, None)?;
            return Ok(Vec::new());
        }
        let detail_code = match result.health_check.as_str() {
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
            _ => "health_unrecognized_result",
        };
        self.audit
            .record("inject", "failed", None, Some(detail_code))?;
        if matches!(
            result.health_check.as_str(),
            "logged_out" | "invalid_session"
        ) {
            return self.invalidate_session(SessionInvalidationReason::RestoreRejected);
        }
        self.lease.state = GroupState::Degraded;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        Ok(Vec::new())
    }

    fn handle_session_invalidate(&mut self, request: SessionInvalidate) -> FcpResult<Vec<Message>> {
        self.require_group(request.account_group_id)?;
        if self.pending.is_some() {
            return Err(FcpError::Protocol(
                "session invalidation refused while an operation is pending".into(),
            ));
        }
        if self.lease.state == GroupState::Uninitialized {
            return Ok(vec![Message::SessionInvalidated(SessionInvalidated {
                account_group_id: MVP_ACCOUNT_GROUP_ID,
                reason: request.reason,
            })]);
        }
        if request.lease_id.is_some() && request.lease_id != self.lease.lease_id {
            return Err(FcpError::Protocol(
                "session invalidation lease mismatch".into(),
            ));
        }
        self.invalidate_session(request.reason)
    }

    fn invalidate_session(&mut self, reason: SessionInvalidationReason) -> FcpResult<Vec<Message>> {
        // Deleting the encrypted payload is authoritative. Startup repairs lease metadata to
        // Uninitialized if the process dies between this deletion and the metadata write.
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
        self.audit
            .record("session_invalidation", "success", None, Some(detail))?;
        Ok(vec![Message::SessionInvalidated(SessionInvalidated {
            account_group_id: MVP_ACCOUNT_GROUP_ID,
            reason,
        })])
    }

    fn handle_evict_begin(&mut self, request: EvictRequest) -> FcpResult<Vec<Message>> {
        self.require_group(request.account_group_id)?;
        if request.phase != EvictPhase::Begin {
            return Err(FcpError::Protocol(
                "extension may send only evict.request phase=begin".into(),
            ));
        }
        if self.pending.is_some() {
            return Err(FcpError::Protocol("another operation is pending".into()));
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
                "eviction request is not valid for current lease state".into(),
            ));
        }
        // Eviction is deliberately non-interactive. Waiting for Hello here would make idle/lock
        // fail open precisely when the user is absent. TPM unwrap remains transaction-scoped.
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
        self.audit.record(
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
            account_group_id: MVP_ACCOUNT_GROUP_ID,
            lease_id: request.lease_id,
            operation_id: request.operation_id,
            phase: EvictPhase::SnapshotRequired,
            reason: request.reason,
        })])
    }

    fn handle_snapshot(&mut self, snapshot: CookiesSnapshot) -> FcpResult<Vec<Message>> {
        self.require_group(snapshot.account_group_id)?;
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
            // Nothing is exposed; stale/open lease metadata can close without decrypting the vault.
            self.lease.vault_sequence
        } else {
            if self.transactions.vault_exists() {
                self.transactions.update_after_snapshot(snapshot.cookies)?
            } else {
                self.transactions.enroll(snapshot.cookies)?
            }
        };

        pending.confirmed = true;
        pending.snapshot_cookie_count = Some(snapshot_cookie_count);
        self.lease.vault_sequence = vault_sequence;
        self.lease.pending_operation_id = Some(snapshot.operation_id);
        self.lease_store.persist(&self.lease)?;
        self.audit.record(
            "vault_snapshot",
            "verified",
            Some(snapshot.operation_id),
            None,
        )?;
        let confirmed = Message::EvictConfirmed(EvictConfirmed {
            account_group_id: MVP_ACCOUNT_GROUP_ID,
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

    fn handle_evict_result(&mut self, result: EvictResult) -> FcpResult<Vec<Message>> {
        self.require_group(result.account_group_id)?;
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
            self.audit
                .record("enrollment", "success", Some(result.operation_id), None)?;
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
                PendingKind::Enrollment => unreachable!("enrollment handled above"),
                PendingKind::Eviction => "eviction",
                PendingKind::Reconciliation => "reconciliation",
            };
            self.audit
                .record(event, "success", Some(result.operation_id), None)?;
            return Ok(Vec::new());
        }

        self.lease.state = GroupState::Degraded;
        self.lease.pending_operation_id = None;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        let (event, detail_code) = if pending.kind == PendingKind::Enrollment {
            ("enrollment", "retained_cookie_count_mismatch")
        } else {
            ("eviction", "cookies_remain")
        };
        self.audit.record(
            event,
            "failed",
            Some(result.operation_id),
            Some(detail_code),
        )?;
        Ok(Vec::new())
    }

    fn require_group(&self, group_id: Uuid) -> FcpResult<()> {
        if group_id != MVP_ACCOUNT_GROUP_ID {
            return Err(FcpError::Protocol("account group mismatch".into()));
        }
        Ok(())
    }

    fn lease_deny(&self, reason: &str) -> Message {
        Message::LeaseDeny(LeaseDeny {
            account_group_id: MVP_ACCOUNT_GROUP_ID,
            reason: reason.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn app_with_stale_vault() -> (NativeHostApp, DataPaths) {
        let root = std::env::temp_dir().join(format!("fcp-dispatcher-test-{}", Uuid::new_v4()));
        let paths = DataPaths {
            root: root.clone(),
            vault_groups: root.join("vault").join("groups"),
            lease_metadata: root.join("leases").join("mvp-group.json"),
            capability_ledger: root.join("leases").join("capability-ledger.json"),
            audit_directory: root.join("audit"),
        };
        fs::create_dir_all(&paths.vault_groups).unwrap();
        fs::write(
            paths
                .vault_groups
                .join(format!("{MVP_ACCOUNT_GROUP_ID}.fcpv")),
            b"encrypted-vault-fixture",
        )
        .unwrap();
        let mut app = NativeHostApp::open(&paths).unwrap();
        app.handle(Message::Handshake(Handshake {
            protocol_version: PROTOCOL_VERSION,
            extension_id: PRODUCT_EXTENSION_ID.into(),
        }))
        .unwrap();
        (app, paths)
    }

    #[test]
    fn external_logout_deletes_vault_and_resets_group() {
        let (mut app, paths) = app_with_stale_vault();
        let response = app
            .handle(Message::SessionInvalidate(SessionInvalidate {
                account_group_id: MVP_ACCOUNT_GROUP_ID,
                lease_id: None,
                reason: SessionInvalidationReason::ExternalLogout,
            }))
            .unwrap();
        assert!(matches!(
            response.as_slice(),
            [Message::SessionInvalidated(SessionInvalidated {
                reason: SessionInvalidationReason::ExternalLogout,
                ..
            })]
        ));
        assert_eq!(app.lease.state, GroupState::Uninitialized);
        assert!(!app.transactions.vault_exists());
        fs::remove_dir_all(paths.root).unwrap();
    }

    #[test]
    fn rejected_restore_invalidates_instead_of_remaining_degraded() {
        let (mut app, paths) = app_with_stale_vault();
        let lease_id = Uuid::new_v4();
        app.lease.state = GroupState::Leased;
        app.lease.lease_id = Some(lease_id);
        app.lease_store.persist(&app.lease).unwrap();

        let response = app
            .handle(Message::InjectResult(InjectResult {
                account_group_id: MVP_ACCOUNT_GROUP_ID,
                lease_id,
                success: false,
                health_check: "logged_out".into(),
            }))
            .unwrap();
        assert!(matches!(
            response.as_slice(),
            [Message::SessionInvalidated(SessionInvalidated {
                reason: SessionInvalidationReason::RestoreRejected,
                ..
            })]
        ));
        assert_eq!(app.lease.state, GroupState::Uninitialized);
        assert!(!app.transactions.vault_exists());
        fs::remove_dir_all(paths.root).unwrap();
    }
}
