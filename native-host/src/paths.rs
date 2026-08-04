use std::path::PathBuf;

use crate::{FcpError, FcpResult};

#[derive(Clone, Debug)]
pub struct DataPaths {
    pub root: PathBuf,
    pub vault_groups: PathBuf,
    pub lease_metadata: PathBuf,
    pub capability_ledger: PathBuf,
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
            lease_metadata: root.join("leases").join("mvp-group.json"),
            capability_ledger: root.join("leases").join("capability-ledger.json"),
            audit_directory: root.join("audit"),
            root,
        })
    }
}
