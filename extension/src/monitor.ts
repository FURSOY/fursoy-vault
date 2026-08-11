import { translate, type Locale } from "./i18n.js";

export type MonitorSeverity = "info" | "medium" | "high";
export type MonitorSource = "extension" | "native_host";
export type MonitorSignal =
  | "remote_debugging_port"
  | "remote_debugging_pipe"
  | "process_inspection_wmi_connect_access_denied"
  | "process_inspection_wmi_connect_failed"
  | "process_inspection_wmi_poll_access_denied"
  | "process_inspection_wmi_poll_failed"
  | "process_inspection_command_line_access_denied"
  | "process_inspection_command_line_unavailable"
  | "host_disconnect"
  | "host_disconnect_active_lease"
  | "reconnect_success"
  | "reconciliation_failed"
  | "audit_integrity_recovered"
  | "lease_outside_cookie_created"
  | "selector_changed"
  | "monitor_queue_overflow"
  | "permission_missing";

export interface MonitorEvent {
  event_id: string;
  observed_at_unix_ms: number;
  source: MonitorSource;
  signal: MonitorSignal;
  severity: MonitorSeverity;
  account_group_id?: string | null;
  occurrence_count: number;
}

export interface NotificationDecisionState {
  [fingerprint: string]: number;
}

export const MONITOR_OUTBOX_LIMIT = 128;
export const MONITOR_RATE_LIMIT_MS = 10 * 60_000;

export function signalSeverity(signal: MonitorSignal): MonitorSeverity {
  switch (signal) {
    case "remote_debugging_port":
    case "remote_debugging_pipe":
    case "host_disconnect_active_lease":
    case "reconciliation_failed":
    case "audit_integrity_recovered":
    case "permission_missing": return "high";
    case "lease_outside_cookie_created":
    case "monitor_queue_overflow": return "medium";
    case "process_inspection_wmi_connect_access_denied":
    case "process_inspection_wmi_connect_failed":
    case "process_inspection_wmi_poll_access_denied":
    case "process_inspection_wmi_poll_failed":
    case "process_inspection_command_line_access_denied":
    case "process_inspection_command_line_unavailable":
    case "host_disconnect":
    case "reconnect_success":
    case "selector_changed": return "info";
  }
}

export function makeMonitorEvent(signal: MonitorSignal, accountGroupId?: string): MonitorEvent {
  return {
    event_id: crypto.randomUUID(),
    observed_at_unix_ms: Date.now(),
    source: "extension",
    signal,
    severity: signalSeverity(signal),
    account_group_id: accountGroupId,
    occurrence_count: 1,
  };
}

export function validateMonitorEvent(value: unknown): value is MonitorEvent {
  if (typeof value !== "object" || value === null) return false;
  const event = value as Partial<MonitorEvent>;
  return typeof event.event_id === "string" && event.event_id.length > 0 &&
    typeof event.observed_at_unix_ms === "number" && Number.isFinite(event.observed_at_unix_ms) &&
    (event.source === "extension" || event.source === "native_host") &&
    typeof event.signal === "string" && isMonitorSignal(event.signal) &&
    event.severity === signalSeverity(event.signal) &&
    (event.account_group_id === undefined || event.account_group_id === null || typeof event.account_group_id === "string") &&
    typeof event.occurrence_count === "number" && Number.isInteger(event.occurrence_count) &&
    event.occurrence_count >= 1 && event.occurrence_count <= 1_000;
}

export function addToBoundedOutbox(outbox: readonly MonitorEvent[], event: MonitorEvent): MonitorEvent[] {
  const result = outbox.map((item) => ({ ...item }));
  const existing = result.find((item) => item.signal === event.signal && (item.account_group_id ?? null) === (event.account_group_id ?? null));
  if (existing !== undefined) {
    existing.observed_at_unix_ms = event.observed_at_unix_ms;
    existing.occurrence_count = Math.min(1_000, existing.occurrence_count + event.occurrence_count);
    return result;
  }
  result.push(event);
  return result.slice(-MONITOR_OUTBOX_LIMIT);
}

export function notificationDecision(
  event: MonitorEvent,
  previous: NotificationDecisionState,
  now: number,
): { show: boolean; next: NotificationDecisionState } {
  if (event.severity === "info") return { show: false, next: previous };
  const fingerprint = `${event.signal}\u0000${event.account_group_id ?? "system"}`;
  const last = previous[fingerprint];
  if (last !== undefined && now - last < MONITOR_RATE_LIMIT_MS) return { show: false, next: previous };
  return { show: true, next: { ...previous, [fingerprint]: now } };
}

export function notificationText(
  event: MonitorEvent,
  activeScopes: readonly string[] = [],
  locale: Locale = "tr",
): { title: string; message: string } {
  const t = (key: string, params?: Readonly<Record<string, string | number>>): string => translate(locale, key, params);
  switch (event.signal) {
    case "remote_debugging_port":
    case "remote_debugging_pipe": {
      // The signal itself is process-wide, not tied to a group (§9.3), so the affected sites are
      // whatever is currently leased at display time rather than something carried on the event.
      const message = activeScopes.length > 0
        ? t("monitor.remoteDebugging.messageWithScopes", { scopes: activeScopes.join(", ") })
        : t("monitor.remoteDebugging.messageNoScopes");
      return { title: t("monitor.remoteDebugging.title"), message };
    }
    case "host_disconnect_active_lease":
      return { title: t("monitor.hostDisconnectActiveLease.title"), message: t("monitor.hostDisconnectActiveLease.message") };
    case "reconciliation_failed":
      return { title: t("monitor.reconciliationFailed.title"), message: t("monitor.reconciliationFailed.message") };
    case "audit_integrity_recovered":
      return { title: t("monitor.auditIntegrityRecovered.title"), message: t("monitor.auditIntegrityRecovered.message") };
    case "lease_outside_cookie_created":
      return { title: t("monitor.leaseOutsideCookieCreated.title"), message: t("monitor.leaseOutsideCookieCreated.message") };
    case "monitor_queue_overflow":
      return { title: t("monitor.queueOverflow.title"), message: t("monitor.queueOverflow.message") };
    case "permission_missing":
      return { title: t("monitor.permissionMissing.title"), message: t("monitor.permissionMissing.message") };
    case "process_inspection_wmi_connect_access_denied":
    case "process_inspection_wmi_connect_failed":
    case "process_inspection_wmi_poll_access_denied":
    case "process_inspection_wmi_poll_failed":
    case "process_inspection_command_line_access_denied":
    case "process_inspection_command_line_unavailable":
    case "host_disconnect":
    case "reconnect_success":
    case "selector_changed":
      return { title: t("monitor.info.title"), message: t("monitor.info.message") };
  }
}

function isMonitorSignal(value: string): value is MonitorSignal {
  return [
    "remote_debugging_port", "remote_debugging_pipe",
    "process_inspection_wmi_connect_access_denied", "process_inspection_wmi_connect_failed",
    "process_inspection_wmi_poll_access_denied", "process_inspection_wmi_poll_failed",
    "process_inspection_command_line_access_denied", "process_inspection_command_line_unavailable",
    "host_disconnect", "host_disconnect_active_lease", "reconnect_success",
    "reconciliation_failed", "audit_integrity_recovered", "lease_outside_cookie_created", "selector_changed",
    "monitor_queue_overflow", "permission_missing",
  ].includes(value);
}
