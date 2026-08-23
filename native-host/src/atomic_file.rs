use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{FcpError, FcpResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DurableWriteResult {
    NotCommitted,
    Committed,
    DurabilityUnknown,
}

#[derive(Debug)]
pub(crate) struct DurableWriteFailure {
    pub(crate) result: DurableWriteResult,
    pub(crate) error: FcpError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectClassification {
    Previous,
    Target,
    Missing,
    Neither,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObjectDigest([u8; 32]);

impl ObjectDigest {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

pub(crate) fn write_verified(
    target: &Path,
    bytes: &[u8],
    verify: impl FnOnce(&[u8]) -> FcpResult<()>,
) -> FcpResult<()> {
    write_verified_durable(target, bytes, verify).map_err(|failure| failure.error)?;
    Ok(())
}

pub(crate) fn write_verified_durable(
    target: &Path,
    bytes: &[u8],
    verify: impl FnOnce(&[u8]) -> FcpResult<()>,
) -> Result<DurableWriteResult, DurableWriteFailure> {
    let parent = target.parent().ok_or_else(|| DurableWriteFailure {
        result: DurableWriteResult::NotCommitted,
        error: FcpError::Format("atomic target has no parent directory".into()),
    })?;
    fs::create_dir_all(parent).map_err(|error| DurableWriteFailure {
        result: DurableWriteResult::NotCommitted,
        error: error.into(),
    })?;
    let temporary = temporary_path(target).map_err(|error| DurableWriteFailure {
        result: DurableWriteResult::NotCommitted,
        error,
    })?;
    let mut replaced = false;

    let result = (|| -> FcpResult<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);

        #[cfg(test)]
        crate::test_support::check(crate::test_support::FailurePoint::AtomicAfterTempSync)?;

        let persisted = fs::read(&temporary)?;
        verify(&persisted)?;
        atomic_replace(&temporary, target)?;
        replaced = true;

        #[cfg(test)]
        crate::test_support::check(crate::test_support::FailurePoint::AtomicAfterReplace)?;

        sync_directory(parent)?;
        Ok(())
    })();

    if result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    match result {
        Ok(()) => Ok(DurableWriteResult::Committed),
        Err(error) => Err(DurableWriteFailure {
            result: if replaced {
                DurableWriteResult::DurabilityUnknown
            } else {
                DurableWriteResult::NotCommitted
            },
            error,
        }),
    }
}

pub(crate) fn classify_object(
    path: &Path,
    previous: Option<ObjectDigest>,
    target: ObjectDigest,
) -> FcpResult<ObjectClassification> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ObjectClassification::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    let actual = ObjectDigest::of(&bytes);
    if actual == target {
        return Ok(ObjectClassification::Target);
    }
    if previous == Some(actual) {
        return Ok(ObjectClassification::Previous);
    }
    Ok(ObjectClassification::Neither)
}

fn temporary_path(target: &Path) -> FcpResult<std::path::PathBuf> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| FcpError::Format("atomic target has no UTF-8 file name".into()))?;
    Ok(target.with_file_name(format!("{file_name}.tmp")))
}

/// Replaces `target` with `source` in one step, so a reader never observes a partial file and a
/// crash leaves either the old contents or the new ones.
#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> FcpResult<()> {
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    let source_wide = wide_path(source);
    let target_wide = wide_path(target);
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(target_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}

/// POSIX `rename` is atomic over an existing target by definition, so this needs none of the
/// flags the Windows path spells out. Durability is not implied — that is what `sync_directory`
/// below is for, exactly as on Windows.
#[cfg(unix)]
fn atomic_replace(source: &Path, target: &Path) -> FcpResult<()> {
    fs::rename(source, target)?;
    Ok(())
}

/// Flushes the *directory entry*, not the file: the contents were already synced when the
/// temporary file was written, but the rename that published them is itself only durable once the
/// directory is flushed. Skipping this can leave a crash-recovered directory pointing at neither
/// name.
#[cfg(windows)]
fn sync_directory(directory: &Path) -> FcpResult<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FlushFileBuffers, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    let directory_wide = wide_path(directory);
    // FILE_FLAG_BACKUP_SEMANTICS is what makes CreateFileW accept a directory at all.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(directory_wide.as_ptr()),
            FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )?
    };
    let flush_result = unsafe { FlushFileBuffers(handle) }.map_err(FcpError::from);
    let close_result = unsafe { CloseHandle(handle) }.map_err(FcpError::from);
    flush_result?;
    close_result?;
    Ok(())
}

/// `fsync` on a directory descriptor is the POSIX equivalent, and opening a directory read-only is
/// enough for it.
#[cfg(unix)]
fn sync_directory(directory: &Path) -> FcpResult<()> {
    std::fs::File::open(directory)?.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FailurePoint, fail_next};
    use uuid::Uuid;

    fn root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("fcp-atomic-{label}-{}", Uuid::new_v4()))
    }

    #[test]
    fn failure_before_replace_preserves_the_previous_target() {
        let root = root("before-replace");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        fs::write(&target, b"old").unwrap();
        let _failure = fail_next(FailurePoint::AtomicAfterTempSync);

        let result = write_verified(&target, b"new", |_| Ok(()));

        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!target.with_file_name("state.json.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn typed_failure_before_replace_is_not_committed() {
        let root = root("typed-before-replace");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        fs::write(&target, b"old").unwrap();
        let _failure = fail_next(FailurePoint::AtomicAfterTempSync);

        let failure = write_verified_durable(&target, b"new", |_| Ok(())).unwrap_err();

        assert_eq!(failure.result, DurableWriteResult::NotCommitted);
        assert_eq!(
            classify_object(
                &target,
                Some(ObjectDigest::of(b"old")),
                ObjectDigest::of(b"new")
            )
            .unwrap(),
            ObjectClassification::Previous
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failure_after_replace_reports_error_with_the_new_target_committed() {
        let root = root("after-replace");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        fs::write(&target, b"old").unwrap();
        let _failure = fail_next(FailurePoint::AtomicAfterReplace);

        let result = write_verified(&target, b"new", |_| Ok(()));

        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!target.with_file_name("state.json.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn typed_failure_after_replace_is_unknown_until_exact_object_classification() {
        let root = root("typed-after-replace");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        fs::write(&target, b"old").unwrap();
        let _failure = fail_next(FailurePoint::AtomicAfterReplace);

        let failure = write_verified_durable(&target, b"new", |_| Ok(())).unwrap_err();

        assert_eq!(failure.result, DurableWriteResult::DurabilityUnknown);
        assert_eq!(
            classify_object(
                &target,
                Some(ObjectDigest::of(b"old")),
                ObjectDigest::of(b"new")
            )
            .unwrap(),
            ObjectClassification::Target
        );
        fs::write(&target, b"unexpected").unwrap();
        assert_eq!(
            classify_object(
                &target,
                Some(ObjectDigest::of(b"old")),
                ObjectDigest::of(b"new")
            )
            .unwrap(),
            ObjectClassification::Neither
        );
        fs::remove_dir_all(root).unwrap();
    }
}
