use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use uuid::Uuid;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY,
};
use windows::core::{BSTR, GUID, PCWSTR};

use super::enqueue_bounded;
use crate::audit::unix_ms;
use crate::protocol::messages::{MonitorEvent, MonitorSignal, MonitorSource};
use crate::{FcpError, FcpResult};

const CLSID_WBEM_LOCATOR: GUID = GUID::from_u128(0x4590f811_1d3a_11d0_891f_00aa004b2e24);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

pub struct ProcessObserver {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ProcessObserver {
    pub fn start(pending: Arc<Mutex<VecDeque<MonitorEvent>>>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("fcp-wmi-process-monitor".into())
            .spawn(move || {
                observe_processes(&pending, &thread_stop);
            })
            .ok();
        Self { stop, thread }
    }
}

impl Drop for ProcessObserver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn observe_processes(pending: &Arc<Mutex<VecDeque<MonitorEvent>>>, stop: &AtomicBool) {
    let _apartment = match ComApartment::initialize() {
        Ok(value) => value,
        Err(error) => {
            let _ = enqueue_signal(pending, connect_failure_signal(&error));
            return;
        }
    };
    let services = match connect_wmi() {
        Ok(value) => value,
        Err(error) => {
            let _ = enqueue_signal(pending, connect_failure_signal(&error));
            return;
        }
    };
    let mut seen = HashSet::new();
    let mut command_line_failures = HashSet::new();
    while !stop.load(Ordering::Acquire) {
        let snapshot = match poll_processes(&services) {
            Ok(value) => value,
            Err(error) => {
                let _ = enqueue_signal(pending, poll_failure_signal(&error));
                break;
            }
        };
        let active: HashSet<u32> = snapshot.iter().map(|(process_id, _)| *process_id).collect();
        seen.retain(|(process_id, _)| active.contains(process_id));
        command_line_failures.retain(|process_id| active.contains(process_id));
        for (process_id, command_line) in snapshot {
            match command_line {
                Ok(value) => enqueue_matches(process_id, &value, pending, &mut seen),
                Err(error) if command_line_failures.insert(process_id) => {
                    let _ = enqueue_signal(pending, command_line_failure_signal(&error));
                }
                Err(_) => {}
            }
        }
        wait_until_next_poll(stop);
    }
}

fn connect_wmi() -> FcpResult<IWbemServices> {
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&CLSID_WBEM_LOCATOR, None, CLSCTX_INPROC_SERVER)? };
    let namespace = BSTR::from("ROOT\\CIMV2");
    let empty = BSTR::new();
    let services =
        unsafe { locator.ConnectServer(&namespace, &empty, &empty, &empty, 0, &empty, None)? };
    unsafe {
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;
    }
    Ok(services)
}

fn poll_processes(services: &IWbemServices) -> FcpResult<Vec<(u32, FcpResult<String>)>> {
    let language = BSTR::from("WQL");
    let query =
        BSTR::from("SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name = 'chrome.exe'");
    let objects = unsafe {
        services.ExecQuery(
            &language,
            &query,
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )?
    };
    let mut snapshot = Vec::new();
    while let Some(object) = next_object(&objects, 2_000)? {
        let process_id = get_u32(&object, windows::core::w!("ProcessId"))?;
        snapshot.push((
            process_id,
            get_string(&object, windows::core::w!("CommandLine")),
        ));
    }
    Ok(snapshot)
}

