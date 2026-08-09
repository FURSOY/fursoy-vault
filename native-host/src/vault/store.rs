use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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

    fn staged_delete_path_for(&self, group_id: uuid::Uuid) -> PathBuf {
        self.root.join(format!("{group_id}.fcpv.delete-pending"))
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

    pub fn stage_delete(&self, group_id: uuid::Uuid) -> FcpResult<()> {
        fs::create_dir_all(&self.root)?;
        let source = self.path_for(group_id);
        if !source.exists() {
            return Ok(());
        }
        let staged = self.staged_delete_path_for(group_id);
        if staged.exists() {
            return Err(crate::FcpError::Format(
                "vault deletion is already staged".into(),
            ));
        }
        fs::rename(source, staged)?;
        Ok(())
    }

    pub fn rollback_staged_delete(&self, group_id: uuid::Uuid) -> FcpResult<()> {
        let staged = self.staged_delete_path_for(group_id);
        if !staged.exists() {
            return Ok(());
        }
        let target = self.path_for(group_id);
        if target.exists() {
            return Err(crate::FcpError::Format(
                "cannot restore staged vault over an existing vault".into(),
            ));
        }
        fs::rename(staged, target)?;
        Ok(())
    }

    pub fn commit_staged_delete(&self, group_id: uuid::Uuid) -> FcpResult<()> {
        remove_if_exists(&self.staged_delete_path_for(group_id))
    }

    /// Recovers the only crash windows in group removal. Configured groups get their staged vault
    /// back; staged files for groups whose config commit succeeded are finalized.
    pub fn recover_staged_deletions(&self, configured: &HashSet<uuid::Uuid>) -> FcpResult<()> {
        if !self.root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            let Some(group_id) = staged_group_id(&path) else {
                continue;
            };
            if configured.contains(&group_id) && !self.path_for(group_id).exists() {
                fs::rename(&path, self.path_for(group_id))?;
            } else {
                remove_if_exists(&path)?;
            }
        }
        Ok(())
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

fn staged_group_id(path: &Path) -> Option<uuid::Uuid> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_suffix(".fcpv.delete-pending")?;
    uuid::Uuid::parse_str(id).ok()
}

fn remove_if_exists(path: &Path) -> FcpResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
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

    #[test]
    fn staged_delete_rolls_back_or_finishes_from_config_membership() {
        let root = std::env::temp_dir().join(format!("fcp-vault-delete-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let group_id = Uuid::new_v4();
        fs::write(root.join(format!("{group_id}.fcpv")), b"ciphertext").unwrap();
        let store = VaultStore::new(&root);
        store.stage_delete(group_id).unwrap();
        assert!(!store.path_for(group_id).exists());
        store
            .recover_staged_deletions(&HashSet::from([group_id]))
            .unwrap();
        assert_eq!(fs::read(store.path_for(group_id)).unwrap(), b"ciphertext");
        store.stage_delete(group_id).unwrap();
        store.recover_staged_deletions(&HashSet::new()).unwrap();
        assert!(!store.path_for(group_id).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
