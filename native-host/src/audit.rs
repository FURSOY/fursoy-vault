use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::atomic_file;
use crate::crypto::fill_random;
use crate::dpapi;
use crate::{FcpError, FcpResult};

const AUDIT_SCHEMA_VERSION: u8 = 2;
const AUDIT_KEY_BYTES: usize = 32;
const ZERO_MAC: [u8; 32] = [0; 32];
const AUDIT_RETENTION_DAYS: u64 = 90;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UnsignedAuditEntry<'a> {
    schema_version: u8,
    sequence: u64,
    previous_mac: &'a str,
    timestamp_unix_ms: u64,
    account_group_id: Option<Uuid>,
    event: &'a str,
    outcome: &'a str,
    operation_id: Option<Uuid>,
    detail_code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audit_event_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuditEntry {
    schema_version: u8,
    sequence: u64,
    previous_mac: String,
    timestamp_unix_ms: u64,
    account_group_id: Option<Uuid>,
    event: String,
    outcome: String,
    operation_id: Option<Uuid>,
    detail_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audit_event_id: Option<Uuid>,
    mac: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuditAnchor {
    schema_version: u8,
    sequence: u64,
    mac: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuditCheckpoint {
    schema_version: u8,
    sequence: u64,
    mac: String,
    archived_through_day: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuditOutboxEntry {
    audit_event_id: Uuid,
    account_group_id: Uuid,
    event: String,
    outcome: String,
    operation_id: Option<Uuid>,
    detail_code: Option<String>,
}

struct AuditState {
    key: Zeroizing<[u8; AUDIT_KEY_BYTES]>,
    sequence: u64,
    mac: [u8; 32],
}

pub struct AuditLogger {
    directory: PathBuf,
    state: Mutex<AuditState>,
    recovered_on_open: bool,
}

impl AuditLogger {
    /// Verifies the retained HMAC chain before producing a portable, redacted JSONL export.
    /// Expired segments are represented by the protected checkpoint and are intentionally not
    /// exported; the export contains only the configured retention window.
    pub fn export_verified(directory: &Path, destination: &Path) -> FcpResult<()> {
        let logger = Self::open_strict(directory)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
            let audit_root = fs::canonicalize(directory)?;
            let export_parent = fs::canonicalize(parent)?;
            let existing_target = destination.exists().then(|| fs::canonicalize(destination));
            if export_parent.starts_with(&audit_root)
                || existing_target
                    .is_some_and(|value| value.is_ok_and(|target| target.starts_with(&audit_root)))
            {
                return Err(FcpError::Format(
                    "audit export destination must be outside the live audit directory".into(),
                ));
            }
        } else {
            return Err(FcpError::Format(
                "audit export destination has no parent".into(),
            ));
        }
        let mut output = Vec::new();
        for path in chain_files(&logger.directory)? {
            output.extend_from_slice(&fs::read(path)?);
        }
        atomic_file::write_verified(destination, &output, |candidate| {
            if candidate != output {
                return Err(FcpError::Format(
                    "audit export write verification failed".into(),
                ));
            }
            Ok(())
        })
    }

    pub fn open(directory: impl Into<PathBuf>) -> FcpResult<Self> {
        let directory = directory.into();
        match Self::open_strict(&directory) {
            Ok(logger) => Ok(logger),
            Err(original) => {
                if !directory.exists() || fs::read_dir(&directory)?.next().is_none() {
                    return Err(original);
                }
                let parent = directory
                    .parent()
                    .ok_or_else(|| FcpError::Format("audit directory has no parent".into()))?;
                let name = directory
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("audit");
                let quarantine = parent.join(format!("{name}-quarantine-{}", Uuid::new_v4()));
                fs::rename(&directory, &quarantine)?;
                let mut logger = Self::open_strict(&directory)?;
                logger.recovered_on_open = true;
                logger.record_system(
                    "audit_recovery",
                    "high_alert",
                    Some("integrity_failure_quarantined"),
                )?;
                Ok(logger)
            }
        }
    }

    fn open_strict(directory: &Path) -> FcpResult<Self> {
        let directory = directory.to_path_buf();
        fs::create_dir_all(&directory)?;
        let key = load_or_create_key(&directory)?;
        let anchor = load_anchor(&directory)?;
        let checkpoint = load_checkpoint(&directory)?;
        let verified = verify_chain(&directory, &key, anchor.as_ref(), checkpoint.as_ref())?;
        let logger = Self {
            directory,
            state: Mutex::new(AuditState {
                key,
                sequence: verified.sequence,
                mac: verified.mac,
            }),
            recovered_on_open: false,
        };
        if anchor.as_ref().is_none_or(|value| {
            value.sequence != verified.sequence || value.mac != encode_hex(&verified.mac)
        }) {
            logger.persist_anchor(verified.sequence, &verified.mac)?;
        }
        logger.compact_expired_segments()?;
        logger.drain_outbox()?;
        Ok(logger)
    }

    pub fn recovered_on_open(&self) -> bool {
        self.recovered_on_open
    }

    /// Audit entries intentionally accept no cookie object, name, value, domain, command line, or
    /// arbitrary error string. Callers can emit only bounded event/outcome/detail codes.
    pub fn record(
        &self,
        account_group_id: Uuid,
        event: &str,
        outcome: &str,
        operation_id: Option<Uuid>,
        detail_code: Option<&str>,
    ) -> FcpResult<()> {
        self.record_inner(
            Some(account_group_id),
            event,
            outcome,
            operation_id,
            detail_code,
            None,
        )
    }

    pub fn record_system(
        &self,
        event: &str,
        outcome: &str,
        detail_code: Option<&str>,
    ) -> FcpResult<()> {
        self.record_inner(None, event, outcome, None, detail_code, None)
    }

    pub fn record_once(
        &self,
        audit_event_id: Uuid,
        account_group_id: Uuid,
        event: &str,
        outcome: &str,
        operation_id: Option<Uuid>,
        detail_code: Option<&str>,
    ) -> FcpResult<bool> {
        if audit_event_id.is_nil() {
            return Err(FcpError::Protocol("audit event id must not be nil".into()));
        }
        for path in chain_files(&self.directory)? {
            for line in fs::read(path)?
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let entry: AuditEntry = serde_json::from_slice(line)?;
                if entry.audit_event_id == Some(audit_event_id) {
                    return Ok(false);
                }
            }
        }
        self.record_inner(
            Some(account_group_id),
            event,
            outcome,
            operation_id,
            detail_code,
            Some(audit_event_id),
        )?;
        Ok(true)
    }

    pub fn record_once_deferred(
        &self,
        audit_event_id: Uuid,
        account_group_id: Uuid,
        event: &str,
        outcome: &str,
        operation_id: Option<Uuid>,
        detail_code: Option<&str>,
    ) {
        if self
            .record_once(
                audit_event_id,
                account_group_id,
                event,
                outcome,
                operation_id,
                detail_code,
            )
            .is_ok()
        {
            return;
        }
        // A post-append failure is already present and is therefore deduplicated by this retry.
        if self
            .record_once(
                audit_event_id,
                account_group_id,
                event,
                outcome,
                operation_id,
                detail_code,
            )
            .is_ok()
        {
            return;
        }
        let entry = AuditOutboxEntry {
            audit_event_id,
            account_group_id,
            event: event.into(),
            outcome: outcome.into(),
            operation_id,
            detail_code: detail_code.map(str::to_owned),
        };
        let _ = self.enqueue_outbox(entry);
    }

    fn enqueue_outbox(&self, entry: AuditOutboxEntry) -> FcpResult<()> {
        let path = self.directory.join("audit-outbox-v1.json");
        let mut entries: Vec<AuditOutboxEntry> = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            Vec::new()
        };
        if !entries
            .iter()
            .any(|value| value.audit_event_id == entry.audit_event_id)
        {
            entries.push(entry);
        }
        atomic_file::write_verified(&path, &serde_json::to_vec(&entries)?, |bytes| {
            let verified: Vec<AuditOutboxEntry> = serde_json::from_slice(bytes)?;
            if verified.len() != entries.len() {
                return Err(FcpError::Format("audit outbox read-back mismatch".into()));
            }
            Ok(())
        })
    }

    fn drain_outbox(&self) -> FcpResult<()> {
        let path = self.directory.join("audit-outbox-v1.json");
        if !path.exists() {
            return Ok(());
        }
        let entries: Vec<AuditOutboxEntry> = serde_json::from_slice(&fs::read(&path)?)?;
        let mut remaining = Vec::new();
        for entry in entries {
            if self
                .record_once(
                    entry.audit_event_id,
                    entry.account_group_id,
                    &entry.event,
                    &entry.outcome,
                    entry.operation_id,
                    entry.detail_code.as_deref(),
                )
                .is_err()
            {
                remaining.push(entry);
            }
        }
        if remaining.is_empty() {
            fs::remove_file(path)?;
        } else {
            atomic_file::write_verified(&path, &serde_json::to_vec(&remaining)?, |_| Ok(()))?;
        }
        Ok(())
    }

    fn record_inner(
        &self,
        account_group_id: Option<Uuid>,
        event: &str,
        outcome: &str,
        operation_id: Option<Uuid>,
        detail_code: Option<&str>,
        audit_event_id: Option<Uuid>,
    ) -> FcpResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| FcpError::Format("audit state lock poisoned".into()))?;
        let sequence = state
            .sequence
            .checked_add(1)
            .ok_or_else(|| FcpError::Format("audit sequence overflow".into()))?;
        let now = unix_ms()?;
        let previous_mac = encode_hex(&state.mac);
        let unsigned = UnsignedAuditEntry {
            schema_version: AUDIT_SCHEMA_VERSION,
            sequence,
            previous_mac: &previous_mac,
            timestamp_unix_ms: now,
            account_group_id,
            event,
            outcome,
            operation_id,
            detail_code,
            audit_event_id,
        };
        let canonical = serde_json::to_vec(&unsigned)?;
        let mac = calculate_mac(state.key.as_slice(), &canonical)?;
        let entry = AuditEntry {
            schema_version: unsigned.schema_version,
            sequence,
            previous_mac,
            timestamp_unix_ms: now,
            account_group_id,
            event: event.into(),
            outcome: outcome.into(),
            operation_id,
            detail_code: detail_code.map(str::to_owned),
            audit_event_id,
            mac: encode_hex(&mac),
        };
        let day = now / 86_400_000;
        let path = self.directory.join(format!("audit-v2-day-{day}.jsonl"));
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');

        #[cfg(test)]
        crate::test_support::check(crate::test_support::FailurePoint::AuditBeforeAppend)?;

        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(&line)?;
        file.sync_data()?;

        #[cfg(test)]
        crate::test_support::check(crate::test_support::FailurePoint::AuditAfterAppend)?;

        state.sequence = sequence;
        state.mac = mac;
        self.persist_anchor(sequence, &mac)
    }

    fn persist_anchor(&self, sequence: u64, mac: &[u8; 32]) -> FcpResult<()> {
        let anchor = AuditAnchor {
            schema_version: AUDIT_SCHEMA_VERSION,
            sequence,
            mac: encode_hex(mac),
        };
        let plaintext = Zeroizing::new(serde_json::to_vec(&anchor)?);
        persist_dpapi_file(&anchor_path(&self.directory), &plaintext)
    }

    fn compact_expired_segments(&self) -> FcpResult<()> {
        let current_day = unix_ms()? / 86_400_000;
        let cutoff = current_day.saturating_sub(AUDIT_RETENTION_DAYS);
        let expired = chain_files(&self.directory)?
            .into_iter()
            .filter(|path| audit_day(path).is_some_and(|day| day < cutoff))
            .collect::<Vec<_>>();
        let Some(last) = expired.last() else {
            return Ok(());
        };
        let bytes = fs::read(last)?;
        let tail = bytes
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.is_empty())
            .ok_or_else(|| FcpError::Format("expired audit segment is empty".into()))?;
        let entry: AuditEntry = serde_json::from_slice(tail)?;
        let checkpoint = AuditCheckpoint {
            schema_version: AUDIT_SCHEMA_VERSION,
            sequence: entry.sequence,
            mac: entry.mac,
            archived_through_day: audit_day(last)
                .ok_or_else(|| FcpError::Format("audit day is malformed".into()))?,
        };
        persist_checkpoint(&self.directory, &checkpoint)?;
        for path in expired {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn load_or_create_key(directory: &Path) -> FcpResult<Zeroizing<[u8; AUDIT_KEY_BYTES]>> {
    let path = key_path(directory);
    if path.exists() {
        let plaintext = Zeroizing::new(dpapi::unprotect(&fs::read(path)?)?);
        let key: [u8; AUDIT_KEY_BYTES] = plaintext
            .as_slice()
            .try_into()
            .map_err(|_| FcpError::Format("audit HMAC key has invalid length".into()))?;
        return Ok(Zeroizing::new(key));
    }
    if anchor_path(directory).exists()
        || checkpoint_path(directory).exists()
        || !chain_files(directory)?.is_empty()
    {
        return Err(FcpError::Format(
            "audit HMAC key is missing while authenticated audit material exists".into(),
        ));
    }
    let mut key = Zeroizing::new([0u8; AUDIT_KEY_BYTES]);
    fill_random(key.as_mut())?;
    persist_dpapi_file(&path, key.as_ref())?;
    Ok(key)
}

fn load_checkpoint(directory: &Path) -> FcpResult<Option<AuditCheckpoint>> {
    let path = checkpoint_path(directory);
    if !path.exists() {
        return Ok(None);
    }
    let plaintext = Zeroizing::new(dpapi::unprotect(&fs::read(path)?)?);
    let checkpoint: AuditCheckpoint = serde_json::from_slice(&plaintext)?;
    if checkpoint.schema_version != AUDIT_SCHEMA_VERSION || decode_mac(&checkpoint.mac).is_err() {
        return Err(FcpError::Format("audit checkpoint is malformed".into()));
    }
    Ok(Some(checkpoint))
}

fn persist_checkpoint(directory: &Path, checkpoint: &AuditCheckpoint) -> FcpResult<()> {
    let plaintext = Zeroizing::new(serde_json::to_vec(checkpoint)?);
    persist_dpapi_file(&checkpoint_path(directory), &plaintext)
}

fn load_anchor(directory: &Path) -> FcpResult<Option<AuditAnchor>> {
    let path = anchor_path(directory);
    if !path.exists() {
        if !chain_files(directory)?.is_empty() {
            return Err(FcpError::Format(
                "audit anchor is missing while authenticated logs exist".into(),
            ));
        }
        return Ok(None);
    }
    let plaintext = Zeroizing::new(dpapi::unprotect(&fs::read(path)?)?);
    let anchor: AuditAnchor = serde_json::from_slice(&plaintext)?;
    if anchor.schema_version != AUDIT_SCHEMA_VERSION || decode_mac(&anchor.mac).is_err() {
        return Err(FcpError::Format("audit anchor is malformed".into()));
    }
    Ok(Some(anchor))
}

fn verify_chain(
    directory: &Path,
    key: &[u8; AUDIT_KEY_BYTES],
    anchor: Option<&AuditAnchor>,
    checkpoint: Option<&AuditCheckpoint>,
) -> FcpResult<VerifiedTail> {
    let mut expected_sequence = checkpoint.map_or(1, |value| value.sequence.saturating_add(1));
    let mut previous_mac = checkpoint.map_or(Ok(ZERO_MAC), |value| decode_mac(&value.mac))?;
    let mut anchor_matched =
        anchor.is_none_or(|value| value.sequence <= checkpoint.map_or(0, |item| item.sequence));
    for path in chain_files(directory)? {
        let bytes = fs::read(&path)?;
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err(FcpError::Format(format!(
                "audit log has a truncated final line: {}",
                path.display()
            )));
        }
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let entry: AuditEntry = serde_json::from_slice(line)?;
            if entry.schema_version != AUDIT_SCHEMA_VERSION || entry.sequence != expected_sequence {
                return Err(FcpError::Format(
                    "audit sequence regression or gap detected".into(),
                ));
            }
            if entry.previous_mac != encode_hex(&previous_mac) {
                return Err(FcpError::Format("audit previous_mac mismatch".into()));
            }
            let unsigned = UnsignedAuditEntry {
                schema_version: entry.schema_version,
                sequence: entry.sequence,
                previous_mac: &entry.previous_mac,
                timestamp_unix_ms: entry.timestamp_unix_ms,
                account_group_id: entry.account_group_id,
                event: &entry.event,
                outcome: &entry.outcome,
                operation_id: entry.operation_id,
                detail_code: entry.detail_code.as_deref(),
                audit_event_id: entry.audit_event_id,
            };
            let canonical = serde_json::to_vec(&unsigned)?;
            let actual_mac = decode_mac(&entry.mac)?;
            verify_mac(key, &canonical, &actual_mac)?;
            previous_mac = actual_mac;
            if anchor.is_some_and(|value| value.sequence == entry.sequence) {
                if anchor.is_some_and(|value| value.mac != entry.mac) {
                    return Err(FcpError::Format("audit anchor MAC mismatch".into()));
                }
                anchor_matched = true;
            }
            expected_sequence = expected_sequence
                .checked_add(1)
                .ok_or_else(|| FcpError::Format("audit sequence overflow".into()))?;
        }
    }
    let sequence = expected_sequence - 1;
    if let Some(anchor) = anchor
        && (anchor.sequence > sequence || !anchor_matched)
    {
        return Err(FcpError::Format(
            "audit log truncation detected by anchor".into(),
        ));
    }
    Ok(VerifiedTail {
        sequence,
        mac: previous_mac,
    })
}

