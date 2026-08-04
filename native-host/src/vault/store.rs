use std::fs;
use std::path::PathBuf;

use crate::FcpResult;
use crate::atomic_file;
use crate::crypto::aead::SecretDek;
use crate::vault::format::VaultRecord;

pub struct VaultStore {
    root: PathBuf,
}

impl VaultStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path_for(&self, group_id: uuid::Uuid) -> PathBuf {
        self.root.join(format!("{group_id}.fcpv"))
    }

    pub fn read(&self, group_id: uuid::Uuid) -> FcpResult<VaultRecord> {
        VaultRecord::decode(&fs::read(self.path_for(group_id))?)
    }

    pub fn delete(&self, group_id: uuid::Uuid) -> FcpResult<()> {
        let path = self.path_for(group_id);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Writes, flushes, reads back, authenticates, then atomically replaces the group file.
    /// The caller must not remove browser cookies until this function returns success.
    pub fn write_verified(&self, record: &VaultRecord, dek: &SecretDek) -> FcpResult<()> {
        let target = self.path_for(record.header.group_id);
        let bytes = record.encode()?;
        atomic_file::write_verified(&target, &bytes, |persisted| {
            let verified = VaultRecord::decode(persisted)?;
            let _ = verified.open(dek)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::aead::DEK_BYTES;
    use crate::crypto::platform_kek::WRAPPED_DEK_BYTES;
    use crate::vault::payload::VaultPayload;
    use uuid::Uuid;

    #[test]
    fn verified_write_replaces_one_group_file() {
        let root = std::env::temp_dir().join(format!("fcp-vault-test-{}", Uuid::new_v4()));
        let group_id = Uuid::new_v4();
        let dek = SecretDek::from_bytes([7; DEK_BYTES]);
        let mut payload = VaultPayload::empty();
        payload.vault_sequence = 1;
        let record = VaultRecord::seal(
            group_id,
            [8; 16],
            vec![9; WRAPPED_DEK_BYTES],
            &dek,
            &payload,
        )
        .unwrap();
        let store = VaultStore::new(&root);
        store.write_verified(&record, &dek).unwrap();
        assert_eq!(store.read(group_id).unwrap().open(&dek).unwrap(), payload);
        store.delete(group_id).unwrap();
        assert!(!store.path_for(group_id).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
