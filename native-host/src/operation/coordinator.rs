use std::path::Path;

use uuid::Uuid;
use zeroize::Zeroize;

use super::{
    BeginResult, JournalOperation, OperationJournalStore, OperationKind, OperationPhase,
    OperationRequest, RecoveryClassification, SnapshotBinding, SnapshotTagContext, SnapshotTagger,
};
use crate::atomic_file::DurableWriteResult;
use crate::paths::DataPaths;
use crate::transaction::{PreparedVaultWrite, VaultTransactions};
use crate::{FcpError, FcpResult};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StartupRecovery {
    pub(crate) needs_reconciliation: bool,
    pub(crate) committed_operation_id: Option<Uuid>,
    pub(crate) committed_vault_sequence: Option<u64>,
}

pub(crate) struct OperationCoordinator {
    journal: OperationJournalStore,
    tagger: SnapshotTagger,
    profile_id: Option<Uuid>,
}

impl OperationCoordinator {
    pub(crate) fn open(paths: &DataPaths, account_group_id: Uuid) -> FcpResult<Self> {
        let tagger = SnapshotTagger::load_or_create(
            &paths.snapshot_integrity_key,
            paths.operation_journals_exist()?,
        )?;
        Ok(Self {
            journal: OperationJournalStore::open(
                paths.operation_journal_path(account_group_id),
                account_group_id,
            )?,
            tagger,
            profile_id: None,
        })
    }

