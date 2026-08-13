use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use uuid::Uuid;

use sha2::{Digest, Sha256};

use crate::atomic_file::write_verified;
use crate::audit::{AuditLogger, unix_ms};
use crate::config::{AccountGroup, AccountGroupsConfig, LoadedConfig, PolicyLevel, StorePolicy};
use crate::crypto::fill_random;
use crate::crypto::hello::HelloAuthorizer;
use crate::lease::metadata::{LeaseMetadata, LeaseMetadataStore};
use crate::lease::store::FileCapabilityLedgerStore;
use crate::monitor::MonitorEngine;
use crate::operation::{OperationCoordinator, OperationKind, StartupRecovery};
use crate::paths::DataPaths;
use crate::protocol::envelope::{LEGACY_PROTOCOL_VERSION, PROTOCOL_VERSION};
use crate::protocol::messages::{
    AuthCacheClear, CapabilityOperation, CapabilityPayload, ConfigRejected, ConfigUpdated,
    CookieDisposition, CookieRecord, CookiesInjectChunk, CookiesSnapshotChunk,
    CookiesSnapshotChunkV7, EvictConfirmed, EvictPhase, EvictRemoveAuthorized, EvictRemovePrepare,
    EvictRequest, EvictResult, EvictResultV7, GroupAdd, GroupRemove, GroupSetPolicy, GroupState,
    Handshake, HandshakeAck, HandshakeGroupState, InjectResult, LeaseDeny, LeaseGrant,
    LeasePurpose, LeaseRequest, Message, Nonce32, OperationBegin, OperationCompleted,
    OperationKindWire, OperationRequiredAction, OperationSnapshotRequired, OperationStatus,
    OperationStatusQuery, RecoveryAdopt, RecoveryAdopted, RecoveryCandidate, SessionInvalidate,
    SessionInvalidated, SessionInvalidationReason,
};
use crate::protocol::messages::{MonitorEvent, MonitorSeverity, MonitorSignal, MonitorSource};
use crate::transaction::VaultTransactions;
use crate::vault::store::VaultStore;
use crate::{FcpError, FcpResult, WIKIPEDIA_ACCOUNT_GROUP_ID};

pub const NATIVE_HOST_NAME: &str = "com.fursoy.vault";
pub const PRODUCT_EXTENSION_ID: &str = "ibjddphkjppgkdbegjibddbjkagdlaea";
const MIN_EXTENSION_VERSION: &str = "0.4.1";
const PROTOCOL_CAPABILITIES: &[&str] = &[
    "chunked_cookies",
    "request_correlation",
    "config_v3",
    "audit_recovery",
    "profile_namespace",
    "durable_operations_v7",
    "guarded_cookie_removal",
    "semantic_operation_status",
    "profile_recovery_v1",
];
const V7_REQUIRED_CAPABILITIES: &[&str] = &[
    "chunked_cookies",
    "request_correlation",
    "config_v3",
    "audit_recovery",
    "profile_namespace",
    "durable_operations_v7",
    "guarded_cookie_removal",
    "semantic_operation_status",
];
const V6_PROTOCOL_CAPABILITIES: &[&str] = &[
    "chunked_cookies",
    "request_correlation",
    "config_v3",
    "audit_recovery",
    "profile_namespace",
];

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
    snapshot_chunk_count: Option<u32>,
    snapshot_next_chunk: u32,
    snapshot_cookies: Vec<CookieRecord>,
    snapshot_bytes: usize,
    operation_sequence: u64,
    attempt_id: Option<Uuid>,
    snapshot_chunk_digests: BTreeMap<u32, [u8; 32]>,
}

const COOKIE_CHUNK_TARGET_BYTES: usize = 400 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;
const MAX_SNAPSHOT_COOKIES: u32 = 100_000;

struct GroupRuntime {
    id: Uuid,
    policy: PolicyLevel,
    transactions: VaultTransactions,
    lease_store: LeaseMetadataStore,
    lease: LeaseMetadata,
    operation_coordinator: OperationCoordinator,
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
    negotiated_protocol: u16,
    connection_profile_id: Option<Uuid>,
}

fn build_group_runtime(paths: &DataPaths, definition: &AccountGroup) -> FcpResult<GroupRuntime> {
    let vault_store = VaultStore::new(&paths.vault_groups);
    let vault_exists = vault_store.path_for(definition.id).exists();
    let capability_store = FileCapabilityLedgerStore::new(paths.capability_path(definition.id));
    let transactions = VaultTransactions::open(definition.id, vault_store, capability_store)?;
    let lease_store = LeaseMetadataStore::new(paths.lease_path(definition.id));
    let mut lease = lease_store.load_or_initialize(definition.id, vault_exists)?;
    let mut operation_coordinator = OperationCoordinator::open(paths, definition.id)?;
    let startup_recovery = operation_coordinator.startup_recover(&transactions.vault_path())?;
    repair_lease_v1_projection(
        &mut lease,
        &lease_store,
        &operation_coordinator,
        &startup_recovery,
        vault_exists,
    )?;
    Ok(GroupRuntime {
        id: definition.id,
        policy: definition.policy_level,
        transactions,
        lease_store,
        lease,
        operation_coordinator,
        pending: None,
        hello_cache_expires_at: None,
    })
}

fn repair_lease_v1_projection(
    lease: &mut LeaseMetadata,
    lease_store: &LeaseMetadataStore,
    coordinator: &OperationCoordinator,
    recovery: &StartupRecovery,
    vault_exists: bool,
) -> FcpResult<()> {
    if let Some(operation_id) = lease.pending_operation_id
        && let Some(completed) = coordinator.completed_projection(operation_id)
    {
        match completed.kind {
            OperationKind::Enrollment => {
                lease.state = GroupState::Leased;
                lease.lease_id = completed.lease_id;
            }
            OperationKind::Eviction | OperationKind::Reconciliation => {
                lease.state = GroupState::Sealed;
                lease.lease_id = None;
                lease.granted_at_unix_ms = None;
                lease.expires_at_unix_ms = None;
            }
        }
        lease.pending_operation_id = None;
        if let Some(snapshot) = completed.snapshot {
            lease.vault_sequence = snapshot.target_vault_sequence;
        }
        lease.advance_transition()?;
        lease_store.persist(lease)?;
        return Ok(());
    }
    if recovery.needs_reconciliation && (lease.state != GroupState::Uninitialized || vault_exists) {
        lease.state = GroupState::Degraded;
        lease.pending_operation_id = recovery.committed_operation_id;
        if let Some(sequence) = recovery.committed_vault_sequence {
            lease.vault_sequence = sequence;
        }
        lease.advance_transition()?;
        lease_store.persist(lease)?;
    }
    Ok(())
}

