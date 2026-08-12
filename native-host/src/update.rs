use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateManager, VelopackApp};

use crate::{FcpError, FcpResult};

const UPDATE_REPOSITORY: &str = "https://github.com/FURSOY/fursoy-vault";
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const UPDATER_PATH_FILE: &str = "updater-path.txt";
const LAST_TRIGGER_FILE: &str = "update-last-trigger";

/// Velopack must see its lifecycle arguments before the native-messaging argument parser. Auto
/// apply is deliberately disabled: the installed Velopack copy updates itself and deploys the
/// host side-by-side through the fast callbacks, so a live Chrome-owned host is never replaced.
pub fn run_startup_hooks() {
    run_startup_hooks_with_args(std::env::args().skip(1).collect());
}

fn run_startup_hooks_with_args(args: Vec<String>) {
    let executable = std::env::current_exe().ok();
    let install_executable = executable.clone();
    let update_executable = executable.clone();
    let mut app = VelopackApp::build()
        .set_args(args)
        .set_auto_apply_on_startup(false);

    #[cfg(target_os = "windows")]
    {
        app = app
            .on_after_install_fast_callback(move |_| {
                run_required_release_script(install_executable.as_deref(), "install.ps1", true)
            })
            .on_after_update_fast_callback(move |_| {
                run_required_release_script(update_executable.as_deref(), "install.ps1", true)
            })
            .on_before_uninstall_fast_callback(move |_| {
                run_required_release_script(executable.as_deref(), "uninstall.ps1", false)
            });
    }

    app.run();
}

/// Runs in the Velopack-managed copy, never in the Chrome stdio process. Download verification is
/// performed by Velopack before apply. The update callback then reuses the existing atomic
/// side-by-side installer to switch the Native Messaging manifest to the new host.
pub fn check_and_apply() -> FcpResult<()> {
    let source = GithubSource::new(UPDATE_REPOSITORY, None, false);
    let manager = UpdateManager::new(source, None, None)
        .map_err(|_| FcpError::Protocol("companion updater is not installed".into()))?;

    if let Some(pending) = manager.get_update_pending_restart() {
        if all_profiles_are_safe_for_update()? {
            manager
                .apply_updates_and_restart(&pending)
                .map_err(redacted_update_error)?;
        }
        return Ok(());
    }

    match manager.check_for_updates().map_err(redacted_update_error)? {
        UpdateCheck::UpdateAvailable(update) => {
            manager
                .download_updates(&update, None)
                .map_err(redacted_update_error)?;
            if all_profiles_are_safe_for_update()? {
                manager
                    .apply_updates_and_restart(&update.TargetFullRelease)
                    .map_err(redacted_update_error)?;
            }
        }
        UpdateCheck::RemoteIsEmpty | UpdateCheck::NoUpdateAvailable => {}
    }
    Ok(())
}

/// A Chrome-launched host only triggers the installed updater and immediately returns to protocol
/// work. Network access, package extraction and installation happen in a detached process whose
/// stdin/stdout cannot corrupt Native Messaging framing.
pub fn trigger_background_check() {
    if std::env::var_os("FCP_DATA_DIR").is_some() {
        return;
    }
    let Some(data_root) = production_data_root() else {
        return;
    };
    let now = unix_seconds(SystemTime::now());
    let trigger_path = data_root.join(LAST_TRIGGER_FILE);
    let previous = fs::read_to_string(&trigger_path)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok());
    if !should_trigger(previous, now, UPDATE_CHECK_INTERVAL.as_secs()) {
        return;
    }
    let Some(updater) = read_valid_updater_path(&data_root) else {
        return;
    };
    if fs::create_dir_all(&data_root).is_err()
        || fs::write(&trigger_path, format!("{now}\n")).is_err()
    {
        return;
    }

    let mut command = Command::new(updater);
    command
        .arg("--check-update")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    if command.spawn().is_err() {
        let _ = fs::remove_file(trigger_path);
    }
}

