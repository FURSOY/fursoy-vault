use std::collections::BTreeMap;

use uuid::Uuid;

use sha2::{Digest, Sha256};

use crate::atomic_file::write_verified;
use crate::audit::{AuditLogger, unix_ms};
use crate::config::{
    AccountGroup, AccountGroupsConfig, EvictionTrigger, LoadedConfig, PolicyLevel, StorePolicy,
};
use crate::crypto::hello::HelloAuthorizer;
use crate::lease::metadata::{LeaseMetadata, LeaseMetadataStore};
use crate::lease::store::FileCapabilityLedgerStore;
use crate::monitor::MonitorEngine;
use crate::paths::DataPaths;
use crate::protocol::envelope::PROTOCOL_VERSION;
use crate::protocol::messages::{
    AuthCacheClear, ConfigRejected, ConfigUpdated, CookieDisposition, CookiesInject,
    CookiesSnapshot, EvictConfirmed, EvictPhase, EvictRequest, EvictResult, GroupAdd, GroupRemove,
    GroupSetPolicy, GroupState, Handshake, HandshakeAck, HandshakeGroupState, InjectResult,
    LeaseDeny, LeaseGrant, LeasePurpose, LeaseRequest, Message, SessionInvalidate,
    SessionInvalidated, SessionInvalidationReason,
};
#[cfg(debug_assertions)]
use crate::protocol::messages::{MonitorEvent, MonitorSeverity, MonitorSignal, MonitorSource};
use crate::transaction::VaultTransactions;
use crate::vault::store::VaultStore;
use crate::{FcpError, FcpResult, WIKIPEDIA_ACCOUNT_GROUP_ID};

pub const NATIVE_HOST_NAME: &str = "com.fursoy.vault";
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
    paths: DataPaths,
    config: AccountGroupsConfig,
    config_digest: String,
    handshake_complete: bool,
    last_message_group: Option<Uuid>,
    hello_authorizer: Option<HelloAuthorizer>,
    monitor: MonitorEngine,
}

fn build_group_runtime(paths: &DataPaths, definition: &AccountGroup) -> FcpResult<GroupRuntime> {
    let vault_store = VaultStore::new(&paths.vault_groups);
    let vault_exists = vault_store.path_for(definition.id).exists();
    let capability_store = FileCapabilityLedgerStore::new(paths.capability_path(definition.id));
    let transactions = VaultTransactions::open(definition.id, vault_store, capability_store)?;
    let lease_store = LeaseMetadataStore::new(paths.lease_path(definition.id));
    let lease = lease_store.load_or_initialize(definition.id, vault_exists)?;
    Ok(GroupRuntime {
        id: definition.id,
        policy: definition.policy_level,
        transactions,
        lease_store,
        lease,
        pending: None,
        hello_cache_expires_at: None,
    })
}