struct VerifiedTail {
    sequence: u64,
    mac: [u8; 32],
}

fn persist_dpapi_file(path: &Path, plaintext: &[u8]) -> FcpResult<()> {
    let protected = dpapi::protect(plaintext)?;
    atomic_file::write_verified(path, &protected, |candidate| {
        let recovered = Zeroizing::new(dpapi::unprotect(candidate)?);
        if recovered.as_slice() != plaintext {
            return Err(FcpError::Crypto("DPAPI write verification failed"));
        }
        Ok(())
    })
}

fn chain_files(directory: &Path) -> FcpResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with("audit-v2-day-") && name.ends_with(".jsonl") {
            files.push(path);
        }
    }
    files.sort_by_key(|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .and_then(|name| name.strip_prefix("audit-v2-day-"))
            .and_then(|name| name.strip_suffix(".jsonl"))
            .and_then(|day| day.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });
    Ok(files)
}

fn calculate_mac(key: &[u8], bytes: &[u8]) -> FcpResult<[u8; 32]> {
    const BLOCK_BYTES: usize = 64;
    if key.len() > BLOCK_BYTES {
        return Err(FcpError::Crypto(
            "audit HMAC key exceeds SHA-256 block size",
        ));
    }
    let mut padded_key = Zeroizing::new([0u8; BLOCK_BYTES]);
    padded_key[..key.len()].copy_from_slice(key);
    let mut inner_pad = Zeroizing::new([0x36u8; BLOCK_BYTES]);
    let mut outer_pad = Zeroizing::new([0x5cu8; BLOCK_BYTES]);
    for index in 0..BLOCK_BYTES {
        inner_pad[index] ^= padded_key[index];
        outer_pad[index] ^= padded_key[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad.as_slice());
    inner.update(bytes);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad.as_slice());
    outer.update(inner_hash);
    Ok(outer.finalize().into())
}

