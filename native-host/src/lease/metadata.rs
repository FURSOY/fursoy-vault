use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::atomic_file;
use crate::protocol::messages::GroupState;
use crate::{FcpError, FcpResult};

pub const LEASE_METADATA_VERSION: u16 = 2;
pub const LEGACY_LEASE_METADATA_VERSION: u16 = 1;

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
    #[serde(default = "legacy_protocol_floor")]
    pub protocol_floor: u16,
    #[serde(default)]
    pub operation_sequence_high_water: u64,
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
            protocol_floor: 6,
            operation_sequence_high_water: 0,
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

    pub fn activate_protocol_v7(&mut self, operation_sequence_high_water: u64) -> FcpResult<bool> {
        if self.protocol_floor > 7 {
            return Err(FcpError::Format("unsupported lease protocol floor".into()));
        }
        let changed = self.protocol_floor < 7
            || self.operation_sequence_high_water < operation_sequence_high_water;
        self.protocol_floor = 7;
        self.operation_sequence_high_water = self
            .operation_sequence_high_water
            .max(operation_sequence_high_water);
        Ok(changed)
    }
}

const fn legacy_protocol_floor() -> u16 {
    6
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
            let mut metadata: LeaseMetadata = serde_json::from_slice(&fs::read(&self.path)?)?;
            if metadata.version == LEGACY_LEASE_METADATA_VERSION {
                metadata.version = LEASE_METADATA_VERSION;
                metadata.protocol_floor = 6;
                metadata.operation_sequence_high_water = 0;
                if matches!(
                    metadata.state,
                    GroupState::Unlocking | GroupState::Evicting | GroupState::Degraded
                ) || metadata.pending_operation_id.is_some()
                {
                    metadata.state = GroupState::Degraded;
                    metadata.lease_id = None;
                    metadata.granted_at_unix_ms = None;
                    metadata.expires_at_unix_ms = None;
                }
                metadata.advance_transition()?;
                self.persist(&metadata)?;
            }
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

        #[cfg(test)]
        crate::test_support::check(crate::test_support::FailurePoint::LeaseBeforePersist)?;

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
    use crate::test_support::{FailurePoint, fail_next};

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

    #[test]
    fn injected_lease_persistence_failure_does_not_replace_the_previous_metadata() {
        let root = std::env::temp_dir().join(format!("fcp-lease-failure-{}", Uuid::new_v4()));
        let path = root.join("lease.json");
        let store = LeaseMetadataStore::new(&path);
        let group_id = Uuid::new_v4();
        let original = store.load_or_initialize(group_id, false).unwrap();
        let mut replacement = original.clone();
        replacement.state = GroupState::Evicting;
        replacement.advance_transition().unwrap();
        let _failure = fail_next(FailurePoint::LeaseBeforePersist);

        assert!(store.persist(&replacement).is_err());
        let persisted: LeaseMetadata = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(persisted, original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stable_v1_migrates_atomically_and_transitional_v1_fails_closed() {
        let root = std::env::temp_dir().join(format!("fcp-lease-v2-test-{}", Uuid::new_v4()));
        let path = root.join("lease.json");
        fs::create_dir_all(&root).unwrap();
        let group_id = Uuid::new_v4();
        let legacy = serde_json::json!({
            "version": 1, "account_group_id": group_id, "state": "evicting",
            "lease_id": Uuid::new_v4(), "granted_at_unix_ms": 1, "expires_at_unix_ms": 2,
            "pending_operation_id": Uuid::new_v4(), "vault_sequence": 4, "transition_sequence": 9
        });
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let migrated = LeaseMetadataStore::new(&path)
            .load_or_initialize(group_id, true)
            .unwrap();
        assert_eq!(migrated.version, 2);
        assert_eq!(migrated.protocol_floor, 6);
        assert_eq!(migrated.state, GroupState::Degraded);
        assert_eq!(migrated.lease_id, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn protocol_floor_is_monotonic() {
        let group_id = Uuid::new_v4();
        let mut lease = LeaseMetadata::initial(group_id, false);
        assert!(lease.activate_protocol_v7(12).unwrap());
        assert_eq!(
            (lease.protocol_floor, lease.operation_sequence_high_water),
            (7, 12)
        );
        assert!(!lease.activate_protocol_v7(7).unwrap());
        assert_eq!(
            (lease.protocol_floor, lease.operation_sequence_high_water),
            (7, 12)
        );
    }
}
