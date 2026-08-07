use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::atomic_file;
use crate::lease::state_machine::{CapabilityLedger, CapabilityLedgerStore};
use crate::{FcpError, FcpResult};

pub struct FileCapabilityLedgerStore {
    path: PathBuf,
}

impl FileCapabilityLedgerStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> FcpResult<CapabilityLedger> {
        let bytes = fs::read(&self.path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn load_or_initialize(&mut self, group_id: Uuid) -> FcpResult<CapabilityLedger> {
        if self.path.exists() {
            let ledger = self.load()?;
            if ledger.account_group_id != group_id {
                return Err(FcpError::Capability(
                    "persisted capability ledger belongs to another group".into(),
                ));
            }
            return Ok(ledger);
        }
        let ledger = CapabilityLedger::new(group_id)?;
        self.persist(&ledger)?;
        Ok(ledger)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl CapabilityLedgerStore for FileCapabilityLedgerStore {
    fn persist(&mut self, snapshot: &CapabilityLedger) -> FcpResult<()> {
        let bytes = serde_json::to_vec(snapshot)?;
        atomic_file::write_verified(&self.path, &bytes, |persisted| {
            let verified: CapabilityLedger = serde_json::from_slice(persisted)?;
            if &verified != snapshot {
                return Err(FcpError::Capability(
                    "capability ledger read-back mismatch".into(),
                ));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::capability::CapabilityVerifier;
    use crate::protocol::messages::{CapabilityOperation, SignedCapability};

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

    #[test]
    fn consumed_capability_state_survives_reload() {
        let root = std::env::temp_dir().join(format!("fcp-ledger-test-{}", Uuid::new_v4()));
        let path = root.join("capability-ledger.json");
        let group = Uuid::new_v4();
        let mut store = FileCapabilityLedgerStore::new(&path);
        let mut ledger = store.load_or_initialize(group).unwrap();
        let reserved = ledger
            .reserve(CapabilityOperation::Inject, 100, 500, &mut store)
            .unwrap();
        let signed = SignedCapability {
            signature: reserved.canonical_bytes(),
            authenticator_data: Vec::new(),
            payload: reserved,
        };
        ledger
            .verify_and_consume(
                &signed,
                CapabilityOperation::Inject,
                200,
                &CanonicalVerifier,
                &mut store,
            )
            .unwrap();
        let reloaded = store.load().unwrap();
        assert_eq!(reloaded.last_consumed_sequence, 1);
        assert!(reloaded.pending.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