impl NativeHostApp {
    pub fn open(paths: &DataPaths) -> FcpResult<Self> {
        let loaded = LoadedConfig::load(&paths.account_groups_config)?;
        paths.migrate_phase5_group(WIKIPEDIA_ACCOUNT_GROUP_ID)?;
        let mut groups = BTreeMap::new();
        for definition in &loaded.config.groups {
            groups.insert(definition.id, build_group_runtime(paths, definition)?);
        }
        let monitor = MonitorEngine::start();
        #[cfg(debug_assertions)]
        if std::env::var_os("FCP_MONITOR_RECONCILIATION_FIXTURE").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            monitor.enqueue_host_event(MonitorEvent {
                event_id: Uuid::new_v4(),
                observed_at_unix_ms: unix_ms()?,
                source: MonitorSource::NativeHost,
                signal: MonitorSignal::ReconciliationFailed,
                severity: MonitorSeverity::High,
                account_group_id: groups.keys().next().copied(),
                occurrence_count: 1,
            })?;
        }
        Ok(Self {
            groups,
            audit: AuditLogger::open(&paths.audit_directory)?,
            paths: paths.clone(),
            config: loaded.config,
            config_digest: loaded.digest,
            handshake_complete: false,
            last_message_group: None,
            hello_authorizer: None,
            monitor,
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
        match message {
            Message::MonitorEvent(event) => {
                if event
                    .account_group_id
                    .is_some_and(|id| !self.groups.contains_key(&id))
                {
                    return Err(FcpError::Protocol("unknown monitor account group".into()));
                }
                return self.monitor.accept_extension_event(event, &self.audit);
            }
            Message::MonitorPoll(request) => return self.monitor.poll(request, &self.audit),
            Message::MonitorAlert(_) => {
                return Err(FcpError::Protocol(
                    "monitor.alert direction is host-to-extension only".into(),
                ));
            }
            // Config mutations are host-wide rather than group-scoped, so they are handled
            // before the per-group routing below.
            Message::GroupAdd(request) => return self.handle_group_add(request),
            Message::GroupRemove(request) => return self.handle_group_remove(request),
            Message::GroupSetPolicy(request) => return self.handle_group_set_policy(request),
            Message::ConfigUpdated(_) | Message::ConfigRejected(_) => {
                return Err(FcpError::Protocol(
                    "config result direction is host-to-extension only".into(),
                ));
            }
            _ => {}
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
        // Q24: the host is the single source of truth for the config, so a stale cached digest
        // on the extension side is not an error — the authoritative config rides along in the ack.
        self.handshake_complete = true;
        let states = self.group_states();
        for group_id in self.groups.keys().copied() {
            self.audit
                .record(group_id, "handshake", "success", None, None)?;
        }
        Ok(vec![Message::HandshakeAck(HandshakeAck {
            protocol_version: PROTOCOL_VERSION,
            config_digest: self.config_digest.clone(),
            config: self.config.clone(),
            groups: states,
        })])
    }

    fn group_states(&self) -> Vec<HandshakeGroupState> {
        self.groups
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
            .collect()
    }

    fn handle_group_add(&mut self, request: GroupAdd) -> FcpResult<Vec<Message>> {
        let scope = request
            .scope
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let display_name = request.display_name.trim().to_owned();
        let display_name = if display_name.is_empty() {
            scope.clone()
        } else {
            display_name
        };
        let mut candidate = self.config.clone();
        candidate.groups.push(AccountGroup {
            id: Uuid::new_v4(),
            display_name,
            scope,
            policy_level: request.policy_level,
            eviction_triggers: vec![
                EvictionTrigger::LastTabClosed,
                EvictionTrigger::Idle,
                EvictionTrigger::Lock,
                EvictionTrigger::Expiry,
                EvictionTrigger::Manual,
            ],
            store_policy: StorePolicy::NormalProfile,
        });
        // Validation is the same routine that guards the on-disk config, so a runtime addition
        // can never produce a config the host would refuse to load on its next start.
        if let Err(error) = candidate.validate() {
            return Ok(vec![Message::ConfigRejected(ConfigRejected {
                reason: config_rejection_code(&error).into(),
            })]);
        }
        let added = candidate
            .groups
            .last()
            .ok_or_else(|| FcpError::Protocol("group addition lost the new group".into()))?
            .clone();
        let runtime = build_group_runtime(&self.paths, &added)?;
        self.commit_config(candidate)?;
        self.groups.insert(added.id, runtime);
        self.audit
            .record(added.id, "config", "success", None, Some("group_added"))?;
        Ok(vec![self.config_updated_message()])
    }

    fn handle_group_remove(&mut self, request: GroupRemove) -> FcpResult<Vec<Message>> {
        let Some(runtime) = self.groups.get_mut(&request.account_group_id) else {
            return Ok(vec![Message::ConfigRejected(ConfigRejected {
                reason: "unknown_group".into(),
            })]);
        };
        if runtime.pending.is_some() {
            return Ok(vec![Message::ConfigRejected(ConfigRejected {
                reason: "operation_pending".into(),
            })]);
        }
        // Dropping protection must not leave the vault behind: discard it under the same
        // fail-closed path used for invalidation before the group stops being tracked.
        runtime.transactions.invalidate()?;
        let mut candidate = self.config.clone();
        candidate
            .groups
            .retain(|group| group.id != request.account_group_id);
        if candidate.groups.is_empty() {
            return Ok(vec![Message::ConfigRejected(ConfigRejected {
                reason: "last_group_cannot_be_removed".into(),
            })]);
        }
        candidate.validate()?;
        self.commit_config(candidate)?;
        self.groups.remove(&request.account_group_id);
        self.audit.record(
            request.account_group_id,
            "config",
            "success",
            None,
            Some("group_removed"),
        )?;
        Ok(vec![self.config_updated_message()])
    }

    fn handle_group_set_policy(&mut self, request: GroupSetPolicy) -> FcpResult<Vec<Message>> {
        match self.groups.get(&request.account_group_id) {
            None => {
                return Ok(vec![Message::ConfigRejected(ConfigRejected {
                    reason: "unknown_group".into(),
                })]);
            }
            Some(runtime) if runtime.pending.is_some() => {
                return Ok(vec![Message::ConfigRejected(ConfigRejected {
                    reason: "operation_pending".into(),
                })]);
            }
            Some(_) => {}
        }
        let mut candidate = self.config.clone();
        let Some(definition) = candidate
            .groups
            .iter_mut()
            .find(|group| group.id == request.account_group_id)
        else {
            return Ok(vec![Message::ConfigRejected(ConfigRejected {
                reason: "unknown_group".into(),
            })]);
        };
        definition.policy_level = request.policy_level;
        candidate.validate()?;
        self.commit_config(candidate)?;
        // The live runtime carries its own copy of the policy, so it is updated in step with the
        // persisted config; otherwise lease durations would keep following the old level.
        if let Some(runtime) = self.groups.get_mut(&request.account_group_id) {
            runtime.policy = request.policy_level;
            runtime.hello_cache_expires_at = None;
        }
        self.audit.record(
            request.account_group_id,
            "config",
            "success",
            None,
            Some("policy_changed"),
        )?;
        Ok(vec![self.config_updated_message()])
    }

    fn commit_config(&mut self, candidate: AccountGroupsConfig) -> FcpResult<()> {
        let bytes = serde_json::to_vec_pretty(&candidate)?;
        // Same write/read-back/replace discipline as the vault: the digest published to the
        // extension is computed from the bytes that are actually on disk.
        write_verified(&self.paths.account_groups_config, &bytes, |persisted| {
            let parsed: AccountGroupsConfig = serde_json::from_slice(persisted)?;
            parsed.validate()
        })?;
        self.config_digest = config_digest_of(&bytes);
        self.config = candidate;
        Ok(())
    }

    fn config_updated_message(&self) -> Message {
        Message::ConfigUpdated(ConfigUpdated {
            config_digest: self.config_digest.clone(),
            config: self.config.clone(),
            groups: self.group_states(),
        })
    }
}

fn config_digest_of(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn config_rejection_code(error: &FcpError) -> &'static str {
    match error {
        FcpError::Format(message) if message.contains("overlap") => "scope_overlaps_existing",
        FcpError::Format(message) if message.contains("scope is invalid") => "scope_invalid",
        FcpError::Format(message) if message.contains("outside bounds") => "group_limit_reached",
        _ => "config_invalid",
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
        // ADR-020: success is a verified cookie round-trip. The host no longer interprets a
        // site-specific health check, so a restored-but-stale session is not an error here; it
        // self-heals when the user logs in again and the next eviction captures fresh cookies.
        if result.success && result.health_check == "restored" {
            audit.record(self.id, "inject", "success", None, None)?;
            return Ok(Vec::new());
        }
        let detail_code = inject_failure_code(&result.health_check);
        audit.record(self.id, "inject", "failed", None, Some(detail_code))?;
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
            SessionInvalidationReason::ScopeEmpty => "scope_empty",
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
        Message::MonitorEvent(value) => value.account_group_id,
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
    use crate::audit::unix_ms;
    use crate::protocol::messages::{
        Handshake, MonitorEvent, MonitorSeverity, MonitorSignal, MonitorSource,
    };

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
            hello_credential: root.join("hello-credential.json"),
        }
    }

    fn handshaken(paths: &DataPaths) -> NativeHostApp {
        let mut app = NativeHostApp::open(paths).unwrap();
        app.handle(Message::Handshake(Handshake {
            protocol_version: PROTOCOL_VERSION,
            extension_id: PRODUCT_EXTENSION_ID.into(),
            cached_config_digest: None,
        }))
        .unwrap();
        app
    }

    #[test]
    fn adding_a_group_persists_config_and_creates_a_runtime() {
        let root = std::env::temp_dir().join(format!("fcp-group-add-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let before = app.config_digest.clone();
        let output = app
            .handle(Message::GroupAdd(GroupAdd {
                scope: "Example.COM".into(),
                display_name: "  ".into(),
                policy_level: PolicyLevel::Balanced,
            }))
            .unwrap();
        match &output[0] {
            Message::ConfigUpdated(updated) => {
                assert_eq!(updated.config.groups.len(), 3);
                assert_ne!(updated.config_digest, before);
                let added = updated
                    .config
                    .groups
                    .iter()
                    .find(|group| group.scope == "example.com")
                    .expect("normalized scope is stored");
                // An empty display name falls back to the scope rather than persisting blank.
                assert_eq!(added.display_name, "example.com");
                assert!(app.groups.contains_key(&added.id));
            }
            other => panic!("expected config.updated, got {other:?}"),
        }
        // The host must be able to reload exactly what it just wrote.
        let reloaded = LoadedConfig::load(&paths.account_groups_config).unwrap();
        assert_eq!(reloaded.config.groups.len(), 3);
        assert_eq!(reloaded.digest, app.config_digest);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overlapping_scope_is_rejected_without_touching_the_live_config() {
        let root = std::env::temp_dir().join(format!("fcp-group-overlap-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let before = app.config_digest.clone();
        let output = app
            .handle(Message::GroupAdd(GroupAdd {
                scope: "tr.wikipedia.org".into(),
                display_name: "nested".into(),
                policy_level: PolicyLevel::Critical,
            }))
            .unwrap();
        match &output[0] {
            Message::ConfigRejected(rejected) => {
                assert_eq!(rejected.reason, "scope_overlaps_existing");
            }
            other => panic!("expected config.rejected, got {other:?}"),
        }
        assert_eq!(app.groups.len(), 2);
        assert_eq!(app.config_digest, before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_a_group_drops_its_runtime_and_keeps_the_others() {
        let root = std::env::temp_dir().join(format!("fcp-group-remove-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let victim = *app.groups.keys().next().unwrap();
        let survivor = *app.groups.keys().nth(1).unwrap();
        let output = app
            .handle(Message::GroupRemove(GroupRemove {
                account_group_id: victim,
            }))
            .unwrap();
        match &output[0] {
            Message::ConfigUpdated(updated) => assert_eq!(updated.config.groups.len(), 1),
            other => panic!("expected config.updated, got {other:?}"),
        }
        assert!(!app.groups.contains_key(&victim));
        assert!(app.groups.contains_key(&survivor));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changing_policy_persists_and_updates_the_live_runtime() {
        let root = std::env::temp_dir().join(format!("fcp-group-policy-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let target = *app.groups.keys().next().unwrap();
        let before = app.groups[&target].policy;
        let next = if before == PolicyLevel::Critical {
            PolicyLevel::Convenient
        } else {
            PolicyLevel::Critical
        };
        let output = app
            .handle(Message::GroupSetPolicy(GroupSetPolicy {
                account_group_id: target,
                policy_level: next,
            }))
            .unwrap();
        match &output[0] {
            Message::ConfigUpdated(updated) => {
                let definition = updated
                    .config
                    .groups
                    .iter()
                    .find(|group| group.id == target)
                    .unwrap();
                assert_eq!(definition.policy_level, next);
            }
            other => panic!("expected config.updated, got {other:?}"),
        }
        // The runtime must not keep enforcing the old lease durations.
        assert_eq!(app.groups[&target].policy, next);
        let reloaded = LoadedConfig::load(&paths.account_groups_config).unwrap();
        assert_eq!(
            reloaded
                .config
                .groups
                .iter()
                .find(|group| group.id == target)
                .unwrap()
                .policy_level,
            next
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_the_last_group_is_refused() {
        let root = std::env::temp_dir().join(format!("fcp-group-last-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let ids: Vec<_> = app.groups.keys().copied().collect();
        app.handle(Message::GroupRemove(GroupRemove {
            account_group_id: ids[0],
        }))
        .unwrap();
        let output = app
            .handle(Message::GroupRemove(GroupRemove {
                account_group_id: ids[1],
            }))
            .unwrap();
        match &output[0] {
            Message::ConfigRejected(rejected) => {
                assert_eq!(rejected.reason, "last_group_cannot_be_removed");
            }
            other => panic!("expected config.rejected, got {other:?}"),
        }
        assert_eq!(app.groups.len(), 1);
        fs::remove_dir_all(root).unwrap();
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
                cached_config_digest: Some(app.config_digest.clone()),
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
    fn stale_extension_cache_is_answered_with_the_authoritative_config() {
        let root = std::env::temp_dir().join(format!("fcp-config-refresh-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = NativeHostApp::open(&paths).unwrap();
        let output = app
            .handle(Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: PRODUCT_EXTENSION_ID.into(),
                cached_config_digest: Some("00".repeat(32)),
            }))
            .unwrap();
        match &output[0] {
            Message::HandshakeAck(ack) => {
                assert_eq!(ack.config_digest, app.config_digest);
                assert_eq!(ack.config.groups.len(), 2);
            }
            _ => panic!("expected handshake ack"),
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn first_run_without_a_cached_digest_is_accepted() {
        let root = std::env::temp_dir().join(format!("fcp-config-firstrun-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = NativeHostApp::open(&paths).unwrap();
        let output = app
            .handle(Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: PRODUCT_EXTENSION_ID.into(),
                cached_config_digest: None,
            }))
            .unwrap();
        assert!(matches!(&output[0], Message::HandshakeAck(_)));
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
    fn monitoring_event_for_one_group_does_not_mutate_any_group_runtime() {
        let root = std::env::temp_dir().join(format!("fcp-monitor-isolation-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = NativeHostApp::open(&paths).unwrap();
        app.handle(Message::Handshake(Handshake {
            protocol_version: PROTOCOL_VERSION,
            extension_id: PRODUCT_EXTENSION_ID.into(),
            cached_config_digest: Some(app.config_digest.clone()),
        }))
        .unwrap();
        let target = *app.groups.keys().next().unwrap();
        let before: Vec<_> = app
            .groups
            .iter()
            .map(|(id, group)| {
                (
                    *id,
                    group.lease.state.clone(),
                    group.lease.transition_sequence,
                )
            })
            .collect();
        let output = app
            .handle(Message::MonitorEvent(MonitorEvent {
                event_id: Uuid::new_v4(),
                observed_at_unix_ms: unix_ms().unwrap(),
                source: MonitorSource::Extension,
                signal: MonitorSignal::SelectorChanged,
                severity: MonitorSeverity::Info,
                account_group_id: Some(target),
                occurrence_count: 1,
            }))
            .unwrap();
        assert!(matches!(output.as_slice(), [Message::MonitorAlert(_)]));
        let after: Vec<_> = app
            .groups
            .iter()
            .map(|(id, group)| {
                (
                    *id,
                    group.lease.state.clone(),
                    group.lease.transition_sequence,
                )
            })
            .collect();
        assert_eq!(before, after);
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
            cached_config_digest: Some(app.config_digest.clone()),
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
