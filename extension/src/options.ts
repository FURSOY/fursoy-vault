type PolicyLevel = "critical" | "balanced" | "convenient";

interface GroupSummary {
  id: string;
  scope: string;
  displayName: string;
  policyLevel: string;
  state: string;
  hasPermission: boolean;
  cookieCount: number;
}

interface StoredAlert {
  signal: string;
  severity: string;
  accountGroupId: string | null;
  observedAtUnixMs: number;
  occurrences: number;
  affectedScopes?: string[];
}

interface PopupState {
  ok?: boolean;
  connected?: boolean;
  groups?: GroupSummary[];
  error?: string;
  log?: StoredAlert[];
}

const STATE_LABELS: Record<string, string> = {
  uninitialized: "kasa boş",
  sealed: "kilitli",
  unlocking: "açılıyor",
  leased: "açık",
  evicting: "tahliye ediliyor",
  degraded: "sorunlu",
};

const POLICY_LABELS: Record<string, string> = {
  critical: "Kritik",
  balanced: "Dengeli",
  convenient: "Kullanışlı",
  monitor: "İzleme",
};

const SEVERITY_LABELS: Record<string, string> = {
  high: "Yüksek",
  medium: "Orta",
  info: "Bilgi",
};

const SIGNAL_LABELS: Record<string, string> = {
  remote_debugging_port: "Tarayıcı uzaktan hata ayıklama açık başlatıldı",
  remote_debugging_pipe: "Tarayıcı uzaktan hata ayıklama açık başlatıldı",
  host_disconnect_active_lease: "Koruma açıkken native host bağlantısı koptu",
  host_disconnect: "Native host bağlantısı koptu",
  reconnect_success: "Native host bağlantısı geri geldi",
  reconciliation_failed: "Uzlaştırma başarısız oldu",
  lease_outside_cookie_created: "Kilitli sitede çerez oluştu",
  selector_changed: "Korunan çerezlerde değişiklik",
  monitor_queue_overflow: "İzleme kuyruğu doldu, olaylar atlandı",
  process_inspection_unavailable: "Process denetimi yapılamadı",
};

const connectionWarning = required<HTMLElement>("connection");
const errorText = required<HTMLElement>("error");
const groupRows = required<HTMLTableSectionElement>("groups");
const groupsTable = required<HTMLTableElement>("groups-table");
const groupsEmpty = required<HTMLElement>("groups-empty");
const addForm = required<HTMLFormElement>("add-form");
const scopeInput = required<HTMLInputElement>("scope");
const policySelect = required<HTMLSelectElement>("policy");
const addButton = required<HTMLButtonElement>("add");
const logRows = required<HTMLTableSectionElement>("log");
const logTable = required<HTMLTableElement>("log-table");
const logEmpty = required<HTMLElement>("log-empty");
const clearLogButton = required<HTMLButtonElement>("clear-log");

addForm.addEventListener("submit", (event) => { event.preventDefault(); void addSite(); });
clearLogButton.addEventListener("click", () => { void clearLog(); });

void refresh();

async function addSite(): Promise<void> {
  const scope = normalizeScope(scopeInput.value);
  if (scope === "") return showError("Alan adı boş olamaz.");
  if (!scope.includes(".") && scope !== "localhost") {
    return showError("Geçerli bir alan adı gir (ör. ornek.com).");
  }
  addButton.disabled = true;
  try {
    // Requesting inside the submit handler keeps the user-gesture context Chrome requires.
    // Chrome closes the popup the moment the permission prompt opens, so the intent is staged in
    // the service worker first. If this page dies, the background still completes the addition
    // when the grant arrives; if it survives, the call below finishes it immediately.
    await send({ type: "popup.stageProtect", scope, displayName: scope, policyLevel: policySelect.value as PolicyLevel });
    const granted = await requestScopePermission(scope);
    if (!granted) {
      await send({ type: "popup.cancelProtect" });
      return showError("Chrome izni verilmedi; site korumaya alınmadı.");
    }
    const response = await sendAwaitingHost({ type: "popup.protect" });
    if (response?.ok !== true) return showError(describeError(response?.error));
    scopeInput.value = "";
    const before = groupCount();
    await refreshUntil(() => groupCount() !== before);
  } catch { showError("Koruma eklenemedi."); }
  finally { addButton.disabled = false; }
}

async function unprotect(group: GroupSummary): Promise<void> {
  const response = await sendAwaitingHost({ type: "popup.unprotect", groupId: group.id });
  if (response?.ok !== true) return showError(describeError(response?.error));
  const before = groupCount();
  await refreshUntil(() => groupCount() !== before);
}

async function grantPermission(group: GroupSummary): Promise<void> {
  const granted = await requestScopePermission(group.scope);
  if (!granted) return showError("Chrome izni verilmedi.");
  await refresh();
}

async function clearLog(): Promise<void> {
  await send({ type: "popup.clearLog" });
  await refresh();
}

// Every config mutation is answered asynchronously by the host, so the UI polls until the
// change it asked for is actually visible rather than reading state once and trusting it.
let latestGroups: GroupSummary[] = [];

async function refreshUntil(satisfied: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 15; attempt += 1) {
    await refresh();
    if (satisfied() || !errorText.hidden) return;
    await new Promise((resolve) => { setTimeout(resolve, 200); });
  }
}

function groupCount(): number { return latestGroups.length; }

function policyOf(groupId: string): string | undefined {
  return latestGroups.find((group) => group.id === groupId)?.policyLevel;
}