    pub(crate) fn set_profile_id(&mut self, profile_id: Uuid) -> FcpResult<()> {
        if profile_id.is_nil() {
            return Err(FcpError::Protocol("profile id must not be nil".into()));
        }
        if self
            .profile_id
            .is_some_and(|existing| existing != profile_id)
        {
            return Err(FcpError::Protocol(
                "operation coordinator profile binding changed".into(),
            ));
        }
        self.profile_id = Some(profile_id);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn bound_profile_id(&self) -> Option<Uuid> {
        self.profile_id
    }

    pub(crate) fn begin_v6(
        &mut self,
        operation_id: Uuid,
        kind: OperationKind,
        lease_id: Option<Uuid>,
        reason_code: String,
        base_vault_sequence: u64,
    ) -> FcpResult<JournalOperation> {
        if self.journal.operation(operation_id).is_none() && self.has_unresolved() {
            return Err(FcpError::Protocol(
                "another durable operation is unresolved for this group".into(),
            ));
        }
        match self.journal.begin(OperationRequest {
            operation_id,
            attempt_id: None,
            kind,
            lease_id,
            reason_code,
            base_vault_sequence,
        })? {
            BeginResult::Created(operation) | BeginResult::Duplicate(operation) => Ok(operation),
        }
    }

    pub(crate) fn issue_v7(
        &mut self,
        attempt_id: Uuid,
        kind: OperationKind,
        lease_id: Option<Uuid>,
        reason_code: String,
        base_vault_sequence: u64,
    ) -> FcpResult<JournalOperation> {
        if self.has_unresolved() {
            return Err(FcpError::Protocol(
                "another durable operation is unresolved for this group".into(),
            ));
        }
        let operation_id = Uuid::new_v4();
        match self.journal.begin(OperationRequest {
            operation_id,
            attempt_id: Some(attempt_id),
            kind,
            lease_id,
            reason_code,
            base_vault_sequence,
        })? {
            BeginResult::Created(operation) => Ok(operation),
            BeginResult::Duplicate(_) => {
                Err(FcpError::Format("host-issued operation id collided".into()))
            }
        }
    }

    pub(crate) fn operation(&self, operation_id: Uuid) -> Option<JournalOperation> {
        self.journal.operation(operation_id).cloned()
    }

    pub(crate) fn operation_by_attempt(&self, attempt_id: Uuid) -> Option<JournalOperation> {
        self.journal
            .journal()
            .operations
            .iter()
            .find(|operation| operation.attempt_id == Some(attempt_id))
            .cloned()
    }

    pub(crate) fn sequence_high_water(&self) -> u64 {
        self.journal.journal().sequence_high_water
    }

    pub(crate) fn verify_snapshot(
        &self,
        operation_id: Uuid,
        operation_sequence: u64,
        attempt_id: Uuid,
        mut canonical_snapshot: Vec<u8>,
    ) -> FcpResult<bool> {
        let operation = self
            .journal
            .operation(operation_id)
            .ok_or_else(|| FcpError::Protocol("unknown operation id".into()))?;
        if operation.sequence != operation_sequence || operation.attempt_id != Some(attempt_id) {
            canonical_snapshot.zeroize();
            return Err(FcpError::Protocol(
                "operation sequence or attempt binding mismatch".into(),
            ));
        }
        let profile_id = self.profile_id.ok_or_else(|| {
            FcpError::Protocol("operation coordinator has no handshake profile binding".into())
        })?;
        let expected = operation.snapshot.as_ref().ok_or_else(|| {
            FcpError::Protocol("operation has no committed snapshot binding".into())
        })?;
        let actual = self.tagger.tag(
            &SnapshotTagContext {
                profile_id,
                account_group_id: self.journal.journal().account_group_id,
                operation_id,
                operation_sequence,
                base_vault_sequence: operation.base_vault_sequence,
            },
            &canonical_snapshot,
        );
        canonical_snapshot.zeroize();
        Ok(actual == expected.snapshot_tag)
    }

    pub(crate) fn has_unresolved(&self) -> bool {
        self.journal
            .journal()
            .operations
            .iter()
            .any(|operation| !operation.phase.is_terminal())
    }

    pub(crate) fn prepare_for_v6_reconciliation(&mut self, vault_path: &Path) -> FcpResult<()> {
        let active: Vec<_> = self
            .journal
            .journal()
            .operations
            .iter()
            .filter(|operation| !operation.phase.is_terminal())
            .map(|operation| operation.operation_id)
            .collect();
        for operation_id in active {
            let operation = self.journal.operation(operation_id).unwrap().clone();
            if operation.snapshot.is_some()
                && matches!(
                    operation.phase,
                    OperationPhase::NotCommitted | OperationPhase::DurabilityUnknown
                )
            {
                let _ = self.journal.recover_vault_phase(operation_id, vault_path)?;
            }
            let phase = self.journal.operation(operation_id).unwrap().phase;
            let terminal = if phase == OperationPhase::NotCommitted {
                OperationPhase::Aborted
            } else {
                OperationPhase::ReconciliationRequired
            };
            self.journal.transition(operation_id, terminal)?;
        }
        Ok(())
    }

    pub(crate) fn commit_snapshot(
        &mut self,
        operation_id: Uuid,
        mut canonical_snapshot: Vec<u8>,
        cookie_count: u32,
        prepared: PreparedVaultWrite,
        transactions: &VaultTransactions,
    ) -> FcpResult<u64> {
        let operation = self
            .journal
            .operation(operation_id)
            .ok_or_else(|| FcpError::Protocol("unknown operation id".into()))?
            .clone();
        let profile_id = self.profile_id.ok_or_else(|| {
            FcpError::Protocol("operation coordinator has no handshake profile binding".into())
        })?;
        let snapshot_tag = self.tagger.tag(
            &SnapshotTagContext {
                profile_id,
                account_group_id: self.journal.journal().account_group_id,
                operation_id,
                operation_sequence: operation.sequence,
                base_vault_sequence: operation.base_vault_sequence,
            },
            &canonical_snapshot,
        );
        let aggregate_bytes = u64::try_from(canonical_snapshot.len())
            .map_err(|_| FcpError::Protocol("snapshot byte length exceeds u64".into()))?;
        canonical_snapshot.zeroize();
        let vault_sequence = prepared.vault_sequence();
        self.journal.bind_snapshot(
            operation_id,
            SnapshotBinding {
                snapshot_tag,
                previous_vault_digest: prepared.previous_digest(),
                target_vault_digest: prepared.target_digest(),
                target_vault_sequence: vault_sequence,
                cookie_count,
                aggregate_bytes,
            },
        )?;
        self.journal
            .transition(operation_id, OperationPhase::DurabilityUnknown)?;
        match transactions.commit_prepared(prepared) {
            Ok(DurableWriteResult::Committed) => {
                self.journal
                    .transition(operation_id, OperationPhase::Committed)?;
                Ok(vault_sequence)
            }
            Ok(other) => Err(FcpError::Format(format!(
                "unexpected successful vault write result {other:?}"
            ))),
            Err(failure) if failure.result == DurableWriteResult::DurabilityUnknown => {
                match self
                    .journal
                    .recover_vault_phase(operation_id, &transactions.vault_path())?
                {
                    RecoveryClassification::Committed => Ok(vault_sequence),
                    RecoveryClassification::NotCommitted => Err(failure.error),
                    RecoveryClassification::ReconciliationRequired => Err(FcpError::Format(
                        "vault durability could not be classified".into(),
                    )),
                }
            }
            Err(failure) => {
                self.journal
                    .transition(operation_id, OperationPhase::NotCommitted)?;
                Err(failure.error)
            }
        }
    }

    pub(crate) fn mark_external_result_pending(&mut self, operation_id: Uuid) -> FcpResult<()> {
        self.journal
            .transition(operation_id, OperationPhase::BrowserRemovalPending)
    }

    pub(crate) fn complete(&mut self, operation_id: Uuid) -> FcpResult<()> {
        self.journal
            .transition(operation_id, OperationPhase::Completed)
    }

    pub(crate) fn abort(&mut self, operation_id: Uuid) -> FcpResult<()> {
        self.journal
            .transition(operation_id, OperationPhase::Aborted)
    }

    pub(crate) fn require_reconciliation(&mut self, operation_id: Uuid) -> FcpResult<()> {
        self.journal
            .transition(operation_id, OperationPhase::ReconciliationRequired)
    }

    pub(crate) fn startup_recover(&mut self, vault_path: &Path) -> FcpResult<StartupRecovery> {
        let active: Vec<_> = self
            .journal
            .journal()
            .operations
            .iter()
            .filter(|operation| !operation.phase.is_terminal())
            .map(|operation| operation.operation_id)
            .collect();
        let mut recovery = StartupRecovery::default();
        for operation_id in active {
            recovery.needs_reconciliation = true;
            let operation = self.journal.operation(operation_id).unwrap().clone();
            if operation.snapshot.is_none() {
                self.journal
                    .transition(operation_id, OperationPhase::Aborted)?;
                continue;
            }
            match self.journal.recover_vault_phase(operation_id, vault_path)? {
                RecoveryClassification::NotCommitted => {
                    self.journal
                        .transition(operation_id, OperationPhase::Aborted)?;
                }
                RecoveryClassification::Committed => {
                    let snapshot = self
                        .journal
                        .operation(operation_id)
                        .and_then(|value| value.snapshot.as_ref())
                        .expect("classified operation has snapshot");
                    recovery.committed_operation_id = Some(operation_id);
                    recovery.committed_vault_sequence = Some(snapshot.target_vault_sequence);
                }
                RecoveryClassification::ReconciliationRequired => {}
            }
        }
        Ok(recovery)
    }

    pub(crate) fn completed_projection(&self, operation_id: Uuid) -> Option<JournalOperation> {
        self.journal
            .operation(operation_id)
            .filter(|operation| operation.phase == OperationPhase::Completed)
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn journal(&self) -> &OperationJournalStore {
        &self.journal
    }

    #[cfg(test)]
    pub(crate) fn open_for_test(
        journal_path: impl Into<std::path::PathBuf>,
        account_group_id: Uuid,
    ) -> FcpResult<Self> {
        Ok(Self {
            journal: OperationJournalStore::open(journal_path, account_group_id)?,
            tagger: SnapshotTagger::for_test([41; 32]),
            profile_id: Some(Uuid::new_v4()),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::crypto::aead::{DEK_BYTES, SecretDek};
    use crate::crypto::platform_kek::WRAPPED_DEK_BYTES;
    use crate::lease::store::FileCapabilityLedgerStore;
    use crate::operation::Digest32;
    use crate::test_support::{FailurePoint, fail_on_nth};
    use crate::transaction::PreparedVaultWrite;
    use crate::vault::format::VaultRecord;
    use crate::vault::payload::VaultPayload;
    use crate::vault::store::VaultStore;

    fn root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("fcp-coordinator-{label}-{}", Uuid::new_v4()))
    }

    fn fixture(
        root: &Path,
        group_id: Uuid,
        sequence: u64,
        previous: Option<&[u8]>,
    ) -> (VaultTransactions, PreparedVaultWrite) {
        let dek = SecretDek::from_bytes([23; DEK_BYTES]);
        let mut payload = VaultPayload::empty();
        payload.vault_sequence = sequence;
        let record = VaultRecord::seal(
            group_id,
            [24; 16],
            vec![25; WRAPPED_DEK_BYTES],
            &dek,
            &payload,
        )
        .unwrap();
        let bytes = record.encode().unwrap();
        let prepared = PreparedVaultWrite::for_test(
            group_id,
            bytes,
            dek,
            previous.map(Digest32::sha256),
            sequence,
        );
        let transactions = VaultTransactions::open(
            group_id,
            VaultStore::new(root.join("vault")),
            FileCapabilityLedgerStore::new(root.join("capability.json")),
        )
        .unwrap();
        (transactions, prepared)
    }

    fn begin(coordinator: &mut OperationCoordinator, operation_id: Uuid) {
        coordinator
            .begin_v6(
                operation_id,
                OperationKind::Eviction,
                Some(Uuid::new_v4()),
                "manual".into(),
                0,
            )
            .unwrap();
    }

    #[test]
    fn durability_unknown_at_vault_replace_is_classified_as_committed() {
        let root = root("unknown-vault");
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let mut coordinator =
            OperationCoordinator::open_for_test(root.join("journal.json"), group_id).unwrap();
        begin(&mut coordinator, operation_id);
        let (transactions, prepared) = fixture(&root, group_id, 1, None);
        // bind journal, durability-unknown journal, then vault replace.
        let _failure = fail_on_nth(FailurePoint::AtomicAfterReplace, 3);

        assert_eq!(
            coordinator
                .commit_snapshot(
                    operation_id,
                    b"synthetic snapshot".to_vec(),
                    1,
                    prepared,
                    &transactions,
                )
                .unwrap(),
            1
        );
        assert_eq!(
            coordinator.journal().operation(operation_id).unwrap().phase,
            OperationPhase::Committed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn vault_commit_with_lagging_journal_is_recovered_after_restart() {
        let root = root("journal-lag");
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let journal_path = root.join("journal.json");
        let mut coordinator = OperationCoordinator::open_for_test(&journal_path, group_id).unwrap();
        begin(&mut coordinator, operation_id);
        let (transactions, prepared) = fixture(&root, group_id, 1, None);
        // bind, unknown marker, vault commit, then fail the journal's Committed transition.
        let _failure = fail_on_nth(FailurePoint::AtomicAfterTempSync, 4);
        assert!(
            coordinator
                .commit_snapshot(
                    operation_id,
                    b"synthetic snapshot".to_vec(),
                    1,
                    prepared,
                    &transactions,
                )
                .is_err()
        );
        drop(coordinator);

        let mut reopened = OperationCoordinator::open_for_test(journal_path, group_id).unwrap();
        let recovery = reopened
            .startup_recover(&transactions.vault_path())
            .unwrap();
        assert!(recovery.needs_reconciliation);
        assert_eq!(recovery.committed_operation_id, Some(operation_id));
        assert_eq!(recovery.committed_vault_sequence, Some(1));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v7_attempt_is_host_issued_duplicate_safe_and_payload_bound() {
        let root = root("v7-attempt");
        let group_id = Uuid::new_v4();
        let attempt_id = Uuid::new_v4();
        let lease_id = Uuid::new_v4();
        let mut coordinator =
            OperationCoordinator::open_for_test(root.join("journal.json"), group_id).unwrap();
        let issued = coordinator
            .issue_v7(
                attempt_id,
                OperationKind::Eviction,
                Some(lease_id),
                "locked".into(),
                3,
            )
            .unwrap();
        assert!(!issued.operation_id.is_nil());
        assert_eq!(issued.sequence, 1);
        assert_eq!(
            coordinator.operation_by_attempt(attempt_id).unwrap(),
            issued
        );
        assert!(
            coordinator
                .begin_v6(
                    issued.operation_id,
                    OperationKind::Eviction,
                    Some(lease_id),
                    "different".into(),
                    3
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
