use std::fs;
use std::path::Path;

use crate::{FcpError, FcpResult};

/// Exclusive, process-lifetime lock over the data directory.
///
/// Two hosts running at once corrupt the HMAC-chained audit log, and a corrupted chain makes every
/// later start fail closed until a human moves the directory aside (PLAN §23.1). Chrome normally
/// owns exactly one host process, but a reconnect race or a manually launched binary can break that
/// assumption. Refusing the second instance turns a destructive, manual-recovery failure into an
/// ordinary connection error the extension already retries.
///
/// Both platforms rely on the kernel dropping the lock when the process dies, however it dies: a
/// lock that outlived a crashed host would wedge the directory permanently, which is worse than
/// the problem it prevents.
pub struct InstanceLock {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    /// Held only for its side effect: closing the file releases the `flock`.
    #[cfg(unix)]
    _file: fs::File,
}

impl InstanceLock {
    #[cfg(windows)]
    pub fn acquire(root: &Path) -> FcpResult<Self> {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_DELETE_ON_CLOSE,
            FILE_GENERIC_WRITE, FILE_SHARE_MODE,
        };
        use windows::core::PCWSTR;

        let path = lock_path(root)?;
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
        .map_err(|_| busy())?;
        Ok(Self { handle })
    }

    /// `flock(LOCK_EX | LOCK_NB)` is the POSIX counterpart to the unshared Windows handle: it is
    /// advisory, but every party contending here is this same binary, and unlike `fcntl` locks it
    /// is owned by the open file description rather than the process, so it is not silently
    /// dropped when an unrelated descriptor to the same file is closed.
    ///
    /// The lock file is left behind on exit rather than deleted. Removing it would race a second
    /// host that already opened it — it would then hold a lock on an unlinked inode while a third
    /// process locks the recreated file, and both would believe they are alone. An empty leftover
    /// file is harmless; `FILE_FLAG_DELETE_ON_CLOSE` on Windows avoids the same race differently.
    #[cfg(unix)]
    pub fn acquire(root: &Path) -> FcpResult<Self> {
        use std::os::unix::io::AsRawFd;

        let path = lock_path(root)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        // SAFETY: `file` owns the descriptor for the duration of the call.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            return Err(busy());
        }
        Ok(Self { _file: file })
    }
}

fn lock_path(root: &Path) -> FcpResult<std::path::PathBuf> {
    fs::create_dir_all(root)?;
    Ok(root.join("host.lock"))
}

fn busy() -> FcpError {
    FcpError::Protocol("another native host instance already owns the data directory".into())
}

#[cfg(windows)]
impl Drop for InstanceLock {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
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
