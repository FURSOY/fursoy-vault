use std::fs::{self, OpenOptions};
use std::io::Write;
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
    pub operation_journals: PathBuf,
    pub snapshot_integrity_key: PathBuf,
}

impl DataPaths {
    pub fn discover() -> FcpResult<Self> {
        Ok(Self::from_root(discover_root()?))
    }

    pub fn discover_profile(profile_id: Uuid) -> FcpResult<Self> {
        let base = discover_root()?;
        let root = base.join("profiles").join(profile_id.to_string());
        migrate_legacy_profile(&base, &root, profile_id)?;
        Ok(Self::from_root(root))
    }

    pub fn discover_for_export(profile_id: Option<Uuid>) -> FcpResult<Self> {
        let base = discover_root()?;
        if let Some(profile_id) = profile_id {
            return Ok(Self::from_root(
                base.join("profiles").join(profile_id.to_string()),
            ));
        }
        let profiles_root = base.join("profiles");
        let mut profiles = Vec::new();
        if profiles_root.exists() {
            for entry in fs::read_dir(&profiles_root)? {
                let entry = entry?;
                if entry.file_type()?.is_dir()
                    && entry.file_name().to_string_lossy().parse::<Uuid>().is_ok()
                {
                    profiles.push(entry.path());
                }
            }
        }
        match profiles.as_slice() {
            [] => Ok(Self::from_root(base)),
            [profile] => Ok(Self::from_root(profile.clone())),
            _ => Err(FcpError::Protocol(
                "multiple browser profiles exist; select one with --profile".into(),
            )),
        }
    }

    pub(crate) fn from_root(root: PathBuf) -> Self {
        Self {
            vault_groups: root.join("vault").join("groups"),
            lease_groups: root.join("leases").join("groups"),
            capability_ledgers: root.join("leases").join("capabilities"),
            legacy_lease_metadata: root.join("leases").join("mvp-group.json"),
            legacy_capability_ledger: root.join("leases").join("capability-ledger.json"),
            account_groups_config: root.join("config").join("account-groups.json"),
            audit_directory: root.join("audit"),
            hello_credential: root.join("hello-credential.json"),
            operation_journals: root.join("operations").join("groups"),
            snapshot_integrity_key: root.join("operations").join("snapshot-key.dpapi"),
            root,
        }
    }

    pub fn sibling_profile(&self, profile_id: Uuid) -> FcpResult<Self> {
        let profiles = self.root.parent().ok_or_else(|| {
            FcpError::Protocol("profile data root has no profiles directory".into())
        })?;
        if profiles.file_name().and_then(|value| value.to_str()) != Some("profiles") {
            return Err(FcpError::Protocol(
                "profile data root is outside the profiles directory".into(),
            ));
        }
        Ok(Self::from_root(profiles.join(profile_id.to_string())))
    }

