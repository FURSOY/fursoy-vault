use std::fs;
use std::path::PathBuf;

use uuid::Uuid;

use super::model::{
    JournalOperation, OperationJournal, OperationKind, OperationPhase, OperationRequest,
    SnapshotBinding,
};
use crate::atomic_file::{
    DurableWriteResult, ObjectClassification, ObjectDigest, classify_object, write_verified_durable,
};
use crate::{FcpError, FcpResult};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BeginResult {
    Created(JournalOperation),
    Duplicate(JournalOperation),
}

pub(crate) struct OperationJournalStore {
    path: PathBuf,
    account_group_id: Uuid,
    journal: OperationJournal,
}

impl OperationJournalStore {
    pub(crate) fn open(path: impl Into<PathBuf>, account_group_id: Uuid) -> FcpResult<Self> {
        let path = path.into();
        let journal = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            OperationJournal::empty(account_group_id)
        };
        journal.validate(account_group_id)?;
        Ok(Self {
            path,
            account_group_id,
            journal,
        })
    }

    pub(crate) fn journal(&self) -> &OperationJournal {
        &self.journal
    }

    pub(crate) fn operation(&self, operation_id: Uuid) -> Option<&JournalOperation> {
        self.journal
            .operations
            .iter()
            .find(|operation| operation.operation_id == operation_id)
    }

    pub(crate) fn begin(&mut self, request: OperationRequest) -> FcpResult<BeginResult> {
        let fingerprint = request.fingerprint(self.account_group_id)?;
        if let Some(existing) = self.operation(request.operation_id) {
            if existing.request_fingerprint == fingerprint {
                return Ok(BeginResult::Duplicate(existing.clone()));
            }
            return Err(FcpError::Protocol(
                "operation id was reused with a different request payload".into(),
            ));
        }
        let sequence = self
            .journal
            .sequence_high_water
            .checked_add(1)
            .ok_or_else(|| FcpError::Format("operation sequence exhausted".into()))?;
        let operation = JournalOperation {
            operation_id: request.operation_id,
            attempt_id: request.attempt_id,
            sequence,
            request_fingerprint: fingerprint,
            kind: request.kind,
            lease_id: request.lease_id,
            reason_code: request.reason_code,
            base_vault_sequence: request.base_vault_sequence,
            phase: OperationPhase::NotCommitted,
            snapshot: None,
        };
        let mut next = self.journal.clone();
        next.sequence_high_water = sequence;
        next.operations.push(operation.clone());
        self.persist(next)?;
        Ok(BeginResult::Created(operation))
    }

    pub(crate) fn issue(
        &mut self,
        kind: OperationKind,
        lease_id: Option<Uuid>,
        reason_code: String,
        base_vault_sequence: u64,
    ) -> FcpResult<JournalOperation> {
        let request = OperationRequest {
            operation_id: Uuid::new_v4(),
            attempt_id: None,
            kind,
            lease_id,
            reason_code,
            base_vault_sequence,
        };
        match self.begin(request)? {
            BeginResult::Created(operation) => Ok(operation),
            BeginResult::Duplicate(_) => Err(FcpError::Format(
                "newly generated operation id collided with the journal".into(),
            )),
        }
    }

    pub(crate) fn bind_snapshot(
        &mut self,
        operation_id: Uuid,
        binding: SnapshotBinding,
    ) -> FcpResult<()> {
        let mut next = self.journal.clone();
        let operation = find_mut(&mut next, operation_id)?;
        if operation.phase != OperationPhase::NotCommitted
            && operation.phase != OperationPhase::DurabilityUnknown
        {
            return Err(FcpError::Protocol(
                "snapshot cannot be bound in the current operation phase".into(),
            ));
        }
        if let Some(existing) = &operation.snapshot {
            if existing == &binding {
                return Ok(());
            }
            return Err(FcpError::Protocol(
                "operation snapshot payload conflicts with the durable binding".into(),
            ));
        }
        operation.snapshot = Some(binding);
        self.persist(next)
    }

    pub(crate) fn transition(
        &mut self,
        operation_id: Uuid,
        next_phase: OperationPhase,
    ) -> FcpResult<()> {
        let mut next = self.journal.clone();
        let operation = find_mut(&mut next, operation_id)?;
        if operation.phase == next_phase {
            return Ok(());
        }
        if !operation.phase.can_transition_to(next_phase) {
            return Err(FcpError::Protocol(format!(
                "illegal operation phase transition from {:?} to {:?}",
                operation.phase, next_phase
            )));
        }
        if next_phase == OperationPhase::Committed && operation.snapshot.is_none() {
            return Err(FcpError::Protocol(
                "operation cannot commit without a snapshot binding".into(),
            ));
        }
        operation.phase = next_phase;
        self.persist(next)
    }

    fn persist(&mut self, next: OperationJournal) -> FcpResult<()> {
        next.validate(self.account_group_id)?;
        let target_bytes = serde_json::to_vec(&next)?;
        let previous_bytes = if self.path.exists() {
            Some(fs::read(&self.path)?)
        } else {
            None
        };
        match write_verified_durable(&self.path, &target_bytes, |persisted| {
            let decoded: OperationJournal = serde_json::from_slice(persisted)?;
            decoded.validate(self.account_group_id)?;
            if decoded != next {
                return Err(FcpError::Format(
                    "operation journal read-back mismatch".into(),
                ));
            }
            Ok(())
        }) {
            Ok(DurableWriteResult::Committed) => {
                self.journal = next;
                Ok(())
            }
            Ok(other) => Err(FcpError::Format(format!(
                "unexpected successful durable write result {other:?}"
            ))),
            Err(failure) if failure.result == DurableWriteResult::DurabilityUnknown => {
                let classification = classify_object(
                    &self.path,
                    previous_bytes.as_deref().map(ObjectDigest::of),
                    ObjectDigest::of(&target_bytes),
                )?;
                match classification {
                    ObjectClassification::Target => {
                        self.journal = next;
                        Ok(())
                    }
                    ObjectClassification::Previous | ObjectClassification::Missing => {
                        Err(failure.error)
                    }
                    ObjectClassification::Neither => Err(FcpError::Format(
                        "operation journal durability could not be classified".into(),
                    )),
                }
            }
            Err(failure) => Err(failure.error),
        }
    }
}

fn find_mut(
    journal: &mut OperationJournal,
    operation_id: Uuid,
) -> FcpResult<&mut JournalOperation> {
    journal
        .operations
        .iter_mut()
        .find(|operation| operation.operation_id == operation_id)
        .ok_or_else(|| FcpError::Protocol("unknown operation id".into()))
}
