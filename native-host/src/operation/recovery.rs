use std::path::Path;

use uuid::Uuid;

use super::journal::OperationJournalStore;
use super::model::{Digest32, OperationPhase};
use crate::atomic_file::{ObjectClassification, ObjectDigest, classify_object};
use crate::{FcpError, FcpResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryClassification {
    NotCommitted,
    Committed,
    ReconciliationRequired,
}

impl OperationJournalStore {
    pub(crate) fn classify_vault(
        &self,
        operation_id: Uuid,
        vault_path: &Path,
    ) -> FcpResult<RecoveryClassification> {
        let operation = self
            .operation(operation_id)
            .ok_or_else(|| FcpError::Protocol("unknown operation id".into()))?;
        let snapshot = operation.snapshot.as_ref().ok_or_else(|| {
            FcpError::Protocol("operation has no durable snapshot binding".into())
        })?;
        let precommit_phase = matches!(
            operation.phase,
            OperationPhase::NotCommitted | OperationPhase::DurabilityUnknown
        );
        match classify_object(
            vault_path,
            snapshot.previous_vault_digest.map(object_digest),
            object_digest(snapshot.target_vault_digest),
        )? {
            ObjectClassification::Previous if precommit_phase => {
                Ok(RecoveryClassification::NotCommitted)
            }
            ObjectClassification::Target => Ok(RecoveryClassification::Committed),
            ObjectClassification::Missing
                if precommit_phase && snapshot.previous_vault_digest.is_none() =>
            {
                Ok(RecoveryClassification::NotCommitted)
            }
            ObjectClassification::Previous
            | ObjectClassification::Missing
            | ObjectClassification::Neither => Ok(RecoveryClassification::ReconciliationRequired),
        }
    }

    pub(crate) fn recover_vault_phase(
        &mut self,
        operation_id: Uuid,
        vault_path: &Path,
    ) -> FcpResult<RecoveryClassification> {
        let classification = self.classify_vault(operation_id, vault_path)?;
        let phase = self.operation(operation_id).unwrap().phase;
        let target_phase = match classification {
            RecoveryClassification::NotCommitted => OperationPhase::NotCommitted,
            RecoveryClassification::Committed
                if matches!(
                    phase,
                    OperationPhase::Committed | OperationPhase::BrowserRemovalPending
                ) =>
            {
                phase
            }
            RecoveryClassification::Committed => OperationPhase::Committed,
            RecoveryClassification::ReconciliationRequired => {
                OperationPhase::ReconciliationRequired
            }
        };
        if phase != target_phase {
            self.transition(operation_id, target_phase)?;
        }
        Ok(classification)
    }
}

fn object_digest(value: Digest32) -> ObjectDigest {
    ObjectDigest::from_bytes(value.0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::operation::{
        BeginResult, Digest32, JournalOperation, OperationKind, OperationRequest, SnapshotBinding,
        SnapshotTagContext, SnapshotTagger,
    };
    use crate::test_support::{FailurePoint, fail_next};

    fn root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("fcp-operation-{label}-{}", Uuid::new_v4()))
    }

    fn request(operation_id: Uuid, reason: &str) -> OperationRequest {
        OperationRequest {
            operation_id,
            attempt_id: None,
            kind: OperationKind::Eviction,
            lease_id: Some(Uuid::new_v4()),
            reason_code: reason.into(),
            base_vault_sequence: 7,
        }
    }

    fn binding(previous: &[u8], target: &[u8], operation: &JournalOperation) -> SnapshotBinding {
        let tagger = SnapshotTagger::for_test([9; 32]);
        let tag = tagger.tag(
            &SnapshotTagContext {
                profile_id: Uuid::nil(),
                account_group_id: Uuid::nil(),
                operation_id: operation.operation_id,
                operation_sequence: operation.sequence,
                base_vault_sequence: operation.base_vault_sequence,
            },
            b"synthetic canonical snapshot",
        );
        SnapshotBinding {
            snapshot_tag: tag,
            previous_vault_digest: Some(Digest32::sha256(previous)),
            target_vault_digest: Digest32::sha256(target),
            target_vault_sequence: operation.base_vault_sequence + 1,
            cookie_count: 2,
            aggregate_bytes: 128,
        }
    }

    #[test]
    fn journal_accepts_only_forward_valid_transitions() {
        let root = root("transitions");
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(root.join("journal.json"), group_id).unwrap();
        let operation = match store.begin(request(operation_id, "manual")).unwrap() {
            BeginResult::Created(operation) => operation,
            BeginResult::Duplicate(_) => unreachable!(),
        };
        store
            .bind_snapshot(operation_id, binding(b"old", b"new", &operation))
            .unwrap();
        store
            .transition(operation_id, OperationPhase::Committed)
            .unwrap();
        store
            .transition(operation_id, OperationPhase::BrowserRemovalPending)
            .unwrap();
        store
            .transition(operation_id, OperationPhase::Completed)
            .unwrap();
        assert!(store.operation(operation_id).unwrap().phase.is_terminal());
        assert!(
            store
                .transition(operation_id, OperationPhase::NotCommitted)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn durability_unknown_can_only_be_classified_forward_or_back_to_not_committed() {
        let root = root("durability-transitions");
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(root.join("journal.json"), group_id).unwrap();
        let operation = match store.begin(request(operation_id, "manual")).unwrap() {
            BeginResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        store
            .bind_snapshot(operation_id, binding(b"old", b"new", &operation))
            .unwrap();
        store
            .transition(operation_id, OperationPhase::DurabilityUnknown)
            .unwrap();
        store
            .transition(operation_id, OperationPhase::NotCommitted)
            .unwrap();
        store
            .transition(operation_id, OperationPhase::DurabilityUnknown)
            .unwrap();
        store
            .transition(operation_id, OperationPhase::Committed)
            .unwrap();
        assert!(
            store
                .transition(operation_id, OperationPhase::DurabilityUnknown)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_request_is_cached_but_same_id_with_different_payload_conflicts() {
        let root = root("duplicates");
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(root.join("journal.json"), group_id).unwrap();
        let original = request(operation_id, "manual");
        assert!(matches!(
            store.begin(original.clone()).unwrap(),
            BeginResult::Created(_)
        ));
        assert!(matches!(
            store.begin(original).unwrap(),
            BeginResult::Duplicate(_)
        ));
        assert!(store.begin(request(operation_id, "locked")).is_err());
        let second_id = Uuid::new_v4();
        let second = match store.begin(request(second_id, "manual")).unwrap() {
            BeginResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        assert_eq!(second.sequence, 2);
        assert_eq!(store.journal().sequence_high_water, 2);
        assert_eq!(store.journal().operations.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_issues_non_nil_ids_with_monotonic_sequences() {
        let root = root("issued-identities");
        let group_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(root.join("journal.json"), group_id).unwrap();
        let first = store
            .issue(OperationKind::Eviction, None, "manual".into(), 2)
            .unwrap();
        let second = store
            .issue(
                OperationKind::Reconciliation,
                None,
                "startup_reconciliation".into(),
                2,
            )
            .unwrap();
        assert!(!first.operation_id.is_nil());
        assert!(!second.operation_id.is_nil());
        assert_ne!(first.operation_id, second.operation_id);
        assert_eq!((first.sequence, second.sequence), (1, 2));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_rebinding_with_different_payload_is_rejected_without_plaintext_persistence() {
        let root = root("binding");
        let journal_path = root.join("journal.json");
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(&journal_path, group_id).unwrap();
        let operation = match store.begin(request(operation_id, "manual")).unwrap() {
            BeginResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        let first = binding(b"old", b"new", &operation);
        store.bind_snapshot(operation_id, first.clone()).unwrap();
        assert!(store.bind_snapshot(operation_id, first).is_ok());
        assert!(
            store
                .bind_snapshot(operation_id, binding(b"old", b"different", &operation))
                .is_err()
        );
        let bytes = fs::read(journal_path).unwrap();
        assert!(
            !bytes
                .windows(b"synthetic canonical snapshot".len())
                .any(|window| window == b"synthetic canonical snapshot")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn vault_committed_while_journal_lags_is_classified_and_recovered_after_restart() {
        let root = root("journal-lag");
        fs::create_dir_all(&root).unwrap();
        let journal_path = root.join("journal.json");
        let vault_path = root.join("vault.fcpv");
        fs::write(&vault_path, b"old").unwrap();
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(&journal_path, group_id).unwrap();
        let operation = match store.begin(request(operation_id, "manual")).unwrap() {
            BeginResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        store
            .bind_snapshot(operation_id, binding(b"old", b"new", &operation))
            .unwrap();
        fs::write(&vault_path, b"new").unwrap();
        drop(store);

        let mut reopened = OperationJournalStore::open(&journal_path, group_id).unwrap();
        assert_eq!(
            reopened
                .recover_vault_phase(operation_id, &vault_path)
                .unwrap(),
            RecoveryClassification::Committed
        );
        assert_eq!(
            reopened.operation(operation_id).unwrap().phase,
            OperationPhase::Committed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restart_classifies_previous_target_and_neither_objects() {
        for (label, bytes, expected) in [
            (
                "previous",
                b"old".as_slice(),
                RecoveryClassification::NotCommitted,
            ),
            (
                "target",
                b"new".as_slice(),
                RecoveryClassification::Committed,
            ),
            (
                "neither",
                b"other".as_slice(),
                RecoveryClassification::ReconciliationRequired,
            ),
        ] {
            let root = root(label);
            fs::create_dir_all(&root).unwrap();
            let vault_path = root.join("vault.fcpv");
            fs::write(&vault_path, bytes).unwrap();
            let group_id = Uuid::new_v4();
            let operation_id = Uuid::new_v4();
            let mut store =
                OperationJournalStore::open(root.join("journal.json"), group_id).unwrap();
            let operation = match store.begin(request(operation_id, "manual")).unwrap() {
                BeginResult::Created(operation) => operation,
                _ => unreachable!(),
            };
            store
                .bind_snapshot(operation_id, binding(b"old", b"new", &operation))
                .unwrap();
            drop(store);
            let reopened =
                OperationJournalStore::open(root.join("journal.json"), group_id).unwrap();
            assert_eq!(
                reopened.classify_vault(operation_id, &vault_path).unwrap(),
                expected
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn journal_atomic_unknown_is_resolved_by_exact_target_classification() {
        let root = root("journal-unknown");
        let group_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(root.join("journal.json"), group_id).unwrap();
        let _failure = fail_next(FailurePoint::AtomicAfterReplace);

        assert!(matches!(
            store.begin(request(Uuid::new_v4(), "manual")).unwrap(),
            BeginResult::Created(_)
        ));
        assert_eq!(store.journal().sequence_high_water, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_atomic_precommit_failure_is_not_applied_in_memory_or_after_restart() {
        let root = root("journal-not-committed");
        let path = root.join("journal.json");
        let group_id = Uuid::new_v4();
        let mut store = OperationJournalStore::open(&path, group_id).unwrap();
        let _failure = fail_next(FailurePoint::AtomicAfterTempSync);

        assert!(store.begin(request(Uuid::new_v4(), "manual")).is_err());
        assert_eq!(store.journal().sequence_high_water, 0);
        drop(store);
        let reopened = OperationJournalStore::open(path, group_id).unwrap();
        assert_eq!(reopened.journal().sequence_high_water, 0);
        fs::remove_dir_all(root).unwrap();
    }
}
