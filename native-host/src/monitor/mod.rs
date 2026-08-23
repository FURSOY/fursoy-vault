/// Process observation is Windows-only. It identifies a trusted browser by Authenticode
/// signature, and Linux has no equivalent to verify against, so rather than ship a weaker check
/// under the same name the capability is simply not advertised there (see `supports_monitoring`).
#[cfg(windows)]
mod process;

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use crate::audit::AuditLogger;
use crate::audit::unix_ms;
use crate::protocol::messages::{
    Message, MonitorAlert, MonitorEvent, MonitorPoll, MonitorSignal, MonitorSource,
};
use crate::{FcpError, FcpResult};

const MAX_PENDING_EVENTS: usize = 128;
const MAX_POLL_EVENTS: u16 = 64;
const MAX_ACCEPTED_EVENT_IDS: usize = 512;

pub struct MonitorEngine {
    pending: Arc<Mutex<VecDeque<MonitorEvent>>>,
    accepted_event_ids: HashSet<uuid::Uuid>,
    accepted_event_order: VecDeque<uuid::Uuid>,
    #[cfg(windows)]
    _process_observer: Option<process::ProcessObserver>,
}

impl Default for MonitorEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MonitorEngine {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(VecDeque::new())),
            accepted_event_ids: HashSet::new(),
            accepted_event_order: VecDeque::new(),
            #[cfg(windows)]
            _process_observer: None,
        }
    }

    /// Whether this build can observe processes at all. The extension asks before offering the
    /// monitor-only policy, so a platform without it never presents a protection level that would
    /// silently do nothing.
    pub const fn supports_monitoring() -> bool {
        cfg!(windows)
    }

    pub fn start() -> Self {
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        #[cfg(windows)]
        let observer = process::ProcessObserver::start(Arc::clone(&pending));
        Self {
            pending,
            accepted_event_ids: HashSet::new(),
            accepted_event_order: VecDeque::new(),
            #[cfg(windows)]
            _process_observer: Some(observer),
        }
    }

    pub fn accept_extension_event(
        &mut self,
        event: MonitorEvent,
        audit: &AuditLogger,
    ) -> FcpResult<Vec<Message>> {
        event.validate()?;
        if event.source != MonitorSource::Extension {
            return Err(FcpError::Protocol(
                "extension monitor event has invalid source".into(),
            ));
        }
        if self.accepted_event_ids.contains(&event.event_id) {
            return Ok(vec![Message::MonitorAlert(MonitorAlert { event })]);
        }
        audit_event(audit, &event)?;
        self.remember_event(event.event_id);
        Ok(vec![Message::MonitorAlert(MonitorAlert { event })])
    }

    pub fn enqueue_host_event(&self, event: MonitorEvent) -> FcpResult<()> {
        event.validate()?;
        if event.source != MonitorSource::NativeHost {
            return Err(FcpError::Protocol(
                "native monitor event has invalid source".into(),
            ));
        }
        enqueue_bounded(&self.pending, event)
    }

    pub fn poll(&mut self, request: MonitorPoll, audit: &AuditLogger) -> FcpResult<Vec<Message>> {
        if request.max_events == 0 || request.max_events > MAX_POLL_EVENTS {
            return Err(FcpError::Protocol(
                "monitor poll max_events is out of range".into(),
            ));
        }
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| FcpError::Format("monitor event queue lock poisoned".into()))?;
        let mut messages = Vec::new();
        for _ in 0..request.max_events {
            let Some(event) = pending.pop_front() else {
                break;
            };
            audit_event(audit, &event)?;
            messages.push(Message::MonitorAlert(MonitorAlert { event }));
        }
        Ok(messages)
    }

    fn remember_event(&mut self, event_id: uuid::Uuid) {
        if self.accepted_event_ids.len() == MAX_ACCEPTED_EVENT_IDS
            && let Some(oldest) = self.accepted_event_order.pop_front()
        {
            self.accepted_event_ids.remove(&oldest);
        }
        self.accepted_event_ids.insert(event_id);
        self.accepted_event_order.push_back(event_id);
    }
}

