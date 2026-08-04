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
                PathBuf::from(local_app_data).join("FursoyCookieProtector")
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
