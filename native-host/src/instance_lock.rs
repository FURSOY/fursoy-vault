use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_DELETE_ON_CLOSE,
    FILE_GENERIC_WRITE, FILE_SHARE_MODE,
};
use windows::core::PCWSTR;

use crate::{FcpError, FcpResult};

/// Exclusive, process-lifetime lock over the data directory.
///
/// Two hosts running at once corrupt the HMAC-chained audit log, and a corrupted chain makes every
/// later start fail closed until a human moves the directory aside (PLAN §23.1). Chrome normally
/// owns exactly one host process, but a reconnect race or a manually launched binary can break that
/// assumption. Refusing the second instance turns a destructive, manual-recovery failure into an
/// ordinary connection error the extension already retries.
pub struct InstanceLock {
    handle: HANDLE,
}

impl InstanceLock {
    pub fn acquire(root: &Path) -> FcpResult<Self> {
        fs::create_dir_all(root)?;
        let path = root.join("host.lock");
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_GENERIC_WRITE.0,
                // No sharing: a second CreateFileW on the same path fails while this handle lives.
                FILE_SHARE_MODE(0),
                None,
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_DELETE_ON_CLOSE,
                None,
            )
        }
        .map_err(|_| {
            FcpError::Protocol(
                "another native host instance already owns the data directory".into(),
            )
        })?;
        Ok(Self { handle })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn second_instance_is_refused_while_the_first_holds_the_lock() {
        let root = std::env::temp_dir().join(format!("fcp-lock-{}", Uuid::new_v4()));
        let first = InstanceLock::acquire(&root).unwrap();
        assert!(InstanceLock::acquire(&root).is_err());
        drop(first);
        // Releasing must make the directory usable again rather than wedging it permanently.
        let second = InstanceLock::acquire(&root).unwrap();
        drop(second);
        fs::remove_dir_all(&root).unwrap();
    }
}
