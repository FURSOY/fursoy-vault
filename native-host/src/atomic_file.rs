use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FlushFileBuffers,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, OPEN_EXISTING,
};
use windows::core::PCWSTR;

use crate::{FcpError, FcpResult};

pub(crate) fn write_verified(
    target: &Path,
    bytes: &[u8],
    verify: impl FnOnce(&[u8]) -> FcpResult<()>,
) -> FcpResult<()> {
    let parent = target
        .parent()
        .ok_or_else(|| FcpError::Format("atomic target has no parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(target)?;

    let result = (|| -> FcpResult<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);

        let persisted = fs::read(&temporary)?;
        verify(&persisted)?;
        atomic_replace(&temporary, target)?;
        sync_directory(parent)?;
        Ok(())
    })();

    if result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(target: &Path) -> FcpResult<std::path::PathBuf> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| FcpError::Format("atomic target has no UTF-8 file name".into()))?;
    Ok(target.with_file_name(format!("{file_name}.tmp")))
}

fn atomic_replace(source: &Path, target: &Path) -> FcpResult<()> {
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

fn sync_directory(directory: &Path) -> FcpResult<()> {
    let directory_wide = wide_path(directory);
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

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