async function refresh(): Promise<void> {
  const state = await send({ type: "popup.state" }) ?? {};
  connectionWarning.hidden = state.connected === true;
  if (state.error !== undefined) showError(describeError(state.error)); else errorText.hidden = true;

  const groups = state.groups ?? [];
  latestGroups = groups;
  groupRows.replaceChildren(...groups.map(renderGroupRow));
  groupsEmpty.hidden = groups.length > 0;
  groupsTable.hidden = groups.length === 0;

  const logResponse = await send({ type: "popup.log" }) ?? {};
  const log = logResponse.log ?? [];
  logRows.replaceChildren(...log.map((entry) => renderLogRow(entry, groups)));
  logEmpty.hidden = log.length > 0;
  logTable.hidden = log.length === 0;
}

function renderGroupRow(group: GroupSummary): HTMLTableRowElement {
  const row = document.createElement("tr");
  row.append(cell(group.scope, "scope"), policyCell(group));
  // A site the host protects but the extension cannot reach is not protected in practice.
  const state = cell(group.hasPermission ? STATE_LABELS[group.state] ?? group.state : "izin yok — koruma çalışmıyor");
  if (!group.hasPermission) state.classList.add("blocked");
  row.append(state, cell(group.hasPermission ? String(group.cookieCount) : "—", "numeric"));

  const actions = document.createElement("td");
  actions.className = "actions";
  if (!group.hasPermission) actions.append(button("İzin ver", () => { void grantPermission(group); }));
  actions.append(button("Kaldır", () => { void unprotect(group); }));
  row.append(actions);
  return row;
}

function policyCell(group: GroupSummary): HTMLTableCellElement {
  const container = document.createElement("td");
  const select = document.createElement("select");
  for (const [value, label] of Object.entries(POLICY_LABELS)) {
    if (value === "monitor") continue;
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    option.selected = value === group.policyLevel;
    select.append(option);
  }
  select.addEventListener("change", () => { void changePolicy(group, select); });
  container.append(select);
  return container;
}

async function changePolicy(group: GroupSummary, select: HTMLSelectElement): Promise<void> {
  const policyLevel = select.value as PolicyLevel;
  if (policyLevel === group.policyLevel) return;
  select.disabled = true;
  try {
    const response = await sendAwaitingHost({ type: "popup.setPolicy", groupId: group.id, policyLevel });
    if (response?.ok !== true) return showError(describeError(response?.error));
    await refreshUntil(() => policyOf(group.id) === policyLevel);
  } finally { select.disabled = false; }
}

function renderLogRow(entry: StoredAlert, groups: GroupSummary[]): HTMLTableRowElement {
  const row = document.createElement("tr");
  const group = groups.find((item) => item.id === entry.accountGroupId);
  const severity = cell(SEVERITY_LABELS[entry.severity] ?? entry.severity);
  severity.className = `severity-${entry.severity}`;
  const site = group?.scope ?? (entry.affectedScopes !== undefined && entry.affectedScopes.length > 0
    ? entry.affectedScopes.join(", ")
    : "—");
  row.append(
    cell(new Date(entry.observedAtUnixMs).toLocaleString(), "numeric"),
    severity,
    cell(SIGNAL_LABELS[entry.signal] ?? entry.signal),
    cell(site),
    cell(String(entry.occurrences), "numeric"),
  );
  return row;
}

function cell(text: string, className?: string): HTMLTableCellElement {
  const element = document.createElement("td");
  element.textContent = text;
  if (className !== undefined) element.className = className;
  return element;
}

function button(label: string, onClick: () => void): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.textContent = label;
  element.addEventListener("click", onClick);
  return element;
}

function normalizeScope(value: string): string {
  return value.trim().replace(/^https?:\/\//, "").replace(/\/.*$/, "").replace(/^\./, "").toLowerCase();
}

function describeError(code: string | undefined): string {
  switch (code) {
    case "scope_overlaps_existing": return "Bu alan adı zaten korunan bir siteyle çakışıyor.";
    case "scope_invalid": return "Alan adı geçersiz.";
    case "group_limit_reached": return "Korunan site sınırına ulaşıldı.";
    case "last_group_cannot_be_removed": return "Son korunan site kaldırılamaz.";
    case "operation_pending": return "Bu site üzerinde bir işlem sürüyor; birazdan tekrar deneyin.";
    case "native_host_not_connected": return "Native host bağlı değil.";
    case "unknown_group": return "Bu site zaten korunmuyor.";
    default: return "İşlem tamamlanamadı.";
  }
}

function showError(message: string): void {
  errorText.textContent = message;
  errorText.hidden = false;
}

function requestScopePermission(scope: string): Promise<boolean> {
  return new Promise((resolve) => {
    chrome.permissions.request({ origins: [`*://${scope}/*`, `*://*.${scope}/*`] }, (granted) => {
      resolve(chrome.runtime.lastError === undefined && granted);
    });
  });
}

// Granting a host permission restarts the service worker, so the native connection is briefly
// down right after the Chrome prompt is accepted. Retrying here rather than waiting inside the
// background message handler avoids a deadlock: the handshake reply is processed on the same
// serialized queue the handler itself runs on.
async function sendAwaitingHost(message: Record<string, unknown>): Promise<PopupState | undefined> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const response = await send(message);
    if (response?.error !== "native_host_not_connected") return response;
    await new Promise((resolve) => { setTimeout(resolve, 200); });
  }
  return { ok: false, error: "native_host_not_connected" };
}

function send(message: Record<string, unknown>): Promise<PopupState | undefined> {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(message, (response) => {
      resolve(chrome.runtime.lastError === undefined ? response as PopupState : undefined);
    });
  });
}

function required<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) throw new Error(`missing required element ${id}`);
  return element as T;
}

export {};
