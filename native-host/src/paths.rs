use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::{FcpError, FcpResult};

#[derive(Clone, Debug)]
pub struct DataPaths {
    pub root: PathBuf,
    pub vault_groups: PathBuf,
    pub lease_groups: PathBuf,
    pub capability_ledgers: PathBuf,
    pub legacy_lease_metadata: PathBuf,
    pub legacy_capability_ledger: PathBuf,
    pub account_groups_config: PathBuf,
    pub audit_directory: PathBuf,
    pub hello_credential: PathBuf,
}

impl DataPaths {
    pub fn discover() -> FcpResult<Self> {
        let root = match std::env::var_os("FCP_DATA_DIR") {
            Some(override_path) => PathBuf::from(override_path),
            None => {
                let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
                    FcpError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "LOCALAPPDATA is unavailable",
                    ))
                })?;
                let local_app_data = PathBuf::from(local_app_data);
                let root = local_app_data.join("FursoyVault");
                // Renamed from FursoyCookieProtector. install/register.ps1 already moves this
                // directory at install time; this is the fallback for a host run directly
                // (dev/test) without going through the installer first.
                migrate_one(&local_app_data.join("FursoyCookieProtector"), &root)?;
                root
            }
        };
        Ok(Self {
            vault_groups: root.join("vault").join("groups"),
            lease_groups: root.join("leases").join("groups"),
            capability_ledgers: root.join("leases").join("capabilities"),
            legacy_lease_metadata: root.join("leases").join("mvp-group.json"),
            legacy_capability_ledger: root.join("leases").join("capability-ledger.json"),
            account_groups_config: root.join("config").join("account-groups.json"),
            audit_directory: root.join("audit"),
            hello_credential: root.join("hello-credential.json"),
            root,
        })
    }

    pub fn lease_path(&self, group_id: Uuid) -> PathBuf {
        self.lease_groups.join(format!("{group_id}.json"))
    }

    pub fn capability_path(&self, group_id: Uuid) -> PathBuf {
        self.capability_ledgers.join(format!("{group_id}.json"))
    }

    pub fn migrate_phase5_group(&self, group_id: Uuid) -> FcpResult<()> {
        migrate_one(&self.legacy_lease_metadata, &self.lease_path(group_id))?;
        migrate_one(
            &self.legacy_capability_ledger,
            &self.capability_path(group_id),
        )?;
        Ok(())
    }
}

fn migrate_one(legacy: &Path, target: &Path) -> FcpResult<()> {
    if !legacy.exists() || target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(legacy, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn migrate_one_moves_a_whole_legacy_root_directory() {
        let base = std::env::temp_dir().join(format!("fcp-paths-test-{}", Uuid::new_v4()));
        let legacy_root = base.join("FursoyCookieProtector");
        let new_root = base.join("FursoyVault");
        fs::create_dir_all(legacy_root.join("vault")).unwrap();
        fs::write(legacy_root.join("hello-credential.json"), b"{}").unwrap();

        migrate_one(&legacy_root, &new_root).unwrap();

        assert!(!legacy_root.exists());
        assert!(new_root.join("hello-credential.json").exists());
        assert!(new_root.join("vault").is_dir());

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn migrate_one_does_nothing_if_the_new_root_already_exists() {
        let base = std::env::temp_dir().join(format!("fcp-paths-test-{}", Uuid::new_v4()));
        let legacy_root = base.join("FursoyCookieProtector");
        let new_root = base.join("FursoyVault");
        fs::create_dir_all(&legacy_root).unwrap();
        fs::write(legacy_root.join("marker"), b"old").unwrap();
        fs::create_dir_all(&new_root).unwrap();
        fs::write(new_root.join("marker"), b"new").unwrap();

        migrate_one(&legacy_root, &new_root).unwrap();

        // Never overwrites an already-migrated (or freshly installed) new root.
        assert!(legacy_root.exists());
        assert_eq!(fs::read(new_root.join("marker")).unwrap(), b"new");

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn migrate_one_does_nothing_if_no_legacy_root_exists() {
        let base = std::env::temp_dir().join(format!("fcp-paths-test-{}", Uuid::new_v4()));
        let legacy_root = base.join("FursoyCookieProtector");
        let new_root = base.join("FursoyVault");

        migrate_one(&legacy_root, &new_root).unwrap();

        assert!(!new_root.exists());
        fs::create_dir_all(&base).unwrap();
        fs::remove_dir_all(&base).unwrap();
    }
}
