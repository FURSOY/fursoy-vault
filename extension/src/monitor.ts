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
  | "monitor_queue_overflow";

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
    case "audit_integrity_recovered": return "high";
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

export function notificationText(event: MonitorEvent, activeScopes: readonly string[] = []): { title: string; message: string } {
  switch (event.signal) {
    case "remote_debugging_port":
    case "remote_debugging_pipe": {
      // The signal itself is process-wide, not tied to a group (§9.3), so the affected sites are
      // whatever is currently leased at display time rather than something carried on the event.
      const exposure = activeScopes.length > 0
        ? ` Şu an açık korumalı site(ler): ${activeScopes.join(", ")}.`
        : " Şu an açık korumalı site yok.";
      return { title: "Yüksek riskli tarayıcı başlatması", message: `Chrome uzaktan hata ayıklama parametresiyle başlatıldı.${exposure}` };
    }
    case "host_disconnect_active_lease":
      return { title: "Cookie koruması bağlantısı kesildi", message: "Açık bir oturum varken native host bağlantısı kayboldu; fail-closed temizlik başlatıldı." };
    case "reconciliation_failed":
      return { title: "Oturum uzlaştırması başarısız", message: "Korunan bir hesap grubu güvenli duruma getirilemedi." };
    case "audit_integrity_recovered":
      return { title: "Audit bütünlüğü bozuldu", message: "Hasarlı audit zinciri kanıt olarak karantinaya alındı ve yeni bir zincir başlatıldı." };
    case "lease_outside_cookie_created":
      return { title: "Lease dışında cookie oluştu", message: "Kilitli bir hesap grubunda cookie yeniden oluştu ve tahliye işlemi başlatıldı." };
    case "monitor_queue_overflow":
      return { title: "Monitoring kuyruğu doldu", message: "Bazı eski monitoring olayları bounded kuyruktan çıkarıldı." };
    case "process_inspection_wmi_connect_access_denied":
    case "process_inspection_wmi_connect_failed":
    case "process_inspection_wmi_poll_access_denied":
    case "process_inspection_wmi_poll_failed":
    case "process_inspection_command_line_access_denied":
    case "process_inspection_command_line_unavailable":
    case "host_disconnect":
    case "reconnect_success":
    case "selector_changed":
      return { title: "FURSOY monitoring bilgisi", message: "Redakte bir monitoring olayı audit kaydına işlendi." };
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
    "monitor_queue_overflow",
  ].includes(value);
}
