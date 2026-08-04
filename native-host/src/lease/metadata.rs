use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::atomic_file;
use crate::protocol::messages::GroupState;
use crate::{FcpError, FcpResult};

pub const LEASE_METADATA_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseMetadata {
    pub version: u16,
    pub account_group_id: Uuid,
    pub state: GroupState,
    pub lease_id: Option<Uuid>,
    pub granted_at_unix_ms: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    pub pending_operation_id: Option<Uuid>,
    pub vault_sequence: u64,
    pub transition_sequence: u64,
}

impl LeaseMetadata {
    pub fn initial(account_group_id: Uuid, vault_exists: bool) -> Self {
        Self {
            version: LEASE_METADATA_VERSION,
            account_group_id,
            state: if vault_exists {
                GroupState::Sealed
            } else {
                GroupState::Uninitialized
            },
            lease_id: None,
            granted_at_unix_ms: None,
            expires_at_unix_ms: None,
            pending_operation_id: None,
            vault_sequence: 0,
            transition_sequence: 0,
        }
    }

    pub fn validate(&self, expected_group: Uuid) -> FcpResult<()> {
        if self.version != LEASE_METADATA_VERSION {
            return Err(FcpError::Format(format!(
                "unsupported lease metadata version {}",
                self.version
            )));
        }
        if self.account_group_id != expected_group {
            return Err(FcpError::Format(
                "lease metadata account group mismatch".into(),
            ));
        }
        Ok(())
    }

    pub fn advance_transition(&mut self) -> FcpResult<()> {
        self.transition_sequence = self
            .transition_sequence
            .checked_add(1)
            .ok_or_else(|| FcpError::Format("lease transition sequence exhausted".into()))?;
        Ok(())
    }
}

pub struct LeaseMetadataStore {
    path: PathBuf,
}

impl LeaseMetadataStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load_or_initialize(
        &self,
        account_group_id: Uuid,
        vault_exists: bool,
    ) -> FcpResult<LeaseMetadata> {
        if self.path.exists() {
            let metadata: LeaseMetadata = serde_json::from_slice(&fs::read(&self.path)?)?;
            metadata.validate(account_group_id)?;
            if metadata.state == GroupState::Uninitialized && vault_exists {
                return Err(FcpError::Format(
                    "vault exists while lease metadata is uninitialized".into(),
                ));
            }
            if metadata.state != GroupState::Uninitialized && !vault_exists {
                // Crash-safe completion for session invalidation: vault deletion is authoritative.
                // If the process died before persisting Uninitialized, no secret remains to lease.
                let reset = LeaseMetadata::initial(account_group_id, false);
                self.persist(&reset)?;
                return Ok(reset);
            }
            return Ok(metadata);
        }
        let metadata = LeaseMetadata::initial(account_group_id, vault_exists);
        self.persist(&metadata)?;
        Ok(metadata)
    }

    pub fn persist(&self, metadata: &LeaseMetadata) -> FcpResult<()> {
        metadata.validate(metadata.account_group_id)?;
        let bytes = serde_json::to_vec(metadata)?;
        atomic_file::write_verified(&self.path, &bytes, |persisted| {
            let verified: LeaseMetadata = serde_json::from_slice(persisted)?;
            if &verified != metadata {
                return Err(FcpError::Format("lease metadata read-back mismatch".into()));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_vault_repairs_stale_lease_to_uninitialized() {
        let root = std::env::temp_dir().join(format!("fcp-lease-metadata-test-{}", Uuid::new_v4()));
        let store = LeaseMetadataStore::new(root.join("lease.json"));
        let group_id = Uuid::new_v4();
        let sealed = store.load_or_initialize(group_id, true).unwrap();
        assert_eq!(sealed.state, GroupState::Sealed);

        let repaired = store.load_or_initialize(group_id, false).unwrap();
        assert_eq!(repaired.state, GroupState::Uninitialized);
        assert_eq!(repaired.lease_id, None);

        fs::remove_dir_all(root).unwrap();
    }
}