impl NativeHostApp {
    pub fn open(paths: &DataPaths) -> FcpResult<Self> {
        let loaded = LoadedConfig::load(&paths.account_groups_config)?;
        paths.migrate_phase5_group(WIKIPEDIA_ACCOUNT_GROUP_ID)?;
        let configured = loaded
            .config
            .groups
            .iter()
            .filter(|group| group.policy_level != PolicyLevel::Monitor)
            .map(|group| group.id)
            .collect::<HashSet<_>>();
        VaultStore::new(&paths.vault_groups).recover_staged_deletions(&configured)?;
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
        let audit = AuditLogger::open(&paths.audit_directory)?;
        if audit.recovered_on_open() {
            monitor.enqueue_host_event(MonitorEvent {
                event_id: Uuid::new_v4(),
                observed_at_unix_ms: unix_ms()?,
                source: MonitorSource::NativeHost,
                signal: MonitorSignal::AuditIntegrityRecovered,
                severity: MonitorSeverity::High,
                account_group_id: None,
                occurrence_count: 1,
            })?;
        }
        Ok(Self {
            groups,
            audit,
            paths: paths.clone(),
            config: loaded.config,
            config_digest: loaded.digest,
            handshake_complete: false,
            last_message_group: None,
            hello_authorizer: None,
            monitor,
            negotiated_protocol: LEGACY_PROTOCOL_VERSION,
            connection_profile_id: None,
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
        self.last_message_group = None;
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
            Message::RecoveryAdopt(request) => return self.handle_recovery_adopt(request),
            Message::RecoveryAdopted(_) => {
                return Err(FcpError::Protocol(
                    "recovery result direction is host-to-extension only".into(),
                ));
            }
            Message::ConfigUpdated(_) | Message::ConfigRejected(_) | Message::OperationError(_) => {
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
        let hello_credential = self.paths.hello_credential.clone();
        let runtime = self
            .groups
            .get_mut(&group_id)
            .ok_or_else(|| FcpError::Protocol("unknown account group".into()))?;
        runtime.handle(
            message,
            &self.audit,
            &mut self.hello_authorizer,
            &hello_credential,
            self.negotiated_protocol,
        )
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

    pub fn error_group_id(&self) -> Option<Uuid> {
        self.last_message_group
    }

    fn handle_handshake(&mut self, handshake: Handshake) -> FcpResult<Vec<Message>> {
        let extension_build_detail = if handshake.extension_version == env!("CARGO_PKG_VERSION") {
            "extension_current"
        } else {
            "extension_legacy"
        };
        if !matches!(
            handshake.protocol_version,
            LEGACY_PROTOCOL_VERSION | PROTOCOL_VERSION
        ) {
            return Err(FcpError::Protocol(
                "handshake protocol version mismatch".into(),
            ));
        }
        if handshake.extension_id != PRODUCT_EXTENSION_ID {
            return Err(FcpError::Protocol("unexpected extension id".into()));
        }
        if compare_versions(&handshake.extension_version, MIN_EXTENSION_VERSION)
            .is_none_or(|ordering| ordering.is_lt())
        {
            return Err(FcpError::Protocol(
                "extension version is below host minimum".into(),
            ));
        }
        if compare_versions(env!("CARGO_PKG_VERSION"), &handshake.min_host_version)
            .is_none_or(|ordering| ordering.is_lt())
        {
            return Err(FcpError::Protocol(
                "host version is below extension minimum".into(),
            ));
        }
        let required_capabilities = if handshake.protocol_version == PROTOCOL_VERSION {
            V7_REQUIRED_CAPABILITIES
        } else {
            V6_PROTOCOL_CAPABILITIES
        };
        if !required_capabilities.iter().all(|required| {
            handshake
                .capabilities
                .iter()
                .any(|offered| offered == required)
        }) {
            return Err(FcpError::Protocol("extension capability mismatch".into()));
        }
        for runtime in self.groups.values_mut() {
            if handshake.protocol_version == LEGACY_PROTOCOL_VERSION
                && runtime.lease.protocol_floor >= PROTOCOL_VERSION
            {
                return Err(FcpError::Protocol(
                    "upgrade_required: profile protocol floor is v7".into(),
                ));
            }
            runtime
                .operation_coordinator
                .set_profile_id(handshake.profile_id)?;
            if handshake.protocol_version == PROTOCOL_VERSION
                && runtime
                    .lease
                    .activate_protocol_v7(runtime.operation_coordinator.sequence_high_water())?
            {
                runtime.lease.advance_transition()?;
                runtime.lease_store.persist(&runtime.lease)?;
            }
        }
        // Q24: the host is the single source of truth for the config, so a stale cached digest
        // on the extension side is not an error — the authoritative config rides along in the ack.
        self.handshake_complete = true;
        self.negotiated_protocol = handshake.protocol_version;
        self.connection_profile_id = Some(handshake.profile_id);
        let recovery_candidates = if handshake.protocol_version == PROTOCOL_VERSION {
            self.recovery_candidates(handshake.profile_id)?
        } else {
            Vec::new()
        };
        let states = self.group_states();
        for group_id in self.groups.keys().copied() {
            self.audit.record(
                group_id,
                "handshake",
                "success",
                None,
                Some(extension_build_detail),
            )?;
        }
        Ok(vec![Message::HandshakeAck(HandshakeAck {
            protocol_version: handshake.protocol_version,
            config_digest: self.config_digest.clone(),
            config: self.config.clone(),
            groups: states,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            min_extension_version: MIN_EXTENSION_VERSION.into(),
            capabilities: if handshake.protocol_version == PROTOCOL_VERSION {
                PROTOCOL_CAPABILITIES
            } else {
                V6_PROTOCOL_CAPABILITIES
            }
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
            recovery_candidates,
        })])
    }

    fn recovery_candidates(&self, current_profile_id: Uuid) -> FcpResult<Vec<RecoveryCandidate>> {
        // Recovery is offered only to an empty, newly-created namespace. Once this profile owns
        // config, switching identity could hide active protection state and is rejected.
        if !self.config.groups.is_empty() {
            return Ok(Vec::new());
        }
        let Ok(mut roots) = self.paths.sibling_profile_roots() else {
            return Ok(Vec::new());
        };
        roots.sort_by_key(|(_, root)| profile_created_unix_ms(root));
        let mut candidates = Vec::new();
        for (index, (profile_id, root)) in roots.into_iter().enumerate() {
            if profile_id == current_profile_id {
                continue;
            }
            let candidate_paths = DataPaths::from_root(root.clone());
            if !candidate_paths.account_groups_config.is_file()
                || !candidate_paths.hello_credential.is_file()
            {
                continue;
            }
            let Ok(loaded) = LoadedConfig::load(&candidate_paths.account_groups_config) else {
                continue;
            };
            if loaded.config.groups.is_empty() {
                continue;
            }
            candidates.push(RecoveryCandidate {
                profile_id,
                display_name: format!("Chrome Profile {}", index + 1),
                browser: "Chrome".into(),
                last_used_unix_ms: profile_last_used_unix_ms(&root),
                site_count: loaded.config.groups.len() as u32,
            });
        }
        candidates.sort_by(|left, right| {
            right
                .last_used_unix_ms
                .cmp(&left.last_used_unix_ms)
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        Ok(candidates)
    }

    fn handle_recovery_adopt(&mut self, request: RecoveryAdopt) -> FcpResult<Vec<Message>> {
        if self.negotiated_protocol != PROTOCOL_VERSION || !self.config.groups.is_empty() {
            return Err(FcpError::Protocol(
                "profile recovery requires an empty v7 profile".into(),
            ));
        }
        let current_profile_id = self.connection_profile_id.ok_or_else(|| {
            FcpError::Protocol("profile recovery requires an authenticated profile".into())
        })?;
        if !self
            .recovery_candidates(current_profile_id)?
            .iter()
            .any(|candidate| candidate.profile_id == request.profile_id)
        {
            return Err(FcpError::Protocol("unknown recovery candidate".into()));
        }
        let candidate_paths = self.paths.sibling_profile(request.profile_id)?;
        let authorizer = HelloAuthorizer::open_existing(&candidate_paths.hello_credential)?;
        let now = unix_ms()?;
        let mut nonce = [0u8; 32];
        fill_random(&mut nonce)?;
        // The signed capability is deliberately not sent to the extension. Successful signing
        // is the local proof; only the selected opaque profile id crosses the protocol boundary.
        authorizer.sign_fresh(CapabilityPayload {
            account_group_id: request.profile_id,
            operation: CapabilityOperation::RecoverProfile,
            expiry_unix_ms: now.saturating_add(60_000),
            monotonic_sequence: now.max(1),
            nonce: Nonce32(nonce),
        })?;
        Ok(vec![Message::RecoveryAdopted(RecoveryAdopted {
            profile_id: request.profile_id,
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
        let mut runtime = build_group_runtime(&self.paths, &added)?;
        // Groups created after the handshake did not pass through handle_handshake's runtime
        // loop. They must inherit the authenticated profile binding and negotiated protocol floor
        // before accepting any v7 snapshot; otherwise keyed snapshot integrity fails closed and
        // leaves a freshly-added site permanently NotCommitted/Evicting.
        let profile_id = self.connection_profile_id.ok_or_else(|| {
            FcpError::Protocol("group addition requires an authenticated profile".into())
        })?;
        runtime.operation_coordinator.set_profile_id(profile_id)?;
        if self.negotiated_protocol == PROTOCOL_VERSION
            && runtime
                .lease
                .activate_protocol_v7(runtime.operation_coordinator.sequence_high_water())?
        {
            runtime.lease.advance_transition()?;
            runtime.lease_store.persist(&runtime.lease)?;
        }
        self.commit_config(candidate)?;
        self.groups.insert(added.id, runtime);
        self.audit
            .record(added.id, "config", "success", None, Some("group_added"))?;
        Ok(vec![self.config_updated_message()])
    }

    fn handle_group_remove(&mut self, request: GroupRemove) -> FcpResult<Vec<Message>> {
        let Some(runtime) = self.groups.get(&request.account_group_id) else {
            return Ok(vec![Message::ConfigRejected(ConfigRejected {
                reason: "unknown_group".into(),
            })]);
        };
        // Explicit unprotect is a destructive, user-confirmed administrative operation. An
        // unfinished protection operation must block lease/inject, but must not make a group
        // impossible to remove. The config remains the authority for group existence; vault
        // deletion is staged below so a failed config commit still restores the only encrypted
        // session copy.
        // Stage rather than delete: a config write failure must leave the configured group and its
        // only encrypted session copy intact. Startup recovery resolves either crash window from
        // whether the group is still present in the authoritative config.
        runtime.transactions.stage_invalidation()?;
        let mut candidate = self.config.clone();
        candidate
            .groups
            .retain(|group| group.id != request.account_group_id);
        candidate.validate()?;
        if let Err(error) = self.commit_config(candidate) {
            self.groups
                .get(&request.account_group_id)
                .ok_or_else(|| FcpError::Protocol("group disappeared during removal".into()))?
                .transactions
                .rollback_staged_invalidation()?;
            return Err(error);
        }
        self.groups
            .get(&request.account_group_id)
            .ok_or_else(|| FcpError::Protocol("group disappeared during removal".into()))?
            .transactions
            .commit_staged_invalidation()?;
        self.groups.remove(&request.account_group_id);
        if self
            .paths
            .remove_group_projections(request.account_group_id)
            .is_err()
        {
            eprintln!("removed group left repairable projection files");
        }
        if self
            .audit
            .record(
                request.account_group_id,
                "config",
                "success",
                None,
                Some("group_removed"),
            )
            .is_err()
        {
            eprintln!("removed group audit append was deferred");
        }
        Ok(vec![self.config_updated_message()])
    }

    fn handle_group_set_policy(&mut self, request: GroupSetPolicy) -> FcpResult<Vec<Message>> {
        match self.groups.get(&request.account_group_id) {
            None => {
                return Ok(vec![Message::ConfigRejected(ConfigRejected {
                    reason: "unknown_group".into(),
                })]);
            }
            Some(runtime)
                if runtime.pending.is_some() || runtime.operation_coordinator.has_unresolved() =>
            {
                return Ok(vec![Message::ConfigRejected(ConfigRejected {
                    reason: "operation_pending".into(),
                })]);
            }
            Some(_) => {}
        }
        let entering_monitor = request.policy_level == PolicyLevel::Monitor
            && self
                .groups
                .get(&request.account_group_id)
                .is_some_and(|runtime| runtime.policy != PolicyLevel::Monitor);
        if entering_monitor
            && self
                .groups
                .get(&request.account_group_id)
                .is_some_and(|runtime| runtime.lease.state == GroupState::Sealed)
        {
            return Ok(vec![Message::ConfigRejected(ConfigRejected {
                reason: "monitor_transition_requires_unlocked_session".into(),
            })]);
        }
        if entering_monitor {
            self.groups
                .get(&request.account_group_id)
                .expect("validated group")
                .transactions
                .stage_invalidation()?;
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
        if let Err(error) = self.commit_config(candidate) {
            if entering_monitor {
                self.groups
                    .get(&request.account_group_id)
                    .expect("validated group")
                    .transactions
                    .rollback_staged_invalidation()?;
            }
            return Err(error);
        }
        // The live runtime carries its own copy of the policy, so it is updated in step with the
        // persisted config; otherwise lease durations would keep following the old level.
        if let Some(runtime) = self.groups.get_mut(&request.account_group_id) {
            runtime.policy = request.policy_level;
            runtime.hello_cache_expires_at = None;
            if entering_monitor {
                runtime.transactions.commit_staged_invalidation()?;
                runtime.lease.state = GroupState::Uninitialized;
                runtime.lease.lease_id = None;
                runtime.lease.granted_at_unix_ms = None;
                runtime.lease.expires_at_unix_ms = None;
                runtime.lease.pending_operation_id = None;
                runtime.lease.vault_sequence = 0;
                runtime.lease.advance_transition()?;
                runtime.lease_store.persist(&runtime.lease)?;
            }
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

fn profile_created_unix_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()))
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

fn profile_last_used_unix_ms(path: &Path) -> u64 {
    fn visit(path: &Path, latest: &mut u64) {
        let Ok(metadata) = fs::metadata(path) else {
            return;
        };
        if let Ok(modified) = metadata.modified()
            && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
        {
            *latest = (*latest).max(duration.as_millis().min(u128::from(u64::MAX)) as u64);
        }
        if !metadata.is_dir() {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            visit(&entry.path(), latest);
        }
    }
    let mut latest = 0;
    visit(path, &mut latest);
    latest
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

fn compare_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let parts = |value: &str| -> Option<[u64; 3]> {
        let parsed = value
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        parsed.try_into().ok()
    };
    Some(parts(left)?.cmp(&parts(right)?))
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
        hello_credential: &std::path::Path,
        negotiated_protocol: u16,
    ) -> FcpResult<Vec<Message>> {
        match message {
            Message::LeaseRequest(request) => self.handle_lease_request(
                request,
                audit,
                hello_authorizer,
                hello_credential,
                negotiated_protocol,
            ),
            Message::InjectResult(result) => self.handle_inject_result(result, audit),
            Message::EvictRequest(request) => {
                self.handle_evict_begin(request, audit, hello_authorizer)
            }
            Message::CookiesSnapshotChunk(snapshot) if negotiated_protocol == PROTOCOL_VERSION => {
                let sequence = snapshot.operation_sequence.ok_or_else(|| {
                    FcpError::Protocol("v7 snapshot lacks operation sequence".into())
                })?;
                let attempt = snapshot
                    .attempt_id
                    .ok_or_else(|| FcpError::Protocol("v7 snapshot lacks attempt id".into()))?;
                self.handle_snapshot_chunk_v7(
                    CookiesSnapshotChunkV7 {
                        account_group_id: snapshot.account_group_id,
                        operation_id: snapshot.operation_id,
                        operation_sequence: sequence,
                        lease_id: snapshot.lease_id,
                        attempt_id: attempt,
                        purpose: snapshot.purpose.ok_or_else(|| {
                            FcpError::Protocol("v7 snapshot lacks purpose".into())
                        })?,
                        chunk_index: snapshot.chunk_index,
                        chunk_count: snapshot.chunk_count,
                        cookie_count: snapshot.cookie_count,
                        cookies: snapshot.cookies,
                    },
                    audit,
                )
            }
            Message::CookiesSnapshotChunk(snapshot) => self.handle_snapshot_chunk(snapshot, audit),
            Message::EvictResult(result) if negotiated_protocol == PROTOCOL_VERSION => self
                .handle_evict_result_v7(
                    EvictResultV7 {
                        account_group_id: result.account_group_id,
                        operation_id: result.operation_id,
                        operation_sequence: result.operation_sequence.ok_or_else(|| {
                            FcpError::Protocol("v7 result lacks operation sequence".into())
                        })?,
                        lease_id: result.lease_id,
                        attempt_id: result.attempt_id.ok_or_else(|| {
                            FcpError::Protocol("v7 result lacks attempt id".into())
                        })?,
                        success: result.success,
                        remaining_cookie_count: result.remaining_cookie_count,
                    },
                    audit,
                ),
            Message::EvictResult(result) => self.handle_evict_result(result, audit),
            Message::OperationBegin(request) if negotiated_protocol == PROTOCOL_VERSION => {
                self.handle_operation_begin(request)
            }
            Message::OperationStatusQuery(request) if negotiated_protocol == PROTOCOL_VERSION => {
                self.handle_operation_status(request)
            }
            Message::EvictRemovePrepare(request) if negotiated_protocol == PROTOCOL_VERSION => {
                self.handle_remove_prepare(request)
            }
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
        hello_credential: &std::path::Path,
        negotiated_protocol: u16,
    ) -> FcpResult<Vec<Message>> {
        if self.pending.is_some() {
            return Ok(vec![self.lease_deny("operation_pending")]);
        }
        if self.operation_coordinator.has_unresolved() {
            return Ok(vec![self.lease_deny("operation_recovery_required")]);
        }
        let policy = self.policy.parameters();
        if policy.monitoring_only {
            return Ok(vec![self.lease_deny("monitoring_policy_has_no_lease")]);
        }
        match request.purpose {
            LeasePurpose::Enroll if negotiated_protocol == PROTOCOL_VERSION => {
                self.grant_enrollment_v7(policy.lease_duration_ms)
            }
            LeasePurpose::Enroll => self.begin_enrollment(policy.lease_duration_ms, audit),
            LeasePurpose::Inject => self.begin_inject(
                policy.lease_duration_ms,
                audit,
                hello_authorizer,
                hello_credential,
            ),
        }
    }

    fn grant_enrollment_v7(&mut self, duration_ms: u64) -> FcpResult<Vec<Message>> {
        if self.lease.state != GroupState::Uninitialized || self.transactions.vault_exists() {
            return Ok(vec![self.lease_deny("group_already_enrolled")]);
        }
        let now = unix_ms()?;
        let expiry = now
            .checked_add(duration_ms)
            .ok_or_else(|| FcpError::Protocol("lease expiry overflow".into()))?;
        let lease_id = Uuid::new_v4();
        self.lease.lease_id = Some(lease_id);
        self.lease.granted_at_unix_ms = Some(now);
        self.lease.expires_at_unix_ms = Some(expiry);
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        Ok(vec![Message::LeaseGrant(LeaseGrant {
            account_group_id: self.id,
            lease_id,
            expiry_unix_ms: expiry,
            capability_sequence: None,
        })])
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
        self.operation_coordinator.begin_v6(
            operation_id,
            OperationKind::Enrollment,
            Some(lease_id),
            "initial_enrollment".into(),
            self.lease.vault_sequence,
        )?;
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
            snapshot_chunk_count: None,
            snapshot_next_chunk: 0,
            snapshot_cookies: Vec::new(),
            snapshot_bytes: 0,
            operation_sequence: 0,
            attempt_id: None,
            snapshot_chunk_digests: BTreeMap::new(),
        });
        record_operation_audit(audit, self.id, operation_id, "enrollment", "started", None);
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
        hello_credential: &std::path::Path,
    ) -> FcpResult<Vec<Message>> {
        if self.lease.state != GroupState::Sealed || !self.transactions.vault_exists() {
            return Ok(vec![self.lease_deny("group_not_sealed")]);
        }
        let now = unix_ms()?;
        let cache_duration = self.policy.parameters().hello_cache_ms.unwrap_or(0);
        if hello_authorizer.is_none() {
            *hello_authorizer = Some(HelloAuthorizer::open_or_create(hello_credential)?);
        }
        let mut use_cached = cache_duration > 0
            && self
                .hello_cache_expires_at
                .is_some_and(|expiry| expiry > now)
            && hello_authorizer
                .as_ref()
                .is_some_and(|value| value.has_cached_handle(self.id));
        let authorization = match self.transactions.authorize_inject(
            hello_authorizer.as_ref().ok_or_else(|| {
                FcpError::Capability("Windows Hello authorizer was not initialized".into())
            })?,
            use_cached,
        ) {
            Ok(value) => value,
            Err(error) if HelloAuthorizer::is_missing_credential_error(&error) => {
                // Windows can remove a platform credential independently (Hello reset, account
                // recovery, TPM maintenance). Re-enroll only for the provider's not-found code;
                // user cancellation and verification failures remain hard failures.
                *hello_authorizer = Some(HelloAuthorizer::recreate(hello_credential)?);
                use_cached = false;
                self.transactions.authorize_inject(
                    hello_authorizer
                        .as_ref()
                        .expect("recreated Hello authorizer"),
                    false,
                )?
            }
            Err(error) => return Err(error),
        };
        let capability_sequence = authorization.monotonic_sequence();
        let payload = self.transactions.read_for_inject(authorization)?;
        if cache_duration > 0 {
            self.hello_cache_expires_at = now.checked_add(cache_duration);
        } else {
            self.hello_cache_expires_at = None;
            hello_authorizer
                .as_ref()
                .expect("Hello authorizer exists")
                .clear_cached_handle(self.id);
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
        let cookie_count = u32::try_from(payload.cookies.len())
            .map_err(|_| FcpError::Protocol("inject cookie count exceeds u32".into()))?;
        let chunks = chunk_cookie_records(payload.cookies)?;
        let chunk_count = u32::try_from(chunks.len())
            .map_err(|_| FcpError::Protocol("inject chunk count exceeds u32".into()))?;
        let mut messages = vec![Message::LeaseGrant(LeaseGrant {
            account_group_id: self.id,
            lease_id,
            expiry_unix_ms: expiry,
            capability_sequence: Some(capability_sequence),
        })];
        for (chunk_index, cookies) in chunks.into_iter().enumerate() {
            messages.push(Message::CookiesInjectChunk(CookiesInjectChunk {
                account_group_id: self.id,
                lease_id,
                chunk_index: u32::try_from(chunk_index)
                    .map_err(|_| FcpError::Protocol("inject chunk index exceeds u32".into()))?,
                chunk_count,
                cookie_count,
                cookies,
            }));
        }
        Ok(messages)
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
        if is_reconciliation {
            self.operation_coordinator
                .prepare_for_v6_reconciliation(&self.transactions.vault_path())?;
        } else if self.operation_coordinator.has_unresolved() {
            return Err(FcpError::Protocol(
                "operation recovery is required before a new eviction".into(),
            ));
        }
        let kind = if is_reconciliation {
            OperationKind::Reconciliation
        } else {
            OperationKind::Eviction
        };
        self.operation_coordinator.begin_v6(
            request.operation_id,
            kind,
            request.lease_id,
            request.reason.clone(),
            self.lease.vault_sequence,
        )?;
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
            snapshot_chunk_count: None,
            snapshot_next_chunk: 0,
            snapshot_cookies: Vec::new(),
            snapshot_bytes: 0,
            operation_sequence: 0,
            attempt_id: None,
            snapshot_chunk_digests: BTreeMap::new(),
        });
        record_operation_audit(
            audit,
            self.id,
            request.operation_id,
            if is_reconciliation {
                "reconciliation"
            } else {
                "eviction"
            },
            "started",
            None,
        );
        Ok(vec![Message::EvictRequest(EvictRequest {
            account_group_id: self.id,
            lease_id: request.lease_id,
            operation_id: request.operation_id,
            phase: EvictPhase::SnapshotRequired,
            reason: request.reason,
        })])
    }

    fn handle_snapshot_chunk(
        &mut self,
        snapshot: CookiesSnapshotChunk,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        let pending = self.pending.as_mut().ok_or_else(|| {
            FcpError::Protocol("snapshot chunk received without pending eviction".into())
        })?;
        if pending.operation_id != snapshot.operation_id || pending.lease_id != snapshot.lease_id {
            return Err(FcpError::Protocol(
                "snapshot operation binding mismatch".into(),
            ));
        }
        if pending.confirmed {
            return Err(FcpError::Protocol("duplicate snapshot received".into()));
        }
        if snapshot.chunk_count == 0 || snapshot.chunk_count > 65_536 {
            return Err(FcpError::Protocol(
                "snapshot chunk count is out of range".into(),
            ));
        }
        if snapshot.cookie_count > MAX_SNAPSHOT_COOKIES {
            return Err(FcpError::Protocol(
                "snapshot cookie count is out of range".into(),
            ));
        }
        if snapshot.chunk_index != pending.snapshot_next_chunk {
            return Err(FcpError::Protocol(
                "snapshot chunk is missing or out of order".into(),
            ));
        }
        if pending
            .snapshot_chunk_count
            .is_some_and(|value| value != snapshot.chunk_count)
        {
            return Err(FcpError::Protocol("snapshot chunk total changed".into()));
        }
        if snapshot.chunk_index + 1 < snapshot.chunk_count && snapshot.cookies.is_empty() {
            return Err(FcpError::Protocol(
                "non-final snapshot chunk is empty".into(),
            ));
        }
        let chunk_bytes = serde_json::to_vec(&snapshot.cookies)?.len();
        let aggregate_bytes = pending
            .snapshot_bytes
            .checked_add(chunk_bytes)
            .ok_or_else(|| FcpError::Protocol("snapshot byte count overflow".into()))?;
        if aggregate_bytes > MAX_SNAPSHOT_BYTES {
            return Err(FcpError::Protocol(
                "snapshot exceeds aggregate byte limit".into(),
            ));
        }
        let aggregate_count = pending
            .snapshot_cookies
            .len()
            .checked_add(snapshot.cookies.len())
            .ok_or_else(|| FcpError::Protocol("snapshot cookie count overflow".into()))?;
        if aggregate_count > snapshot.cookie_count as usize {
            return Err(FcpError::Protocol(
                "snapshot contains more cookies than declared".into(),
            ));
        }
        pending.snapshot_chunk_count = Some(snapshot.chunk_count);
        pending.snapshot_next_chunk += 1;
        pending.snapshot_bytes = aggregate_bytes;
        pending.snapshot_cookies.extend(snapshot.cookies);
        if pending.snapshot_next_chunk < snapshot.chunk_count {
            return Ok(Vec::new());
        }
        if aggregate_count != snapshot.cookie_count as usize {
            return Err(FcpError::Protocol(
                "snapshot cookie total does not match declaration".into(),
            ));
        }

        let mut pending = self.pending.take().expect("pending snapshot was validated");
        let cookies = std::mem::take(&mut pending.snapshot_cookies);
        if pending.kind == PendingKind::Enrollment && cookies.is_empty() {
            // An empty scope is a normal observation, not an ambiguous commit. Leaving the
            // operation NotCommitted and the lease Evicting made this harmless case permanently
            // block reconciliation and even explicit removal of the configured group.
            self.operation_coordinator.abort(snapshot.operation_id)?;
            self.lease.state = GroupState::Uninitialized;
            self.lease.lease_id = None;
            self.lease.granted_at_unix_ms = None;
            self.lease.expires_at_unix_ms = None;
            self.lease.pending_operation_id = None;
            self.lease.advance_transition()?;
            self.lease_store.persist(&self.lease)?;
            record_operation_audit(
                audit,
                self.id,
                snapshot.operation_id,
                "enrollment",
                "aborted",
                Some("scope_empty"),
            );
            return Ok(vec![Message::SessionInvalidated(SessionInvalidated {
                account_group_id: self.id,
                reason: SessionInvalidationReason::ScopeEmpty,
            })]);
        }
        let snapshot_cookie_count = u32::try_from(cookies.len())
            .map_err(|_| FcpError::Protocol("snapshot cookie count exceeds u32".into()))?;
        let canonical_snapshot = serde_json::to_vec(&cookies)?;
        let prepared = match self.transactions.prepare_after_snapshot(cookies) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.pending = Some(pending);
                return Err(error);
            }
        };
        let vault_sequence = match self.operation_coordinator.commit_snapshot(
            snapshot.operation_id,
            canonical_snapshot,
            snapshot_cookie_count,
            prepared,
            &self.transactions,
        ) {
            Ok(sequence) => sequence,
            Err(error) => {
                self.pending = Some(pending);
                return Err(error);
            }
        };
        pending.confirmed = true;
        pending.snapshot_cookie_count = Some(snapshot_cookie_count);
        self.lease.vault_sequence = vault_sequence;
        self.lease.pending_operation_id = Some(snapshot.operation_id);
        if let Err(error) = self.lease_store.persist(&self.lease) {
            self.pending = Some(pending);
            return Err(error);
        }
        record_operation_audit(
            audit,
            self.id,
            snapshot.operation_id,
            "vault_snapshot",
            "verified",
            None,
        );
        if let Err(error) = self
            .operation_coordinator
            .mark_external_result_pending(snapshot.operation_id)
        {
            self.pending = Some(pending);
            return Err(error);
        }
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
            operation_sequence: None,
            attempt_id: None,
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
            self.operation_coordinator.complete(result.operation_id)?;
            self.lease.state = GroupState::Leased;
            self.lease.pending_operation_id = None;
            self.lease.advance_transition()?;
            self.lease_store.persist(&self.lease)?;
            record_operation_audit(
                audit,
                self.id,
                result.operation_id,
                "enrollment",
                "success",
                None,
            );
            return Ok(Vec::new());
        }
        if pending.kind != PendingKind::Enrollment
            && result.success
            && result.remaining_cookie_count == 0
        {
            self.operation_coordinator.complete(result.operation_id)?;
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
            record_operation_audit(audit, self.id, result.operation_id, event, "success", None);
            return Ok(Vec::new());
        }
        self.operation_coordinator
            .require_reconciliation(result.operation_id)?;
        self.lease.state = GroupState::Degraded;
        self.lease.pending_operation_id = None;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        let (event, detail) = if pending.kind == PendingKind::Enrollment {
            ("enrollment", "retained_cookie_count_mismatch")
        } else {
            ("eviction", "cookies_remain")
        };
        record_operation_audit(
            audit,
            self.id,
            result.operation_id,
            event,
            "failed",
            Some(detail),
        );
        Ok(Vec::new())
    }

    fn handle_operation_begin(&mut self, request: OperationBegin) -> FcpResult<Vec<Message>> {
        if request.account_group_id != self.id || request.attempt_id.is_nil() {
            return Err(FcpError::Protocol(
                "operation begin identity is invalid".into(),
            ));
        }
        if let Some(existing) = self
            .operation_coordinator
            .operation_by_attempt(request.attempt_id)
        {
            let expected_kind = wire_kind(request.kind);
            if existing.kind != expected_kind
                || existing.lease_id != request.lease_id
                || existing.reason_code != request.reason
            {
                return Err(FcpError::Protocol(
                    "operation attempt was reused with different payload".into(),
                ));
            }
            // The host may have durably issued this identity while the corresponding response was
            // lost before the extension persisted it. For NotCommitted, replay the binding-bearing
            // response rather than a status that an unbound client cannot authenticate yet.
            if existing.phase == crate::operation::OperationPhase::NotCommitted {
                return Ok(vec![Message::OperationSnapshotRequired(
                    OperationSnapshotRequired {
                        account_group_id: self.id,
                        operation_id: existing.operation_id,
                        operation_sequence: existing.sequence,
                        lease_id: existing.lease_id,
                        attempt_id: request.attempt_id,
                    },
                )]);
            }
            return Ok(vec![self.semantic_status(&existing)]);
        }
        let valid = match request.kind {
            OperationKindWire::Enrollment => self.lease.state == GroupState::Uninitialized,
            OperationKindWire::Eviction => {
                self.lease.state == GroupState::Leased
                    && self.lease.lease_id == request.lease_id
                    && request.lease_id.is_some()
            }
            OperationKindWire::Reconciliation => self.lease.state != GroupState::Uninitialized,
        };
        if !valid || self.pending.is_some() {
            return Err(FcpError::Protocol(
                "operation begin is invalid for current lease".into(),
            ));
        }
        let operation = self.operation_coordinator.issue_v7(
            request.attempt_id,
            wire_kind(request.kind),
            request.lease_id,
            request.reason,
            self.lease.vault_sequence,
        )?;
        self.lease.state = GroupState::Evicting;
        self.lease.pending_operation_id = Some(operation.operation_id);
        self.lease.operation_sequence_high_water = operation.sequence;
        self.lease.advance_transition()?;
        self.lease_store.persist(&self.lease)?;
        self.pending = Some(PendingOperation {
            kind: match request.kind {
                OperationKindWire::Enrollment => PendingKind::Enrollment,
                OperationKindWire::Eviction => PendingKind::Eviction,
                OperationKindWire::Reconciliation => PendingKind::Reconciliation,
            },
            operation_id: operation.operation_id,
            lease_id: request.lease_id,
            confirmed: false,
            snapshot_cookie_count: None,
            snapshot_chunk_count: None,
            snapshot_next_chunk: 0,
            snapshot_cookies: Vec::new(),
            snapshot_bytes: 0,
            operation_sequence: operation.sequence,
            attempt_id: Some(request.attempt_id),
            snapshot_chunk_digests: BTreeMap::new(),
        });
        Ok(vec![Message::OperationSnapshotRequired(
            OperationSnapshotRequired {
                account_group_id: self.id,
                operation_id: operation.operation_id,
                operation_sequence: operation.sequence,
                lease_id: request.lease_id,
                attempt_id: request.attempt_id,
            },
        )])
    }

    fn handle_operation_status(
        &mut self,
        request: OperationStatusQuery,
    ) -> FcpResult<Vec<Message>> {
        if request.account_group_id != self.id {
            return Err(FcpError::Protocol("operation status group mismatch".into()));
        }
        let operation = self
            .operation_coordinator
            .operation(request.operation_id)
            .ok_or_else(|| FcpError::Protocol("unknown operation id".into()))?;
        if operation.sequence != request.operation_sequence
            || operation.lease_id != request.lease_id
        {
            return Err(FcpError::Protocol(
                "operation status binding mismatch".into(),
            ));
        }
        if self.pending.is_none() && !operation.phase.is_terminal() {
            self.pending = Some(PendingOperation {
                kind: match operation.kind {
                    OperationKind::Enrollment => PendingKind::Enrollment,
                    OperationKind::Eviction => PendingKind::Eviction,
                    OperationKind::Reconciliation => PendingKind::Reconciliation,
                },
                operation_id: operation.operation_id,
                lease_id: operation.lease_id,
                confirmed: matches!(
                    operation.phase,
                    crate::operation::OperationPhase::Committed
                        | crate::operation::OperationPhase::BrowserRemovalPending
                ),
                snapshot_cookie_count: operation.snapshot.as_ref().map(|value| value.cookie_count),
                snapshot_chunk_count: None,
                snapshot_next_chunk: 0,
                snapshot_cookies: Vec::new(),
                snapshot_bytes: 0,
                operation_sequence: operation.sequence,
                attempt_id: operation.attempt_id,
                snapshot_chunk_digests: BTreeMap::new(),
            });
        }
        Ok(vec![self.semantic_status(&operation)])
    }

    fn semantic_status(&self, operation: &crate::operation::JournalOperation) -> Message {
        use crate::operation::OperationPhase;
        let required_action = match operation.phase {
            OperationPhase::NotCommitted => OperationRequiredAction::SendSnapshot,
            OperationPhase::DurabilityUnknown => OperationRequiredAction::ClassifyDurability,
            OperationPhase::Committed | OperationPhase::BrowserRemovalPending => {
                OperationRequiredAction::PrepareRemoval
            }
            OperationPhase::Completed | OperationPhase::Aborted => {
                OperationRequiredAction::Completed
            }
            OperationPhase::ReconciliationRequired => {
                OperationRequiredAction::ReconciliationRequired
            }
        };
        Message::OperationStatus(OperationStatus {
            account_group_id: self.id,
            operation_id: operation.operation_id,
            operation_sequence: operation.sequence,
            lease_id: operation.lease_id,
            phase: format!("{:?}", operation.phase).to_ascii_lowercase(),
            required_action,
        })
    }

    fn handle_snapshot_chunk_v7(
        &mut self,
        snapshot: CookiesSnapshotChunkV7,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        let pending = self
            .pending
            .as_mut()
            .ok_or_else(|| FcpError::Protocol("snapshot has no active operation".into()))?;
        if snapshot.account_group_id != self.id
            || pending.operation_id != snapshot.operation_id
            || pending.operation_sequence != snapshot.operation_sequence
            || pending.lease_id != snapshot.lease_id
            || pending.attempt_id != Some(snapshot.attempt_id)
        {
            return Err(FcpError::Protocol(
                "v7 snapshot operation/sequence/attempt binding mismatch".into(),
            ));
        }
        let chunk_digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&snapshot.cookies)?).into();
        if let Some(existing) = pending.snapshot_chunk_digests.get(&snapshot.chunk_index) {
            return if existing == &chunk_digest {
                Ok(Vec::new())
            } else {
                Err(FcpError::Protocol(
                    "duplicate snapshot chunk payload conflict".into(),
                ))
            };
        }
        pending
            .snapshot_chunk_digests
            .insert(snapshot.chunk_index, chunk_digest);
        if snapshot.purpose == "removal_precheck" {
            return self.handle_removal_snapshot_chunk(snapshot);
        }
        if snapshot.purpose != "commit" {
            return Err(FcpError::Protocol("unknown snapshot purpose".into()));
        }
        let operation_sequence = snapshot.operation_sequence;
        let attempt_id = snapshot.attempt_id;
        let output = self.handle_snapshot_chunk(
            CookiesSnapshotChunk {
                account_group_id: snapshot.account_group_id,
                lease_id: snapshot.lease_id,
                operation_id: snapshot.operation_id,
                chunk_index: snapshot.chunk_index,
                chunk_count: snapshot.chunk_count,
                cookie_count: snapshot.cookie_count,
                cookies: snapshot.cookies,
                operation_sequence: None,
                attempt_id: None,
                purpose: None,
            },
            audit,
        )?;
        Ok(output
            .into_iter()
            .map(|message| match message {
                Message::EvictConfirmed(value) => Message::EvictConfirmed(EvictConfirmed {
                    account_group_id: value.account_group_id,
                    operation_id: value.operation_id,
                    operation_sequence: Some(operation_sequence),
                    lease_id: value.lease_id,
                    attempt_id: Some(attempt_id),
                    vault_sequence: value.vault_sequence,
                    cookie_disposition: value.cookie_disposition,
                }),
                other => other,
            })
            .collect())
    }

    fn handle_remove_prepare(&mut self, request: EvictRemovePrepare) -> FcpResult<Vec<Message>> {
        let pending = self
            .pending
            .as_mut()
            .ok_or_else(|| FcpError::Protocol("removal prepare has no active operation".into()))?;
        if request.account_group_id != self.id
            || pending.operation_id != request.operation_id
            || pending.operation_sequence != request.operation_sequence
            || pending.lease_id != request.lease_id
            || pending.attempt_id != Some(request.attempt_id)
            || !pending.confirmed
        {
            return Err(FcpError::Protocol(
                "removal prepare binding mismatch".into(),
            ));
        }
        pending.snapshot_cookie_count = None;
        pending.snapshot_chunk_count = None;
        pending.snapshot_next_chunk = 0;
        pending.snapshot_cookies.clear();
        pending.snapshot_bytes = 0;
        pending.snapshot_chunk_digests.clear();
        Ok(Vec::new())
    }

    fn handle_removal_snapshot_chunk(
        &mut self,
        snapshot: CookiesSnapshotChunkV7,
    ) -> FcpResult<Vec<Message>> {
        let pending = self
            .pending
            .as_mut()
            .ok_or_else(|| FcpError::Protocol("removal snapshot has no active operation".into()))?;
        append_v7_chunk(pending, &snapshot)?;
        if pending.snapshot_next_chunk < snapshot.chunk_count {
            return Ok(Vec::new());
        }
        let canonical = serde_json::to_vec(&pending.snapshot_cookies)?;
        let matches = self.operation_coordinator.verify_snapshot(
            snapshot.operation_id,
            snapshot.operation_sequence,
            snapshot.attempt_id,
            canonical,
        )?;
        if !matches {
            self.operation_coordinator
                .require_reconciliation(snapshot.operation_id)?;
            self.lease.state = GroupState::Degraded;
            self.lease.advance_transition()?;
            self.lease_store.persist(&self.lease)?;
            self.pending = None;
            return Ok(vec![Message::OperationCompleted(OperationCompleted {
                account_group_id: self.id,
                operation_id: snapshot.operation_id,
                operation_sequence: snapshot.operation_sequence,
                lease_id: snapshot.lease_id,
                success: false,
            })]);
        }
        Ok(vec![Message::EvictRemoveAuthorized(
            EvictRemoveAuthorized {
                account_group_id: self.id,
                operation_id: snapshot.operation_id,
                operation_sequence: snapshot.operation_sequence,
                lease_id: snapshot.lease_id,
                attempt_id: snapshot.attempt_id,
            },
        )])
    }

    fn handle_evict_result_v7(
        &mut self,
        result: EvictResultV7,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        let pending = self
            .pending
            .as_ref()
            .ok_or_else(|| FcpError::Protocol("evict result has no active operation".into()))?;
        if result.account_group_id != self.id
            || pending.operation_sequence != result.operation_sequence
            || pending.attempt_id != Some(result.attempt_id)
        {
            return Err(FcpError::Protocol(
                "v7 evict result binding mismatch".into(),
            ));
        }
        let completion_success = result.success
            && if pending.kind == PendingKind::Enrollment {
                result.remaining_cookie_count > 0
            } else {
                result.remaining_cookie_count == 0
            };
        let completed = OperationCompleted {
            account_group_id: self.id,
            operation_id: result.operation_id,
            operation_sequence: result.operation_sequence,
            lease_id: result.lease_id,
            success: completion_success,
        };
        self.handle_evict_result(
            EvictResult {
                account_group_id: result.account_group_id,
                lease_id: result.lease_id,
                operation_id: result.operation_id,
                success: result.success,
                remaining_cookie_count: result.remaining_cookie_count,
                operation_sequence: None,
                attempt_id: None,
            },
            audit,
        )?;
        Ok(vec![Message::OperationCompleted(completed)])
    }

    fn lease_deny(&self, reason: &str) -> Message {
        Message::LeaseDeny(LeaseDeny {
            account_group_id: self.id,
            reason: reason.into(),
        })
    }
}

fn wire_kind(kind: OperationKindWire) -> OperationKind {
    match kind {
        OperationKindWire::Enrollment => OperationKind::Enrollment,
        OperationKindWire::Eviction => OperationKind::Eviction,
        OperationKindWire::Reconciliation => OperationKind::Reconciliation,
    }
}

fn record_operation_audit(
    audit: &AuditLogger,
    group_id: Uuid,
    operation_id: Uuid,
    event: &'static str,
    outcome: &'static str,
    detail: Option<&'static str>,
) {
    let mut hasher = Sha256::new();
    hasher.update(b"FCP-AUDIT-EVENT-1");
    hasher.update(group_id.as_bytes());
    hasher.update(operation_id.as_bytes());
    hasher.update(event.as_bytes());
    hasher.update(outcome.as_bytes());
    let mut bytes: [u8; 16] = hasher.finalize()[..16].try_into().expect("digest prefix");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let event_id = Uuid::from_bytes(bytes);
    audit.record_once_deferred(
        event_id,
        group_id,
        event,
        outcome,
        Some(operation_id),
        detail,
    );
}

fn append_v7_chunk(
    pending: &mut PendingOperation,
    snapshot: &CookiesSnapshotChunkV7,
) -> FcpResult<()> {
    if snapshot.chunk_count == 0
        || snapshot.chunk_count > 65_536
        || snapshot.cookie_count > MAX_SNAPSHOT_COOKIES
    {
        return Err(FcpError::Protocol(
            "snapshot chunk metadata is out of range".into(),
        ));
    }
    let encoded = serde_json::to_vec(&snapshot.cookies)?;
    let digest: [u8; 32] = Sha256::digest(&encoded).into();
    if snapshot.chunk_index < pending.snapshot_next_chunk {
        return match pending.snapshot_chunk_digests.get(&snapshot.chunk_index) {
            Some(existing) if existing == &digest => Ok(()),
            _ => Err(FcpError::Protocol(
                "duplicate snapshot chunk payload conflict".into(),
            )),
        };
    }
    if snapshot.chunk_index != pending.snapshot_next_chunk
        || pending
            .snapshot_chunk_count
            .is_some_and(|value| value != snapshot.chunk_count)
    {
        return Err(FcpError::Protocol(
            "snapshot chunk binding or order mismatch".into(),
        ));
    }
    let next_bytes = pending
        .snapshot_bytes
        .checked_add(encoded.len())
        .ok_or_else(|| FcpError::Protocol("snapshot byte count overflow".into()))?;
    let next_count = pending
        .snapshot_cookies
        .len()
        .checked_add(snapshot.cookies.len())
        .ok_or_else(|| FcpError::Protocol("snapshot cookie count overflow".into()))?;
    if next_bytes > MAX_SNAPSHOT_BYTES || next_count > snapshot.cookie_count as usize {
        return Err(FcpError::Protocol(
            "snapshot aggregate declaration exceeded".into(),
        ));
    }
    pending.snapshot_chunk_count = Some(snapshot.chunk_count);
    pending
        .snapshot_chunk_digests
        .insert(snapshot.chunk_index, digest);
    pending.snapshot_next_chunk += 1;
    pending.snapshot_bytes = next_bytes;
    pending.snapshot_cookies.extend(snapshot.cookies.clone());
    if pending.snapshot_next_chunk == snapshot.chunk_count
        && next_count != snapshot.cookie_count as usize
    {
        return Err(FcpError::Protocol(
            "snapshot cookie total does not match declaration".into(),
        ));
    }
    Ok(())
}

fn message_group_id(message: &Message) -> Option<Uuid> {
    match message {
        Message::LeaseRequest(value) => Some(value.account_group_id),
        Message::InjectResult(value) => Some(value.account_group_id),
        Message::EvictRequest(value) => Some(value.account_group_id),
        Message::CookiesSnapshotChunk(value) => Some(value.account_group_id),
        Message::EvictResult(value) => Some(value.account_group_id),
        Message::OperationBegin(value) => Some(value.account_group_id),
        Message::OperationStatusQuery(value) => Some(value.account_group_id),
        Message::EvictRemovePrepare(value) => Some(value.account_group_id),
        Message::SessionInvalidate(value) => Some(value.account_group_id),
        Message::AuthCacheClear(value) => Some(value.account_group_id),
        Message::MonitorEvent(value) => value.account_group_id,
        _ => None,
    }
}

fn chunk_cookie_records(cookies: Vec<CookieRecord>) -> FcpResult<Vec<Vec<CookieRecord>>> {
    if cookies.is_empty() {
        return Ok(vec![Vec::new()]);
    }

    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_bytes = 2usize; // JSON array brackets.
    for cookie in cookies {
        let cookie_bytes = serde_json::to_vec(&cookie)?.len() + usize::from(!current.is_empty());
        if cookie_bytes > COOKIE_CHUNK_TARGET_BYTES {
            return Err(FcpError::Protocol(
                "one cookie exceeds the chunk byte limit".into(),
            ));
        }
        if !current.is_empty() && current_bytes + cookie_bytes > COOKIE_CHUNK_TARGET_BYTES {
            chunks.push(std::mem::take(&mut current));
            current_bytes = 2;
        }
        current_bytes += cookie_bytes;
        current.push(cookie);
    }
    chunks.push(current);
    Ok(chunks)
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
    use crate::crypto::aead::{DEK_BYTES, SecretDek};
    use crate::crypto::platform_kek::WRAPPED_DEK_BYTES;
    use crate::operation::OperationPhase;
    use crate::protocol::messages::{
        Handshake, MonitorEvent, MonitorSeverity, MonitorSignal, MonitorSource,
    };
    use crate::test_support::{FailurePoint, fail_next};
    use crate::transaction::PreparedVaultWrite;
    use crate::vault::format::VaultRecord;
    use crate::vault::payload::VaultPayload;

    // Two-group fixture, self-contained in the test module: most dispatcher tests want a
    // deterministic starting point (a scope-overlap test in particular needs a real parent
    // domain already registered). This used to ride on the bundled production default, but that
    // now ships empty for a real fresh install (2026-08-08), so tests seed their own copy here
    // instead of depending on production config content.
    fn test_paths(root: &std::path::Path) -> DataPaths {
        let account_groups_config = root.join("config/account-groups.json");
        fs::create_dir_all(account_groups_config.parent().unwrap()).unwrap();
        fs::write(
            &account_groups_config,
            r#"{
  "version": 2,
  "compatibility_version": 2,
  "groups": [
    {
      "id": "7a144677-3f5c-4a86-a767-16fd3ca315b8",
      "display_name": "Test Group A",
      "scope": "wikipedia.org",
      "policy_level": "balanced",
      "eviction_triggers": ["last_tab_closed", "idle", "lock", "expiry", "manual"],
      "store_policy": "normal_profile"
    },
    {
      "id": "c2b71d84-6d3f-45d4-a184-994083ba7659",
      "display_name": "Test Group B",
      "scope": "localhost",
      "policy_level": "critical",
      "eviction_triggers": ["last_tab_closed", "idle", "lock", "expiry", "manual"],
      "store_policy": "normal_profile"
    }
  ]
}"#,
        )
        .unwrap();
        DataPaths {
            root: root.to_path_buf(),
            vault_groups: root.join("vault/groups"),
            lease_groups: root.join("leases/groups"),
            capability_ledgers: root.join("leases/capabilities"),
            legacy_lease_metadata: root.join("leases/mvp-group.json"),
            legacy_capability_ledger: root.join("leases/capability-ledger.json"),
            account_groups_config,
            audit_directory: root.join("audit"),
            hello_credential: root.join("hello-credential.json"),
            operation_journals: root.join("operations/groups"),
            snapshot_integrity_key: root.join("operations/snapshot-key.dpapi"),
        }
    }

    fn handshaken(paths: &DataPaths) -> NativeHostApp {
        let mut app = NativeHostApp::open(paths).unwrap();
        app.handle(Message::Handshake(Handshake {
            protocol_version: PROTOCOL_VERSION,
            extension_id: PRODUCT_EXTENSION_ID.into(),
            profile_id: Uuid::new_v4(),
            extension_version: MIN_EXTENSION_VERSION.into(),
            min_host_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: PROTOCOL_CAPABILITIES
                .iter()
                .map(|value| (*value).into())
                .collect(),
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
        let added_id = match &output[0] {
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
                added.id
            }
            other => panic!("expected config.updated, got {other:?}"),
        };
        assert_eq!(app.groups[&added_id].lease.protocol_floor, PROTOCOL_VERSION);
        assert_eq!(
            app.groups[&added_id]
                .operation_coordinator
                .bound_profile_id(),
            app.connection_profile_id
        );
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
    fn explicit_group_removal_cancels_unresolved_operation_and_cleans_projections() {
        let root =
            std::env::temp_dir().join(format!("fcp-group-remove-degraded-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let victim = *app.groups.keys().next().unwrap();
        let operation_id = Uuid::new_v4();
        app.groups
            .get_mut(&victim)
            .unwrap()
            .operation_coordinator
            .begin_v6(
                operation_id,
                OperationKind::Reconciliation,
                None,
                "startup_reconciliation".into(),
                0,
            )
            .unwrap();
        assert!(paths.operation_journal_path(victim).exists());

        let output = app
            .handle(Message::GroupRemove(GroupRemove {
                account_group_id: victim,
            }))
            .unwrap();

        assert!(matches!(output.as_slice(), [Message::ConfigUpdated(_)]));
        assert!(!app.groups.contains_key(&victim));
        assert!(!paths.operation_journal_path(victim).exists());
        assert!(!paths.lease_path(victim).exists());
        assert!(!paths.capability_path(victim).exists());
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
    fn monitor_transition_resets_unprotected_runtime_and_rejects_sealed_session() {
        let root = std::env::temp_dir().join(format!("fcp-monitor-policy-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let target = *app.groups.keys().next().unwrap();

        let output = app
            .handle(Message::GroupSetPolicy(GroupSetPolicy {
                account_group_id: target,
                policy_level: PolicyLevel::Monitor,
            }))
            .unwrap();
        assert!(matches!(output[0], Message::ConfigUpdated(_)));
        assert_eq!(app.groups[&target].policy, PolicyLevel::Monitor);
        assert_eq!(app.groups[&target].lease.state, GroupState::Uninitialized);

        app.groups.get_mut(&target).unwrap().policy = PolicyLevel::Balanced;
        app.groups.get_mut(&target).unwrap().lease.state = GroupState::Sealed;
        let rejected = app
            .handle(Message::GroupSetPolicy(GroupSetPolicy {
                account_group_id: target,
                policy_level: PolicyLevel::Monitor,
            }))
            .unwrap();
        match &rejected[0] {
            Message::ConfigRejected(value) => {
                assert_eq!(value.reason, "monitor_transition_requires_unlocked_session")
            }
            other => panic!("expected config.rejected, got {other:?}"),
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_the_last_group_leaves_zero_groups_configured() {
        // Zero groups is a valid state pre-launch (2026-08-08): a fresh install starts empty and
        // the user adds their first group through onboarding, so removal must not be blocked at
        // one remaining group either.
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
            Message::ConfigUpdated(_) => {}
            other => panic!("expected config.updated, got {other:?}"),
        }
        assert_eq!(app.groups.len(), 0);
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
                profile_id: Uuid::new_v4(),
                extension_version: MIN_EXTENSION_VERSION.into(),
                min_host_version: env!("CARGO_PKG_VERSION").into(),
                capabilities: PROTOCOL_CAPABILITIES
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
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
    fn handshake_rejects_incompatible_versions_and_capabilities() {
        let cases = [
            ("0.3.0", env!("CARGO_PKG_VERSION"), true),
            (MIN_EXTENSION_VERSION, "99.0.0", true),
            (MIN_EXTENSION_VERSION, env!("CARGO_PKG_VERSION"), false),
            ("not-a-version", env!("CARGO_PKG_VERSION"), true),
        ];
        for (index, (extension_version, min_host_version, include_capabilities)) in
            cases.into_iter().enumerate()
        {
            let root = std::env::temp_dir().join(format!(
                "fcp-incompatible-handshake-{index}-{}",
                Uuid::new_v4()
            ));
            let paths = test_paths(&root);
            let mut app = NativeHostApp::open(&paths).unwrap();
            let capabilities = if include_capabilities {
                PROTOCOL_CAPABILITIES
                    .iter()
                    .map(|value| (*value).into())
                    .collect()
            } else {
                Vec::new()
            };
            assert!(
                app.handle(Message::Handshake(Handshake {
                    protocol_version: PROTOCOL_VERSION,
                    extension_id: PRODUCT_EXTENSION_ID.into(),
                    profile_id: Uuid::new_v4(),
                    extension_version: extension_version.into(),
                    min_host_version: min_host_version.into(),
                    capabilities,
                    cached_config_digest: None,
                }))
                .is_err()
            );
            fs::remove_dir_all(root).unwrap();
        }
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
                profile_id: Uuid::new_v4(),
                extension_version: MIN_EXTENSION_VERSION.into(),
                min_host_version: env!("CARGO_PKG_VERSION").into(),
                capabilities: PROTOCOL_CAPABILITIES
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
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
                profile_id: Uuid::new_v4(),
                extension_version: MIN_EXTENSION_VERSION.into(),
                min_host_version: env!("CARGO_PKG_VERSION").into(),
                capabilities: PROTOCOL_CAPABILITIES
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
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
            profile_id: Uuid::new_v4(),
            extension_version: MIN_EXTENSION_VERSION.into(),
            min_host_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: PROTOCOL_CAPABILITIES
                .iter()
                .map(|value| (*value).into())
                .collect(),
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
            profile_id: Uuid::new_v4(),
            extension_version: MIN_EXTENSION_VERSION.into(),
            min_host_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: PROTOCOL_CAPABILITIES
                .iter()
                .map(|value| (*value).into())
                .collect(),
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

    fn synthetic_cookie(index: usize, value_bytes: usize) -> CookieRecord {
        CookieRecord {
            domain: ".example.test".into(),
            expiration_date: None,
            host_only: false,
            http_only: true,
            name: format!("cookie-{index}"),
            partition_key: None,
            path: "/".into(),
            same_site: crate::protocol::messages::SameSiteStatus::Lax,
            secure: true,
            session: true,
            store_id: "0".into(),
            value: "x".repeat(value_bytes),
        }
    }

    fn prepared_test_vault(group_id: Uuid, sequence: u64) -> PreparedVaultWrite {
        let dek = SecretDek::from_bytes([61; DEK_BYTES]);
        let mut payload = VaultPayload::empty();
        payload.vault_sequence = sequence;
        let record = VaultRecord::seal(
            group_id,
            [62; 16],
            vec![63; WRAPPED_DEK_BYTES],
            &dek,
            &payload,
        )
        .unwrap();
        PreparedVaultWrite::for_test(group_id, record.encode().unwrap(), dek, None, sequence)
    }

    #[test]
    fn cookie_chunking_carries_an_aggregate_larger_than_one_mib_in_bounded_ordered_frames() {
        let cookies: Vec<_> = (0..4)
            .map(|index| synthetic_cookie(index, 300 * 1024))
            .collect();

        let chunks = chunk_cookie_records(cookies.clone()).unwrap();

        assert!(serde_json::to_vec(&cookies).unwrap().len() > 1024 * 1024);
        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| serde_json::to_vec(chunk).unwrap().len() <= COOKIE_CHUNK_TARGET_BYTES)
        );
        assert_eq!(chunks.into_iter().flatten().collect::<Vec<_>>(), cookies);
    }

    #[test]
    fn snapshot_handler_rejects_declared_cookie_count_above_the_production_limit() {
        let root = std::env::temp_dir().join(format!("fcp-snapshot-limit-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = NativeHostApp::open(&paths).unwrap();
        let group_id = *app.groups.keys().next().unwrap();
        let operation_id = Uuid::new_v4();
        app.groups.get_mut(&group_id).unwrap().pending = Some(PendingOperation {
            kind: PendingKind::Eviction,
            operation_id,
            lease_id: None,
            confirmed: false,
            snapshot_cookie_count: None,
            snapshot_chunk_count: None,
            snapshot_next_chunk: 0,
            snapshot_cookies: Vec::new(),
            snapshot_bytes: 0,
            operation_sequence: 0,
            attempt_id: None,
            snapshot_chunk_digests: BTreeMap::new(),
        });

        let audit = &app.audit;
        let runtime = app.groups.get_mut(&group_id).unwrap();
        let result = runtime.handle_snapshot_chunk(
            CookiesSnapshotChunk {
                account_group_id: group_id,
                lease_id: None,
                operation_id,
                chunk_index: 0,
                chunk_count: 1,
                cookie_count: MAX_SNAPSHOT_COOKIES + 1,
                cookies: Vec::new(),
                operation_sequence: None,
                attempt_id: None,
                purpose: None,
            },
            audit,
        );

        assert!(
            matches!(result, Err(FcpError::Protocol(message)) if message == "snapshot cookie count is out of range")
        );
        assert_eq!(runtime.pending.as_ref().unwrap().snapshot_next_chunk, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_enrollment_aborts_without_leaving_group_degraded_or_operation_unresolved() {
        let root = std::env::temp_dir().join(format!("fcp-empty-enrollment-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let group_id = *app.groups.keys().next().unwrap();

        let grant = app
            .handle(Message::LeaseRequest(LeaseRequest {
                account_group_id: group_id,
                purpose: LeasePurpose::Enroll,
            }))
            .unwrap();
        let lease_id = match grant.as_slice() {
            [Message::LeaseGrant(value)] => value.lease_id,
            other => panic!("expected lease grant, got {other:?}"),
        };
        let attempt_id = Uuid::new_v4();
        let issued = app
            .handle(Message::OperationBegin(OperationBegin {
                account_group_id: group_id,
                lease_id: Some(lease_id),
                attempt_id,
                kind: OperationKindWire::Enrollment,
                reason: "initial_enrollment".into(),
            }))
            .unwrap();
        let (operation_id, operation_sequence) = match issued.as_slice() {
            [Message::OperationSnapshotRequired(value)] => {
                (value.operation_id, value.operation_sequence)
            }
            other => panic!("expected snapshot request, got {other:?}"),
        };

        let retried = app
            .handle(Message::OperationBegin(OperationBegin {
                account_group_id: group_id,
                lease_id: Some(lease_id),
                attempt_id,
                kind: OperationKindWire::Enrollment,
                reason: "initial_enrollment".into(),
            }))
            .unwrap();
        assert!(matches!(
            retried.as_slice(),
            [Message::OperationSnapshotRequired(value)]
                if value.operation_id == operation_id
                    && value.operation_sequence == operation_sequence
                    && value.attempt_id == attempt_id
        ));

        let completed = app
            .handle(Message::CookiesSnapshotChunk(CookiesSnapshotChunk {
                account_group_id: group_id,
                lease_id: Some(lease_id),
                operation_id,
                chunk_index: 0,
                chunk_count: 1,
                cookie_count: 0,
                cookies: Vec::new(),
                operation_sequence: Some(operation_sequence),
                attempt_id: Some(attempt_id),
                purpose: Some("commit".into()),
            }))
            .unwrap();

        assert!(matches!(
            completed.as_slice(),
            [Message::SessionInvalidated(SessionInvalidated {
                reason: SessionInvalidationReason::ScopeEmpty,
                ..
            })]
        ));
        let runtime = &app.groups[&group_id];
        assert_eq!(runtime.lease.state, GroupState::Uninitialized);
        assert_eq!(runtime.lease.lease_id, None);
        assert_eq!(runtime.lease.pending_operation_id, None);
        assert!(runtime.pending.is_none());
        assert!(!runtime.operation_coordinator.has_unresolved());
        assert_eq!(
            runtime
                .operation_coordinator
                .operation(operation_id)
                .unwrap()
                .phase,
            OperationPhase::Aborted
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_completed_but_lease_persist_failed_is_repaired_on_restart() {
        let root = std::env::temp_dir().join(format!("fcp-lease-projection-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let group_id = *app.groups.keys().next().unwrap();
        let operation_id = Uuid::new_v4();
        {
            let runtime = app.groups.get_mut(&group_id).unwrap();
            runtime
                .operation_coordinator
                .begin_v6(
                    operation_id,
                    OperationKind::Eviction,
                    None,
                    "manual".into(),
                    0,
                )
                .unwrap();
            runtime.lease.state = GroupState::Evicting;
            runtime.lease.pending_operation_id = Some(operation_id);
            runtime.lease.advance_transition().unwrap();
            runtime.lease_store.persist(&runtime.lease).unwrap();
            let prepared = prepared_test_vault(group_id, 1);
            runtime
                .operation_coordinator
                .commit_snapshot(
                    operation_id,
                    b"synthetic snapshot".to_vec(),
                    0,
                    prepared,
                    &runtime.transactions,
                )
                .unwrap();
            runtime
                .operation_coordinator
                .mark_external_result_pending(operation_id)
                .unwrap();
            runtime
                .operation_coordinator
                .complete(operation_id)
                .unwrap();
            runtime.lease.state = GroupState::Sealed;
            runtime.lease.pending_operation_id = None;
            runtime.lease.vault_sequence = 1;
            runtime.lease.advance_transition().unwrap();
            let _failure = fail_next(FailurePoint::LeaseBeforePersist);
            assert!(runtime.lease_store.persist(&runtime.lease).is_err());
        }
        drop(app);

        let reopened = NativeHostApp::open(&paths).unwrap();
        let lease = &reopened.groups[&group_id].lease;
        assert_eq!(lease.state, GroupState::Sealed);
        assert_eq!(lease.pending_operation_id, None);
        assert_eq!(lease.vault_sequence, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nonterminal_committed_operation_is_recovered_degraded_and_blocks_new_lease() {
        let root = std::env::temp_dir().join(format!("fcp-startup-operation-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = handshaken(&paths);
        let group_id = *app.groups.keys().next().unwrap();
        let operation_id = Uuid::new_v4();
        {
            let runtime = app.groups.get_mut(&group_id).unwrap();
            runtime
                .operation_coordinator
                .begin_v6(
                    operation_id,
                    OperationKind::Eviction,
                    None,
                    "manual".into(),
                    0,
                )
                .unwrap();
            runtime.lease.state = GroupState::Evicting;
            runtime.lease.pending_operation_id = Some(operation_id);
            runtime.lease.advance_transition().unwrap();
            runtime.lease_store.persist(&runtime.lease).unwrap();
            let prepared = prepared_test_vault(group_id, 1);
            runtime
                .operation_coordinator
                .commit_snapshot(
                    operation_id,
                    b"synthetic snapshot".to_vec(),
                    0,
                    prepared,
                    &runtime.transactions,
                )
                .unwrap();
        }
        drop(app);

        let mut reopened = handshaken(&paths);
        assert_eq!(reopened.groups[&group_id].lease.state, GroupState::Degraded);
        assert_eq!(
            reopened.groups[&group_id]
                .operation_coordinator
                .journal()
                .operation(operation_id)
                .unwrap()
                .phase,
            OperationPhase::Committed
        );
        let output = reopened
            .handle(Message::LeaseRequest(LeaseRequest {
                account_group_id: group_id,
                purpose: LeasePurpose::Inject,
            }))
            .unwrap();
        assert!(matches!(
            output.as_slice(),
            [Message::LeaseDeny(LeaseDeny { reason, .. })] if reason == "operation_recovery_required"
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_or_corrupted_snapshot_key_with_a_journal_fails_closed() {
        for label in ["missing", "corrupt"] {
            let root = std::env::temp_dir().join(format!("fcp-key-{label}-{}", Uuid::new_v4()));
            let paths = test_paths(&root);
            let mut app = handshaken(&paths);
            let group_id = *app.groups.keys().next().unwrap();
            app.groups
                .get_mut(&group_id)
                .unwrap()
                .operation_coordinator
                .begin_v6(
                    Uuid::new_v4(),
                    OperationKind::Eviction,
                    None,
                    "manual".into(),
                    0,
                )
                .unwrap();
            drop(app);
            if label == "missing" {
                fs::remove_file(&paths.snapshot_integrity_key).unwrap();
            } else {
                fs::write(&paths.snapshot_integrity_key, b"not-dpapi").unwrap();
            }
            assert!(NativeHostApp::open(&paths).is_err());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn v6_handshake_shape_does_not_expose_internal_operation_journal_fields() {
        let root = std::env::temp_dir().join(format!("fcp-v6-shape-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let mut app = NativeHostApp::open(&paths).unwrap();
        let output = app
            .handle(Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: PRODUCT_EXTENSION_ID.into(),
                profile_id: Uuid::new_v4(),
                extension_version: MIN_EXTENSION_VERSION.into(),
                min_host_version: env!("CARGO_PKG_VERSION").into(),
                capabilities: PROTOCOL_CAPABILITIES
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                cached_config_digest: None,
            }))
            .unwrap();
        let encoded = serde_json::to_value(&output[0]).unwrap();
        assert_eq!(encoded["payload"]["protocol_version"], PROTOCOL_VERSION);
        assert!(encoded["payload"].get("active_operation").is_none());
        assert!(
            encoded["payload"]["groups"]
                .as_array()
                .unwrap()
                .iter()
                .all(|group| group.get("active_operation").is_none())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_catalog_lists_only_other_nonempty_profiles_without_domains() {
        let base = std::env::temp_dir().join(format!("fcp-recovery-catalog-{}", Uuid::new_v4()));
        let current_id = Uuid::new_v4();
        let old_id = Uuid::new_v4();
        let current = DataPaths::from_root(base.join("profiles").join(current_id.to_string()));
        let old = DataPaths::from_root(base.join("profiles").join(old_id.to_string()));
        for paths in [&current, &old] {
            fs::create_dir_all(paths.account_groups_config.parent().unwrap()).unwrap();
        }
        fs::write(
            &current.account_groups_config,
            r#"{"version":3,"compatibility_version":3,"groups":[]}"#,
        )
        .unwrap();
        fs::write(
            &old.account_groups_config,
            r#"{"version":3,"compatibility_version":3,"groups":[{"id":"33333333-3333-4333-8333-333333333333","display_name":"Private Name","scope":"secret.example","policy_level":"balanced","store_policy":"normal_profile"}]}"#,
        )
        .unwrap();
        fs::write(&old.hello_credential, b"credential-present").unwrap();

        let app = NativeHostApp::open(&current).unwrap();
        let candidates = app.recovery_candidates(current_id).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].profile_id, old_id);
        assert_eq!(candidates[0].site_count, 1);
        let encoded = serde_json::to_string(&candidates).unwrap();
        assert!(!encoded.contains("secret.example"));
        assert!(!encoded.contains("Private Name"));
        fs::remove_dir_all(base).unwrap();
    }
}
