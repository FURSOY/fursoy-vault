use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::config::LoadedConfig;
use crate::instance_lock::InstanceLock;
use crate::{FcpError, FcpResult};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryProfileCandidate {
    pub profile_id: Uuid,
    pub scopes: Vec<String>,
    pub config_modified_unix_ms: u64,
}

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
        Ok(Self::from_root(discover_root()?))
    }

    pub fn discover_profile(profile_id: Uuid) -> FcpResult<Self> {
        let base = discover_root()?;
        let root = base.join("profiles").join(profile_id.to_string());
        migrate_legacy_profile(&base, &root, profile_id)?;
        if root.join("recovery-claim-in-progress").exists() {
            return Err(FcpError::Protocol("profile recovery is in progress".into()));
        }
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

    fn from_root(root: PathBuf) -> Self {
        Self {
            vault_groups: root.join("vault").join("groups"),
            lease_groups: root.join("leases").join("groups"),
            capability_ledgers: root.join("leases").join("capabilities"),
            legacy_lease_metadata: root.join("leases").join("mvp-group.json"),
            legacy_capability_ledger: root.join("leases").join("capability-ledger.json"),
            account_groups_config: root.join("config").join("account-groups.json"),
            audit_directory: root.join("audit"),
            hello_credential: root.join("hello-credential.json"),
            root,
        }
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

    pub fn recovery_profiles(
        &self,
        current_profile_id: Uuid,
    ) -> FcpResult<Vec<RecoveryProfileCandidate>> {
        let profiles_root = self.root.parent().ok_or_else(|| {
            FcpError::Protocol("profile namespace has no profiles directory".into())
        })?;
        // Dispatcher unit tests and legacy export tools can open a direct root. Recovery discovery
        // is meaningful only for a real profiles/<uuid> namespace.
        if profiles_root.file_name().and_then(|value| value.to_str()) != Some("profiles") {
            return Ok(Vec::new());
        }
        if self.root.file_name().and_then(|value| value.to_str())
            != Some(current_profile_id.to_string().as_str())
        {
            return Err(FcpError::Protocol(
                "handshake profile does not match the selected namespace".into(),
            ));
        }
        recovery_profiles_in(profiles_root, current_profile_id)
    }

    pub fn claim_recovery_profile(
        &self,
        current_profile_id: Uuid,
        source_profile_id: Uuid,
        target_profile_id: Uuid,
    ) -> FcpResult<()> {
        if source_profile_id == current_profile_id
            || target_profile_id == current_profile_id
            || source_profile_id == target_profile_id
            || target_profile_id.is_nil()
        {
            return Err(FcpError::Protocol(
                "invalid recovery profile identities".into(),
            ));
        }
        let profiles_root = self.root.parent().ok_or_else(|| {
            FcpError::Protocol("profile namespace has no profiles directory".into())
        })?;
        if profiles_root.file_name().and_then(|value| value.to_str()) != Some("profiles")
            || self.root.file_name().and_then(|value| value.to_str())
                != Some(current_profile_id.to_string().as_str())
        {
            return Err(FcpError::Protocol(
                "current recovery namespace mismatch".into(),
            ));
        }
        let candidates = recovery_profiles_in(profiles_root, current_profile_id)?;
        if !candidates
            .iter()
            .any(|candidate| candidate.profile_id == source_profile_id)
        {
            return Err(FcpError::Protocol("recovery source is unavailable".into()));
        }
        let source = profiles_root.join(source_profile_id.to_string());
        let target = profiles_root.join(target_profile_id.to_string());
        if target.exists() {
            return Err(FcpError::Protocol("recovery target already exists".into()));
        }
        // Prove the source profile is not active, then leave a marker that makes a source-profile
        // host fail closed during the tiny unlock/rename window. The marker moves with the vault
        // and is removed only after the atomic same-volume rename succeeds.
        let source_lock = InstanceLock::acquire(&source)?;
        let marker = source.join("recovery-claim-in-progress");
        fs::write(&marker, target_profile_id.to_string())?;
        drop(source_lock);
        if let Err(error) = fs::rename(&source, &target) {
            let _ = fs::remove_file(&marker);
            return Err(error.into());
        }
        fs::remove_file(target.join("recovery-claim-in-progress"))?;
        Ok(())
    }
}

fn recovery_profiles_in(
    profiles_root: &Path,
    current_profile_id: Uuid,
) -> FcpResult<Vec<RecoveryProfileCandidate>> {
    let mut candidates = Vec::new();
    if !profiles_root.exists() {
        return Ok(candidates);
    }
    for entry in fs::read_dir(profiles_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Ok(profile_id) = entry.file_name().to_string_lossy().parse::<Uuid>() else {
            continue;
        };
        if profile_id == current_profile_id {
            continue;
        }
        let config_path = entry.path().join("config").join("account-groups.json");
        if !config_path.is_file() {
            continue;
        }
        // A corrupt or partially copied namespace must not prevent the current profile from
        // connecting. It is simply not offered as a recovery target.
        let Ok(loaded) = LoadedConfig::load(&config_path) else {
            continue;
        };
        if loaded.config.groups.is_empty() {
            continue;
        }
        let modified = fs::metadata(&config_path)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        candidates.push(RecoveryProfileCandidate {
            profile_id,
            scopes: loaded
                .config
                .groups
                .into_iter()
                .map(|group| group.scope)
                .collect(),
            config_modified_unix_ms: modified,
        });
    }
    candidates.sort_by(|left, right| {
        right
            .config_modified_unix_ms
            .cmp(&left.config_modified_unix_ms)
            .then_with(|| left.profile_id.cmp(&right.profile_id))
    });
    Ok(candidates)
}

fn discover_root() -> FcpResult<PathBuf> {
    match std::env::var_os("FCP_DATA_DIR") {
        Some(override_path) => Ok(PathBuf::from(override_path)),
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
            Ok(root)
        }
    }
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

    #[test]
    fn recovery_lists_only_other_non_empty_valid_profiles() {
        let base = std::env::temp_dir().join(format!("fcp-recovery-paths-{}", Uuid::new_v4()));
        let profiles = base.join("profiles");
        let current = Uuid::new_v4();
        let recoverable = Uuid::new_v4();
        let empty = Uuid::new_v4();
        let broken = Uuid::new_v4();
        for id in [current, recoverable, empty, broken] {
            fs::create_dir_all(profiles.join(id.to_string()).join("config")).unwrap();
        }
        let group_id = Uuid::new_v4();
        fs::write(
            profiles.join(recoverable.to_string()).join("config/account-groups.json"),
            format!(r#"{{"version":3,"compatibility_version":3,"groups":[{{"id":"{group_id}","display_name":"Example","scope":"example.com","policy_level":"balanced","store_policy":"normal_profile"}}]}}"#),
        ).unwrap();
        fs::write(
            profiles
                .join(empty.to_string())
                .join("config/account-groups.json"),
            br#"{"version":3,"compatibility_version":3,"groups":[]}"#,
        )
        .unwrap();
        fs::write(
            profiles
                .join(broken.to_string())
                .join("config/account-groups.json"),
            b"not-json",
        )
        .unwrap();

        let found = recovery_profiles_in(&profiles, current).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].profile_id, recoverable);
        assert_eq!(found[0].scopes, vec!["example.com"]);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn recovery_claim_moves_vault_to_a_fresh_identity() {
        let base = std::env::temp_dir().join(format!("fcp-recovery-claim-{}", Uuid::new_v4()));
        let profiles = base.join("profiles");
        let current = Uuid::new_v4();
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        fs::create_dir_all(profiles.join(current.to_string())).unwrap();
        fs::create_dir_all(profiles.join(source.to_string()).join("config")).unwrap();
        let group_id = Uuid::new_v4();
        fs::write(
            profiles.join(source.to_string()).join("config/account-groups.json"),
            format!(r#"{{"version":3,"compatibility_version":3,"groups":[{{"id":"{group_id}","display_name":"Recovered","scope":"recover.example","policy_level":"balanced","store_policy":"normal_profile"}}]}}"#),
        ).unwrap();
        fs::create_dir_all(profiles.join(source.to_string()).join("vault/groups")).unwrap();
        fs::write(
            profiles
                .join(source.to_string())
                .join("vault/groups/marker"),
            b"vault",
        )
        .unwrap();
        let paths = DataPaths::from_root(profiles.join(current.to_string()));

        let active_source = InstanceLock::acquire(&profiles.join(source.to_string())).unwrap();
        assert!(
            paths
                .claim_recovery_profile(current, source, target)
                .is_err()
        );
        assert!(profiles.join(source.to_string()).exists());
        drop(active_source);
        paths
            .claim_recovery_profile(current, source, target)
            .unwrap();

        assert!(!profiles.join(source.to_string()).exists());
        assert_eq!(
            fs::read(
                profiles
                    .join(target.to_string())
                    .join("vault/groups/marker")
            )
            .unwrap(),
            b"vault"
        );
        fs::remove_dir_all(base).unwrap();
    }
}