    pub fn sibling_profile_roots(&self) -> FcpResult<Vec<(Uuid, PathBuf)>> {
        let profiles = self.root.parent().ok_or_else(|| {
            FcpError::Protocol("profile data root has no profiles directory".into())
        })?;
        if profiles.file_name().and_then(|value| value.to_str()) != Some("profiles") {
            return Err(FcpError::Protocol(
                "profile data root is outside the profiles directory".into(),
            ));
        }
        if !profiles.exists() {
            return Ok(Vec::new());
        }
        let mut roots = Vec::new();
        for entry in fs::read_dir(profiles)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Ok(profile_id) = name.parse::<Uuid>() {
                roots.push((profile_id, entry.path()));
            }
        }
        Ok(roots)
    }

    pub fn lease_path(&self, group_id: Uuid) -> PathBuf {
        self.lease_groups.join(format!("{group_id}.json"))
    }

    pub fn capability_path(&self, group_id: Uuid) -> PathBuf {
        self.capability_ledgers.join(format!("{group_id}.json"))
    }

    pub fn operation_journal_path(&self, group_id: Uuid) -> PathBuf {
        self.operation_journals.join(format!("{group_id}.json"))
    }

    pub fn operation_journals_exist(&self) -> FcpResult<bool> {
        if !self.operation_journals.exists() {
            return Ok(false);
        }
        Ok(fs::read_dir(&self.operation_journals)?
            .filter_map(Result::ok)
            .any(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json")))
    }

    /// Removes non-authoritative per-group projections after the authoritative config no longer
    /// contains the group. This is intentionally idempotent so a crash after config commit can be
    /// repaired by a retry without resurrecting an unfinished operation.
    pub fn remove_group_projections(&self, group_id: Uuid) -> FcpResult<()> {
        for path in [
            self.lease_path(group_id),
            self.capability_path(group_id),
            self.operation_journal_path(group_id),
        ] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
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

fn discover_root() -> FcpResult<PathBuf> {
    match std::env::var_os("FCP_DATA_DIR") {
        Some(override_path) => Ok(PathBuf::from(override_path)),
        None => platform_root(),
    }
}

#[cfg(windows)]
fn platform_root() -> FcpResult<PathBuf> {
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
    Ok(root)
}

/// XDG's data directory, which is where state that must survive and is not a cache belongs. The
/// `$HOME/.local/share` fallback is the specification's own default for an unset `XDG_DATA_HOME`.
/// There is no legacy directory to migrate here: no Linux build has ever shipped.
#[cfg(unix)]
fn platform_root() -> FcpResult<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => {
            let home = std::env::var_os("HOME").ok_or_else(|| {
                FcpError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "neither XDG_DATA_HOME nor HOME is set",
                ))
            })?;
            PathBuf::from(home).join(".local").join("share")
        }
    };
    Ok(base.join("fursoy-vault"))
}

fn migrate_legacy_profile(base: &Path, target: &Path, profile_id: Uuid) -> FcpResult<()> {
    let legacy_items = ["vault", "leases", "config", "audit"];
    let has_legacy = legacy_items.iter().any(|name| base.join(name).exists())
        || base.join("hello-credential.json").exists();
    if !has_legacy {
        return Ok(());
    }
    fs::create_dir_all(base.join("profiles"))?;
    let owner_path = base.join("profile-migration-owner");
    let owner = match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&owner_path)
    {
        Ok(mut file) => {
            let value = profile_id.to_string();
            file.write_all(value.as_bytes())?;
            file.sync_all()?;
            value
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::read_to_string(&owner_path)?.trim().to_owned()
        }
        Err(error) => return Err(error.into()),
    };
    if owner != profile_id.to_string() {
        return Ok(());
    }
    for name in legacy_items {
        migrate_one(&base.join(name), &target.join(name))?;
    }
    migrate_one(
        &base.join("hello-credential.json"),
        &target.join("hello-credential.json"),
    )?;
    Ok(())
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

    #[test]
    fn first_profile_claims_legacy_data_and_other_profiles_start_isolated() {
        let base = std::env::temp_dir().join(format!("fcp-profile-paths-{}", Uuid::new_v4()));
        fs::create_dir_all(base.join("config")).unwrap();
        fs::write(base.join("config/account-groups.json"), b"legacy").unwrap();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let first_root = base.join("profiles").join(first.to_string());
        let second_root = base.join("profiles").join(second.to_string());

        migrate_legacy_profile(&base, &first_root, first).unwrap();
        migrate_legacy_profile(&base, &second_root, second).unwrap();

        assert_eq!(
            fs::read(first_root.join("config/account-groups.json")).unwrap(),
            b"legacy"
        );
        assert!(!second_root.join("config/account-groups.json").exists());
        assert_eq!(
            fs::read_to_string(base.join("profile-migration-owner")).unwrap(),
            first.to_string()
        );
        fs::remove_dir_all(base).unwrap();
    }
}