/// Applying an update is intentionally conservative. The package may be downloaded while the
/// browser is active, but a profile with an exposed cookie lease or a nonterminal durable
/// operation prevents manifest activation until a later host connection observes a safe state.
pub fn profile_is_safe_for_update(data_root: &Path) -> FcpResult<bool> {
    let leases = data_root.join("leases").join("groups");
    if leases.exists() {
        for entry in fs::read_dir(leases)? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let value: serde_json::Value = serde_json::from_slice(&fs::read(entry.path())?)?;
            let state = value.get("state").and_then(serde_json::Value::as_str);
            if matches!(state, Some("leased" | "evicting" | "injecting")) {
                return Ok(false);
            }
        }
    }

    let operations = data_root.join("operations").join("groups");
    if operations.exists() {
        for entry in fs::read_dir(operations)? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let value: serde_json::Value = serde_json::from_slice(&fs::read(entry.path())?)?;
            let Some(records) = value
                .get("operations")
                .and_then(serde_json::Value::as_array)
            else {
                return Ok(false);
            };
            if records.iter().any(|record| {
                !matches!(
                    record.get("phase").and_then(serde_json::Value::as_str),
                    Some("completed" | "aborted" | "reconciliation_required")
                )
            }) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn all_profiles_are_safe_for_update() -> FcpResult<bool> {
    let Some(root) = production_data_root() else {
        return Ok(false);
    };
    if !profile_is_safe_for_update(&root)? {
        return Ok(false);
    }
    let profiles = root.join("profiles");
    if !profiles.exists() {
        return Ok(true);
    }
    for entry in fs::read_dir(profiles)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && !profile_is_safe_for_update(&entry.path())? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn production_data_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("FursoyVault"))
}

fn read_valid_updater_path(data_root: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(data_root.join("native-host").join(UPDATER_PATH_FILE)).ok()?;
    let candidate = PathBuf::from(raw.trim());
    let local_app_data = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    let expected_name = std::env::current_exe().ok()?.file_name()?.to_owned();
    if !candidate.is_absolute()
        || !candidate.starts_with(local_app_data)
        || candidate.file_name() != Some(expected_name.as_os_str())
        || !candidate.is_file()
    {
        return None;
    }
    Some(candidate)
}

fn should_trigger(previous: Option<u64>, now: u64, interval: u64) -> bool {
    previous.is_none_or(|value| now.saturating_sub(value) >= interval)
}

fn unix_seconds(value: SystemTime) -> u64 {
    value
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn redacted_update_error(_: velopack::Error) -> FcpError {
    FcpError::Protocol("companion update failed".into())
}

#[cfg(target_os = "windows")]
fn run_required_release_script(executable: Option<&Path>, name: &str, install: bool) {
    let result = (|| -> std::io::Result<()> {
        let executable = executable.ok_or_else(|| std::io::Error::other("missing executable"))?;
        let script = executable
            .parent()
            .ok_or_else(|| std::io::Error::other("missing executable directory"))?
            .join(name);
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]);
        command.arg(script);
        if install {
            command.arg("-UpdaterPath").arg(executable);
        }
        let status = command.status()?;
        if !status.success() {
            return Err(std::io::Error::other("release lifecycle script failed"));
        }
        Ok(())
    })();
    if result.is_err() {
        eprintln!("FURSOY Vault installation lifecycle failed");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_trigger_is_due_only_after_the_interval() {
        assert!(should_trigger(None, 100, 50));
        assert!(!should_trigger(Some(80), 100, 50));
        assert!(should_trigger(Some(50), 100, 50));
    }

    #[test]
    fn clock_rollback_does_not_cause_an_update_check_loop() {
        assert!(!should_trigger(Some(200), 100, 50));
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fcp-update-{label}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn active_lease_blocks_update_activation() {
        let root = test_root("lease");
        let leases = root.join("leases/groups");
        fs::create_dir_all(&leases).unwrap();
        fs::write(leases.join("group.json"), br#"{"state":"leased"}"#).unwrap();

        assert!(!profile_is_safe_for_update(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nonterminal_journal_blocks_update_activation() {
        let root = test_root("operation");
        let operations = root.join("operations/groups");
        fs::create_dir_all(&operations).unwrap();
        fs::write(
            operations.join("group.json"),
            br#"{"operations":[{"phase":"browser_removal_pending"}]}"#,
        )
        .unwrap();

        assert!(!profile_is_safe_for_update(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sealed_terminal_profile_allows_update_activation() {
        let root = test_root("safe");
        let leases = root.join("leases/groups");
        let operations = root.join("operations/groups");
        fs::create_dir_all(&leases).unwrap();
        fs::create_dir_all(&operations).unwrap();
        fs::write(leases.join("group.json"), br#"{"state":"sealed"}"#).unwrap();
        fs::write(
            operations.join("group.json"),
            br#"{"operations":[{"phase":"completed"}]}"#,
        )
        .unwrap();

        assert!(profile_is_safe_for_update(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_native_messaging_arguments_are_ignored_by_velopack() {
        run_startup_hooks_with_args(vec!["chrome-extension://test/".into()]);
    }
}
