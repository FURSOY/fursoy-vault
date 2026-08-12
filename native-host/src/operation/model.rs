use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;

use crate::{FcpError, FcpResult};

pub(crate) const OPERATION_JOURNAL_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationKind {
    Enrollment,
    Eviction,
    Reconciliation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationPhase {
    NotCommitted,
    DurabilityUnknown,
    Committed,
    BrowserRemovalPending,
    Completed,
    Aborted,
    ReconciliationRequired,
}

impl OperationPhase {
    pub(crate) fn can_transition_to(self, next: Self) -> bool {
        use OperationPhase::*;
        matches!(
            (self, next),
            (
                NotCommitted,
                DurabilityUnknown | Committed | Aborted | ReconciliationRequired
            ) | (
                DurabilityUnknown,
                NotCommitted | Committed | ReconciliationRequired
            ) | (Committed, BrowserRemovalPending | ReconciliationRequired)
                | (BrowserRemovalPending, Completed | ReconciliationRequired)
        )
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Aborted | Self::ReconciliationRequired
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Digest32(pub(crate) [u8; 32]);

impl Digest32 {
    pub(crate) fn sha256(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationRequest {
    pub(crate) operation_id: Uuid,
    pub(crate) attempt_id: Option<Uuid>,
    pub(crate) kind: OperationKind,
    pub(crate) lease_id: Option<Uuid>,
    pub(crate) reason_code: String,
    pub(crate) base_vault_sequence: u64,
}

impl OperationRequest {
    pub(crate) fn validate(&self) -> FcpResult<()> {
        if self.operation_id.is_nil() {
            return Err(FcpError::Protocol("operation id must not be nil".into()));
        }
        if self.attempt_id.is_some_and(|value| value.is_nil()) {
            return Err(FcpError::Protocol(
                "operation attempt id must not be nil".into(),
            ));
        }
        if self.reason_code.is_empty()
            || self.reason_code.len() > 64
            || !self
                .reason_code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(FcpError::Protocol(
                "operation reason code is not canonical".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn fingerprint(&self, account_group_id: Uuid) -> FcpResult<Digest32> {
        self.validate()?;
        #[derive(Serialize)]
        struct BoundRequest<'a> {
            domain: &'static str,
            account_group_id: Uuid,
            request: &'a OperationRequest,
        }
        Ok(Digest32::sha256(&serde_json::to_vec(&BoundRequest {
            domain: "FCP-OP-REQUEST-1",
            account_group_id,
            request: self,
        })?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotBinding {
    pub(crate) snapshot_tag: Digest32,
    pub(crate) previous_vault_digest: Option<Digest32>,
    pub(crate) target_vault_digest: Digest32,
    pub(crate) target_vault_sequence: u64,
    pub(crate) cookie_count: u32,
    pub(crate) aggregate_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JournalOperation {
    pub(crate) operation_id: Uuid,
    #[serde(default)]
    pub(crate) attempt_id: Option<Uuid>,
    pub(crate) sequence: u64,
    pub(crate) request_fingerprint: Digest32,
    pub(crate) kind: OperationKind,
    pub(crate) lease_id: Option<Uuid>,
    pub(crate) reason_code: String,
    pub(crate) base_vault_sequence: u64,
    pub(crate) phase: OperationPhase,
    pub(crate) snapshot: Option<SnapshotBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationJournal {
    pub(crate) version: u16,
    pub(crate) account_group_id: Uuid,
    pub(crate) sequence_high_water: u64,
    pub(crate) operations: Vec<JournalOperation>,
}

impl OperationJournal {
    pub(crate) fn empty(account_group_id: Uuid) -> Self {
        Self {
            version: OPERATION_JOURNAL_VERSION,
            account_group_id,
            sequence_high_water: 0,
            operations: Vec::new(),
        }
    }

    pub(crate) fn validate(&self, expected_group: Uuid) -> FcpResult<()> {
        if self.version != OPERATION_JOURNAL_VERSION || self.account_group_id != expected_group {
            return Err(FcpError::Format(
                "operation journal identity or version mismatch".into(),
            ));
        }
        let mut previous_sequence = 0;
        let mut operation_ids = HashSet::new();
        for operation in &self.operations {
            if operation.sequence <= previous_sequence
                || operation.sequence > self.sequence_high_water
                || operation.operation_id.is_nil()
                || !operation_ids.insert(operation.operation_id)
            {
                return Err(FcpError::Format(
                    "operation journal sequence is invalid".into(),
                ));
            }
            let request = OperationRequest {
                operation_id: operation.operation_id,
                attempt_id: operation.attempt_id,
                kind: operation.kind,
                lease_id: operation.lease_id,
                reason_code: operation.reason_code.clone(),
                base_vault_sequence: operation.base_vault_sequence,
            };
            if request.fingerprint(expected_group)? != operation.request_fingerprint {
                return Err(FcpError::Format(
                    "operation journal request fingerprint mismatch".into(),
                ));
            }
            if matches!(
                operation.phase,
                OperationPhase::Committed
                    | OperationPhase::BrowserRemovalPending
                    | OperationPhase::Completed
            ) && operation.snapshot.is_none()
            {
                return Err(FcpError::Format(
                    "committed operation is missing its snapshot binding".into(),
                ));
            }
            previous_sequence = operation.sequence;
        }
        if previous_sequence != self.sequence_high_water {
            return Err(FcpError::Format(
                "operation journal high-water mark mismatch".into(),
            ));
        }
        Ok(())
    }
}