fn enqueue_bounded(
    pending: &Arc<Mutex<VecDeque<MonitorEvent>>>,
    event: MonitorEvent,
) -> FcpResult<()> {
    let mut queue = pending
        .lock()
        .map_err(|_| FcpError::Format("monitor event queue lock poisoned".into()))?;
    if queue.len() == MAX_PENDING_EVENTS {
        queue.pop_front();
        if event.signal != MonitorSignal::MonitorQueueOverflow {
            queue.push_back(MonitorEvent {
                event_id: uuid::Uuid::new_v4(),
                observed_at_unix_ms: unix_ms()?,
                source: MonitorSource::NativeHost,
                signal: MonitorSignal::MonitorQueueOverflow,
                severity: MonitorSignal::MonitorQueueOverflow.severity(),
                account_group_id: None,
                occurrence_count: 1,
            });
            queue.pop_front();
        }
    }
    queue.push_back(event);
    Ok(())
}

fn audit_event(audit: &AuditLogger, event: &MonitorEvent) -> FcpResult<()> {
    let outcome = match event.severity {
        crate::protocol::messages::MonitorSeverity::Info => "info",
        crate::protocol::messages::MonitorSeverity::Medium => "warning",
        crate::protocol::messages::MonitorSeverity::High => "high",
    };
    if let Some(group_id) = event.account_group_id {
        audit.record(
            group_id,
            "monitor",
            outcome,
            Some(event.event_id),
            Some(event.signal.audit_code()),
        )
    } else {
        audit.record_system("monitor", outcome, Some(event.signal.audit_code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::unix_ms;
    use crate::protocol::messages::{MonitorSeverity, MonitorSignal};
    use uuid::Uuid;

    #[test]
    fn bounded_poll_and_source_validation() {
        let mut engine = MonitorEngine::new();
        let event = MonitorEvent {
            event_id: Uuid::new_v4(),
            observed_at_unix_ms: unix_ms().unwrap(),
            source: MonitorSource::NativeHost,
            signal: MonitorSignal::RemoteDebuggingPort,
            severity: MonitorSeverity::High,
            account_group_id: None,
            occurrence_count: 1,
        };
        engine.enqueue_host_event(event).unwrap();
        assert!(
            engine
                .poll(MonitorPoll { max_events: 0 }, &test_audit())
                .is_err()
        );
        let output = engine
            .poll(MonitorPoll { max_events: 1 }, &test_audit())
            .unwrap();
        assert_eq!(output.len(), 1);
    }

    #[test]
    fn duplicate_extension_event_is_acknowledged_without_second_audit_entry() {
        let mut engine = MonitorEngine::new();
        let audit = test_audit();
        let event = MonitorEvent {
            event_id: Uuid::new_v4(),
            observed_at_unix_ms: unix_ms().unwrap(),
            source: MonitorSource::Extension,
            signal: MonitorSignal::HostDisconnect,
            severity: MonitorSeverity::Info,
            account_group_id: None,
            occurrence_count: 1,
        };
        assert_eq!(
            engine
                .accept_extension_event(event.clone(), &audit)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            engine.accept_extension_event(event, &audit).unwrap().len(),
            1
        );
        assert_eq!(engine.accepted_event_ids.len(), 1);
    }

    #[test]
    fn host_queue_overflow_is_explicitly_reported() {
        let mut engine = MonitorEngine::new();
        for _ in 0..=MAX_PENDING_EVENTS {
            engine
                .enqueue_host_event(MonitorEvent {
                    event_id: Uuid::new_v4(),
                    observed_at_unix_ms: unix_ms().unwrap(),
                    source: MonitorSource::NativeHost,
                    signal: MonitorSignal::ProcessInspectionWmiPollFailed,
                    severity: MonitorSeverity::Info,
                    account_group_id: None,
                    occurrence_count: 1,
                })
                .unwrap();
        }
        let audit = test_audit();
        let mut output = engine.poll(MonitorPoll { max_events: 64 }, &audit).unwrap();
        output.extend(engine.poll(MonitorPoll { max_events: 64 }, &audit).unwrap());
        assert!(output.iter().any(|message| matches!(
            message,
            Message::MonitorAlert(MonitorAlert { event })
                if event.signal == MonitorSignal::MonitorQueueOverflow
        )));
    }

    fn test_audit() -> AuditLogger {
        let root = std::env::temp_dir().join(format!("fcp-monitor-audit-{}", Uuid::new_v4()));
        AuditLogger::open(root).unwrap()
    }
}
