use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use uuid::Uuid;

use crate::{FcpResult, MVP_ACCOUNT_GROUP_ID};

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct AuditEntry<'a> {
    timestamp_unix_ms: u64,
    account_group_id: Uuid,
    event: &'a str,
    outcome: &'a str,
    operation_id: Option<Uuid>,
    detail_code: Option<&'a str>,
}

pub struct AuditLogger {
    directory: PathBuf,
}

impl AuditLogger {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// Audit entries intentionally accept no cookie object, name, value, domain, or arbitrary
    /// error string. Callers can emit only bounded event/outcome/detail codes.
    pub fn record(
        &self,
        event: &'static str,
        outcome: &'static str,
        operation_id: Option<Uuid>,
        detail_code: Option<&'static str>,
    ) -> FcpResult<()> {
        fs::create_dir_all(&self.directory)?;
        let now = unix_ms()?;
        let day = now / 86_400_000;
        let path = self.directory.join(format!("audit-day-{day}.log"));
        let entry = AuditEntry {
            timestamp_unix_ms: now,
            account_group_id: MVP_ACCOUNT_GROUP_ID,
            event,
            outcome,
            operation_id,
            detail_code,
        };
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(&line)?;
        file.sync_data()?;
        Ok(())
    }
}

pub fn unix_ms() -> FcpResult<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "system clock precedes Unix epoch",
            )
        })?
        .as_millis()
        .try_into()
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Unix time exceeds u64")
        })?)
}
