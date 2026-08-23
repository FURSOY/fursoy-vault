use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::capability::{CapabilityVerifier, new_payload};
use crate::protocol::messages::{
    CAPABILITY_MAX_LIFETIME_MS, CapabilityOperation, CapabilityPayload, Nonce32, SignedCapability,
};
use crate::{FcpError, FcpResult};

const REPLAY_NONCE_WINDOW: usize = 128;

/// Persisted, non-secret authorization metadata. Implementations must atomically replace the old
/// snapshot. A successful consume is returned only after this durable write succeeds, so callers
/// can safely enforce "consume capability, then unwrap" ordering.
pub trait CapabilityLedgerStore {
    fn persist(&mut self, snapshot: &CapabilityLedger) -> FcpResult<()>;
}

/// Linear inject authorization token. It is deliberately non-Clone and must be moved into exactly
/// one vault read transaction after durable capability consumption.
#[derive(Debug)]
pub struct ConsumedCapability {
    account_group_id: Uuid,
    operation: CapabilityOperation,
    monotonic_sequence: u64,
}

impl ConsumedCapability {
    pub fn account_group_id(&self) -> Uuid {
        self.account_group_id
    }

    pub fn operation(&self) -> CapabilityOperation {
        self.operation
    }

    pub fn monotonic_sequence(&self) -> u64 {
        self.monotonic_sequence
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityLedger {
    pub account_group_id: Uuid,
    pub last_issued_sequence: u64,
    pub last_consumed_sequence: u64,
    pub consumed_nonces: VecDeque<Nonce32>,
    pub pending: Option<CapabilityPayload>,
}

impl CapabilityLedger {
    pub fn new(account_group_id: Uuid) -> FcpResult<Self> {
        if account_group_id.is_nil() {
            return Err(FcpError::Capability("account group must not be nil".into()));
        }
        Ok(Self {
            account_group_id,
            last_issued_sequence: 0,
            last_consumed_sequence: 0,
            consumed_nonces: VecDeque::new(),
            pending: None,
        })
    }

    pub fn reserve(
        &mut self,
        operation: CapabilityOperation,
        now_unix_ms: u64,
        lifetime_ms: u64,
        store: &mut impl CapabilityLedgerStore,
    ) -> FcpResult<CapabilityPayload> {
        if self.pending.is_some() {
            return Err(FcpError::Capability(
                "another capability is already pending".into(),
            ));
        }
        let sequence = self
            .last_issued_sequence
            .checked_add(1)
            .ok_or_else(|| FcpError::Capability("monotonic sequence exhausted".into()))?;
        let payload = new_payload(
            self.account_group_id,
            operation,
            now_unix_ms,
            lifetime_ms,
            sequence,
        )?;
        let mut next = self.clone();
        next.last_issued_sequence = sequence;
        next.pending = Some(payload.clone());
        store.persist(&next)?;
        *self = next;
        Ok(payload)
    }

    pub fn cancel_pending(&mut self, store: &mut impl CapabilityLedgerStore) -> FcpResult<()> {
        let mut next = self.clone();
        next.pending = None;
        store.persist(&next)?;
        *self = next;
        Ok(())
    }

    pub fn verify_and_consume(
        &mut self,
        signed: &SignedCapability,
        expected_operation: CapabilityOperation,
        now_unix_ms: u64,
        verifier: &impl CapabilityVerifier,
        store: &mut impl CapabilityLedgerStore,
    ) -> FcpResult<ConsumedCapability> {
        signed.payload.validate_shape()?;
        verifier.verify_signature(signed)?;

        if signed.payload.account_group_id != self.account_group_id {
            return Err(FcpError::Capability("account group mismatch".into()));
        }
        if signed.payload.operation != expected_operation {
            return Err(FcpError::Capability("operation mismatch".into()));
        }
        if signed.payload.expiry_unix_ms < now_unix_ms {
            return Err(FcpError::Capability("capability expired".into()));
        }
        if signed.payload.expiry_unix_ms.saturating_sub(now_unix_ms) > CAPABILITY_MAX_LIFETIME_MS {
            return Err(FcpError::Capability(
                "capability expiry exceeds maximum remaining lifetime".into(),
            ));
        }
        if signed.payload.monotonic_sequence <= self.last_consumed_sequence {
            return Err(FcpError::Capability("sequence replay detected".into()));
        }
        if self.consumed_nonces.contains(&signed.payload.nonce) {
            return Err(FcpError::Capability("nonce replay detected".into()));
        }
        let pending = self
            .pending
            .as_ref()
            .ok_or_else(|| FcpError::Capability("no capability is pending".into()))?;
        if pending != &signed.payload {
            return Err(FcpError::Capability(
                "signed payload does not match the reserved transition".into(),
            ));
        }

        let mut next = self.clone();
        next.last_consumed_sequence = signed.payload.monotonic_sequence;
        next.pending = None;
        next.consumed_nonces.push_back(signed.payload.nonce);
        while next.consumed_nonces.len() > REPLAY_NONCE_WINDOW {
            next.consumed_nonces.pop_front();
        }

        // This durable persistence is deliberately before the in-memory commit and before the
        // caller is allowed to invoke PlatformKek::unwrap_dek.
        store.persist(&next)?;
        *self = next;
        Ok(ConsumedCapability {
            account_group_id: signed.payload.account_group_id,
            operation: signed.payload.operation,
            monotonic_sequence: signed.payload.monotonic_sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::capability::CapabilityVerifier;

    #[derive(Default)]
    struct MemoryStore {
        persisted: Vec<CapabilityLedger>,
    }

    impl CapabilityLedgerStore for MemoryStore {
        fn persist(&mut self, snapshot: &CapabilityLedger) -> FcpResult<()> {
            self.persisted.push(snapshot.clone());
            Ok(())
        }
    }

    struct CanonicalVerifier;

    impl CapabilityVerifier for CanonicalVerifier {
        fn verify_signature(&self, capability: &SignedCapability) -> FcpResult<()> {
            if capability.signature == capability.payload.canonical_bytes() {
                Ok(())
            } else {
                Err(FcpError::Capability("test signature mismatch".into()))
            }
        }
    }

    fn signed(payload: CapabilityPayload) -> SignedCapability {
        SignedCapability {
            signature: payload.canonical_bytes(),
            proof_context: Vec::new(),
            payload,
        }
    }

    #[test]
    fn capability_is_single_use_and_persisted_consumed_before_return() {
        let group = Uuid::from_u128(44);
        let mut ledger = CapabilityLedger::new(group).unwrap();
        let mut store = MemoryStore::default();
        let payload = ledger
            .reserve(CapabilityOperation::Inject, 1_000, 5_000, &mut store)
            .unwrap();
        let capability = signed(payload);

        ledger
            .verify_and_consume(
                &capability,
                CapabilityOperation::Inject,
                2_000,
                &CanonicalVerifier,
                &mut store,
            )
            .unwrap();
        assert_eq!(ledger.last_consumed_sequence, 1);
        assert!(ledger.pending.is_none());
        assert_eq!(store.persisted.last(), Some(&ledger));

        assert!(
            ledger
                .verify_and_consume(
                    &capability,
                    CapabilityOperation::Inject,
                    2_001,
                    &CanonicalVerifier,
                    &mut store,
                )
                .is_err()
        );
    }

    #[test]
    fn changing_any_bound_transition_field_is_rejected() {
        let group = Uuid::from_u128(45);
        let mut ledger = CapabilityLedger::new(group).unwrap();
        let mut store = MemoryStore::default();
        let payload = ledger
            .reserve(CapabilityOperation::Inject, 10_000, 2_000, &mut store)
            .unwrap();
        let mut changed = payload;
        changed.expiry_unix_ms += 1;
        let capability = signed(changed);
        assert!(
            ledger
                .verify_and_consume(
                    &capability,
                    CapabilityOperation::Inject,
                    10_100,
                    &CanonicalVerifier,
                    &mut store,
                )
                .is_err()
        );
        assert!(ledger.pending.is_some());
    }

    #[test]
    fn crash_recovery_cancels_reservation_without_reusing_sequence() {
        let group = Uuid::from_u128(46);
        let mut ledger = CapabilityLedger::new(group).unwrap();
        let mut store = MemoryStore::default();
        let abandoned = ledger
            .reserve(CapabilityOperation::Inject, 20_000, 2_000, &mut store)
            .unwrap();
        ledger.cancel_pending(&mut store).unwrap();
        let replacement = ledger
            .reserve(CapabilityOperation::Inject, 21_000, 2_000, &mut store)
            .unwrap();

        assert_eq!(abandoned.monotonic_sequence, 1);
        assert_eq!(replacement.monotonic_sequence, 2);
        assert_ne!(abandoned.nonce, replacement.nonce);
    }
}