fn wait_until_next_poll(stop: &AtomicBool) {
    let slices = POLL_INTERVAL.as_millis() / 100;
    for _ in 0..slices {
        if stop.load(Ordering::Acquire) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn connect_failure_signal(error: &FcpError) -> MonitorSignal {
    if is_wbem_access_denied(error) {
        MonitorSignal::ProcessInspectionWmiConnectAccessDenied
    } else {
        MonitorSignal::ProcessInspectionWmiConnectFailed
    }
}

fn poll_failure_signal(error: &FcpError) -> MonitorSignal {
    if is_wbem_access_denied(error) {
        MonitorSignal::ProcessInspectionWmiPollAccessDenied
    } else {
        MonitorSignal::ProcessInspectionWmiPollFailed
    }
}

fn command_line_failure_signal(error: &FcpError) -> MonitorSignal {
    if is_wbem_access_denied(error) {
        MonitorSignal::ProcessInspectionCommandLineAccessDenied
    } else {
        MonitorSignal::ProcessInspectionCommandLineUnavailable
    }
}

fn is_wbem_access_denied(error: &FcpError) -> bool {
    matches!(error, FcpError::Windows(value) if value.code().0 as u32 == 0x8004_1003)
}

fn next_object(
    enumerator: &IEnumWbemClassObject,
    timeout_ms: i32,
) -> FcpResult<Option<IWbemClassObject>> {
    let mut values = [None];
    let mut returned = 0;
    let result = unsafe { enumerator.Next(timeout_ms, &mut values, &mut returned) };
    if result.is_err() {
        return Err(FcpError::Windows(result.into()));
    }
    Ok(if returned == 1 {
        values[0].take()
    } else {
        None
    })
}

fn get_u32(object: &IWbemClassObject, name: PCWSTR) -> FcpResult<u32> {
    let mut value = VARIANT::default();
    unsafe { object.Get(name, 0, &mut value, None, None)? };
    u32::try_from(&value).map_err(FcpError::Windows)
}

fn get_string(object: &IWbemClassObject, name: PCWSTR) -> FcpResult<String> {
    let mut value = VARIANT::default();
    unsafe { object.Get(name, 0, &mut value, None, None)? };
    let value = BSTR::try_from(&value).map_err(FcpError::Windows)?;
    Ok(value.to_string())
}

fn enqueue_matches(
    process_id: u32,
    command_line: &str,
    pending: &Arc<Mutex<VecDeque<MonitorEvent>>>,
    seen: &mut HashSet<(u32, MonitorSignal)>,
) {
    let matches = detect_remote_debugging(command_line);
    for signal in matches {
        if seen.insert((process_id, signal)) {
            let _ = enqueue_signal(pending, signal);
        }
    }
}

fn enqueue_signal(
    pending: &Arc<Mutex<VecDeque<MonitorEvent>>>,
    signal: MonitorSignal,
) -> FcpResult<()> {
    let event = MonitorEvent {
        event_id: Uuid::new_v4(),
        observed_at_unix_ms: unix_ms()?,
        source: MonitorSource::NativeHost,
        signal,
        severity: signal.severity(),
        account_group_id: None,
        occurrence_count: 1,
    };
    enqueue_bounded(pending, event)
}

pub fn detect_remote_debugging(command_line: &str) -> Vec<MonitorSignal> {
    let arguments = split_windows_arguments(command_line);
    let mut matches = Vec::new();
    if has_switch(&arguments, "--remote-debugging-port") {
        matches.push(MonitorSignal::RemoteDebuggingPort);
    }
    if has_switch(&arguments, "--remote-debugging-pipe") {
        matches.push(MonitorSignal::RemoteDebuggingPipe);
    }
    matches
}

fn has_switch(arguments: &[String], expected: &str) -> bool {
    arguments.iter().any(|argument| {
        let normalized = argument.to_ascii_lowercase();
        normalized == expected || normalized.starts_with(&format!("{expected}="))
    })
}

fn split_windows_arguments(command_line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in command_line.chars() {
        match character {
            '"' => quoted = !quoted,
            value if value.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            value => current.push(value),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> FcpResult<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        result.ok().map_err(FcpError::Windows)?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_supported_switch_forms_and_case() {
        let cases = [
            r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --remote-debugging-port=0 --user-data-dir="C:\Temp\FCP Test""#,
            r#"chrome.exe --REMOTE-DEBUGGING-PORT 9222"#,
            r#"chrome.exe --remote-debugging-pipe"#,
        ];
        assert_eq!(
            detect_remote_debugging(cases[0]),
            vec![MonitorSignal::RemoteDebuggingPort]
        );
        assert_eq!(
            detect_remote_debugging(cases[1]),
            vec![MonitorSignal::RemoteDebuggingPort]
        );
        assert_eq!(
            detect_remote_debugging(cases[2]),
            vec![MonitorSignal::RemoteDebuggingPipe]
        );
    }

    #[test]
    fn parser_rejects_substrings_and_normal_chrome_launches() {
        for command in [
            r#"chrome.exe --profile-directory=Default https://example.test/?q=--remote-debugging-port=1"#,
            r#"chrome.exe --disable-features=RemoteDebuggingPort"#,
            r#"chrome.exe --remote-debugging-portable=1"#,
        ] {
            assert!(detect_remote_debugging(command).is_empty(), "{command}");
        }
    }

    #[test]
    fn queue_bound_is_fixed() {
        assert_eq!(super::super::MAX_PENDING_EVENTS, 128);
    }

    #[test]
    fn access_denied_hresult_is_redacted_to_stage_specific_codes() {
        let error = FcpError::Windows(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x8004_1003u32 as i32,
        )));
        assert_eq!(
            connect_failure_signal(&error),
            MonitorSignal::ProcessInspectionWmiConnectAccessDenied
        );
        assert_eq!(
            poll_failure_signal(&error),
            MonitorSignal::ProcessInspectionWmiPollAccessDenied
        );
        assert_eq!(
            command_line_failure_signal(&error),
            MonitorSignal::ProcessInspectionCommandLineAccessDenied
        );
    }

    #[test]
    #[ignore = "requires host WMI permission; run explicitly during Windows acceptance"]
    fn local_wmi_process_poll_reads_a_chrome_command_line() {
        let _apartment = ComApartment::initialize().unwrap();
        let services = connect_wmi().unwrap();
        let snapshot = poll_processes(&services).unwrap();
        assert!(
            !snapshot.is_empty(),
            "Chrome must be running for this acceptance test"
        );
        assert!(
            snapshot
                .iter()
                .any(|(_, command_line)| command_line.is_ok())
        );
    }

    #[test]
    #[ignore = "launch a temporary-profile remote-debugging Chrome before running"]
    fn local_wmi_poll_detects_running_remote_debugging_chrome() {
        let _apartment = ComApartment::initialize().unwrap();
        let services = connect_wmi().unwrap();
        let snapshot = poll_processes(&services).unwrap();
        assert!(snapshot.into_iter().any(|(_, command_line)| {
            command_line.is_ok_and(|value| !detect_remote_debugging(&value).is_empty())
        }));
    }
}