fn verify_mac(key: &[u8], bytes: &[u8], expected: &[u8; 32]) -> FcpResult<()> {
    let actual = calculate_mac(key, bytes)?;
    let difference = actual
        .iter()
        .zip(expected)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right));
    if difference == 0 {
        Ok(())
    } else {
        Err(FcpError::Format("audit HMAC verification failed".into()))
    }
}

fn key_path(directory: &Path) -> PathBuf {
    directory.join("audit-key.dpapi")
}

fn anchor_path(directory: &Path) -> PathBuf {
    directory.join("audit-anchor.dpapi")
}

fn checkpoint_path(directory: &Path) -> PathBuf {
    directory.join("audit-checkpoint.dpapi")
}

fn audit_day(path: &Path) -> Option<u64> {
    path.file_name()
        .and_then(|value| value.to_str())
        .and_then(|name| name.strip_prefix("audit-v2-day-"))
        .and_then(|name| name.strip_suffix(".jsonl"))
        .and_then(|day| day.parse().ok())
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02x}")).collect()
}

fn decode_mac(value: &str) -> FcpResult<[u8; 32]> {
    if value.len() != 64 {
        return Err(FcpError::Format("audit MAC has invalid length".into()));
    }
    let mut decoded = [0u8; 32];
    for (index, slot) in decoded.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| FcpError::Format("audit MAC is not lowercase hex".into()))?;
    }
    if encode_hex(&decoded) != value {
        return Err(FcpError::Format(
            "audit MAC is not canonical lowercase hex".into(),
        ));
    }
    Ok(decoded)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FailurePoint, fail_next};

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fcp-audit-{label}-{}", Uuid::new_v4()))
    }

    #[test]
    fn authenticated_audit_round_trips_and_excludes_sensitive_fields() {
        let root = root("roundtrip");
        let group = Uuid::new_v4();
        let logger = AuditLogger::open(&root).unwrap();
        logger
            .record(group, "reconciliation", "success", None, Some("isolated"))
            .unwrap();
        drop(logger);
        AuditLogger::open(&root).unwrap();
        let path = chain_files(&root).unwrap().remove(0);
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains(&group.to_string()));
        assert!(!text.contains("cookie_name"));
        assert!(!text.contains("cookie_value"));
        assert!(!text.contains("command_line"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_line_tamper_sequence_regression_and_truncated_tail() {
        let root = root("tamper");
        let logger = AuditLogger::open(&root).unwrap();
        logger
            .record_system("monitor", "observed", Some("baseline"))
            .unwrap();
        logger
            .record_system("monitor", "observed", Some("second"))
            .unwrap();
        drop(logger);
        let path = chain_files(&root).unwrap().remove(0);
        let original = fs::read(&path).unwrap();

        let mut tampered = original.clone();
        let position = tampered.iter().position(|byte| *byte == b's').unwrap();
        tampered[position] = b'x';
        fs::write(&path, tampered).unwrap();
        assert!(AuditLogger::open_strict(&root).is_err());

        fs::write(&path, &original[..original.len() - 1]).unwrap();
        assert!(AuditLogger::open_strict(&root).is_err());
        fs::write(&path, &original).unwrap();

        let mut lines: Vec<&[u8]> = original
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .collect();
        lines.swap(0, 1);
        let reordered = lines
            .into_iter()
            .flat_map(|line| line.iter().copied().chain(*b"\n"))
            .collect::<Vec<_>>();
        fs::write(&path, reordered).unwrap();
        assert!(AuditLogger::open_strict(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn anchor_detects_complete_tail_deletion() {
        let root = root("anchor");
        let logger = AuditLogger::open(&root).unwrap();
        logger
            .record_system("monitor", "observed", Some("one"))
            .unwrap();
        logger
            .record_system("monitor", "observed", Some("two"))
            .unwrap();
        drop(logger);
        let path = chain_files(&root).unwrap().remove(0);
        let bytes = fs::read(&path).unwrap();
        let first_end = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        fs::write(path, &bytes[..first_end]).unwrap();
        assert!(AuditLogger::open_strict(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corruption_is_quarantined_and_a_new_chain_records_recovery() {
        let root = root("recovery");
        let logger = AuditLogger::open(&root).unwrap();
        logger
            .record_system("monitor", "observed", Some("one"))
            .unwrap();
        drop(logger);
        let path = chain_files(&root).unwrap().remove(0);
        fs::write(path, b"truncated").unwrap();

        let recovered = AuditLogger::open(&root).unwrap();
        assert!(recovered.recovered_on_open());
        assert_eq!(chain_files(&root).unwrap().len(), 1);
        let parent = root.parent().unwrap();
        let prefix = format!(
            "{}-quarantine-",
            root.file_name().unwrap().to_string_lossy()
        );
        assert!(
            fs::read_dir(parent)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        );
        drop(recovered);
        fs::remove_dir_all(&root).unwrap();
        for entry in fs::read_dir(parent).unwrap().filter_map(Result::ok) {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                fs::remove_dir_all(entry.path()).unwrap();
            }
        }
    }

    #[test]
    fn verified_export_contains_only_redacted_audit_entries() {
        let root = root("export");
        let export = root.with_extension("jsonl");
        let logger = AuditLogger::open(&root).unwrap();
        logger
            .record_system("monitor", "observed", Some("remote_debugging_port"))
            .unwrap();
        drop(logger);

        AuditLogger::export_verified(&root, &export).unwrap();
        let text = fs::read_to_string(&export).unwrap();
        assert!(text.contains("remote_debugging_port"));
        assert!(!text.contains("cookie_value"));

        fs::remove_dir_all(root).unwrap();
        fs::remove_file(export).unwrap();
    }

    #[test]
    fn injected_failure_before_append_leaves_the_chain_unchanged() {
        let root = root("before-append-failure");
        let logger = AuditLogger::open(&root).unwrap();
        logger
            .record_system("monitor", "observed", Some("baseline"))
            .unwrap();
        let before = fs::read(&chain_files(&root).unwrap()[0]).unwrap();
        let _failure = fail_next(FailurePoint::AuditBeforeAppend);

        assert!(
            logger
                .record_system("monitor", "observed", Some("second"))
                .is_err()
        );
        assert_eq!(fs::read(&chain_files(&root).unwrap()[0]).unwrap(), before);
        drop(logger);
        assert!(AuditLogger::open_strict(&root).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn injected_failure_after_append_is_recoverable_but_reports_an_error() {
        let root = root("after-append-failure");
        let logger = AuditLogger::open(&root).unwrap();
        logger
            .record_system("monitor", "observed", Some("baseline"))
            .unwrap();
        let _failure = fail_next(FailurePoint::AuditAfterAppend);

        assert!(
            logger
                .record_system("monitor", "observed", Some("second"))
                .is_err()
        );
        drop(logger);

        let reopened = AuditLogger::open_strict(&root).unwrap();
        assert_eq!(reopened.state.lock().unwrap().sequence, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn record_once_deduplicates_after_append_and_restart() {
        let root = root("record-once");
        let event_id = Uuid::new_v4();
        let group_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let logger = AuditLogger::open(&root).unwrap();
        assert!(
            logger
                .record_once(
                    event_id,
                    group_id,
                    "eviction",
                    "success",
                    Some(operation_id),
                    None
                )
                .unwrap()
        );
        assert!(
            !logger
                .record_once(
                    event_id,
                    group_id,
                    "eviction",
                    "success",
                    Some(operation_id),
                    None
                )
                .unwrap()
        );
        drop(logger);
        let reopened = AuditLogger::open_strict(&root).unwrap();
        assert!(
            !reopened
                .record_once(
                    event_id,
                    group_id,
                    "eviction",
                    "success",
                    Some(operation_id),
                    None
                )
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
