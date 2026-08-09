type PolicyLevel = "critical" | "balanced" | "convenient" | "monitor";

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
  host?: string;
  suggestedScope?: string;
  groups?: GroupSummary[];
  error?: string;
  alert?: StoredAlert;
  pending?: boolean;
  requestId?: string;
}

const SIGNAL_LABELS: Record<string, string> = {
  remote_debugging_port: "Bir tarayıcı uzaktan hata ayıklama açık başlatıldı",
  remote_debugging_pipe: "Bir tarayıcı uzaktan hata ayıklama açık başlatıldı",
  host_disconnect_active_lease: "Koruma açıkken native host bağlantısı koptu",
  host_disconnect: "Native host bağlantısı koptu",
  reconnect_success: "Native host bağlantısı geri geldi",
  reconciliation_failed: "Uzlaştırma başarısız oldu",
  audit_integrity_recovered: "Audit bütünlüğü bozuldu; hasarlı zincir karantinaya alındı",
  lease_outside_cookie_created: "Kilitli sitede çerez oluştu",
  selector_changed: "Korunan çerezlerde değişiklik",
  monitor_queue_overflow: "İzleme kuyruğu doldu, olaylar atlandı",
  process_inspection_unavailable: "Process denetimi yapılamadı",
};

const POLICY_LABELS: Record<string, string> = {
  critical: "Kritik",
  balanced: "Dengeli",
  convenient: "Kullanışlı",
  monitor: "İzleme",
};

const STATE_LABELS: Record<string, string> = {
  uninitialized: "kasa boş",
  sealed: "kilitli",
  unlocking: "açılıyor",
  leased: "açık",
  evicting: "tahliye ediliyor",
  degraded: "sorunlu",
};

const connectionWarning = required<HTMLElement>("connection");
const errorText = required<HTMLElement>("error");
const currentSection = required<HTMLElement>("current");
const currentHost = required<HTMLElement>("current-host");
const unprotectedBox = required<HTMLElement>("current-unprotected");
const protectedBox = required<HTMLElement>("current-protected");
const currentPolicySelect = required<HTMLSelectElement>("current-policy");
const unprotectCurrentButton = required<HTMLButtonElement>("unprotect-current");
const scopeInput = required<HTMLInputElement>("scope");
const policySelect = required<HTMLSelectElement>("policy");
const protectButton = required<HTMLButtonElement>("protect");
const groupList = required<HTMLUListElement>("groups");
const groupsEmpty = required<HTMLElement>("groups-empty");
const alertBox = required<HTMLElement>("alert");
const openOptionsButton = required<HTMLButtonElement>("open-options");

protectButton.addEventListener("click", () => { void protectCurrentSite(); });
openOptionsButton.addEventListener("click", () => { chrome.runtime.openOptionsPage(); });
unprotectCurrentButton.addEventListener("click", () => {
  if (currentGroup !== undefined) void unprotect(currentGroup);
});
currentPolicySelect.addEventListener("change", () => { void changeCurrentPolicy(); });

// The group covering the active tab, so the top section can act on it directly.
let currentGroup: GroupSummary | undefined;

async function changeCurrentPolicy(): Promise<void> {
  if (currentGroup === undefined) return;
  const groupId = currentGroup.id;
  const policyLevel = currentPolicySelect.value as PolicyLevel;
  if (policyLevel === currentGroup.policyLevel) return;
  currentPolicySelect.disabled = true;
  try {
    const response = await sendAwaitingHost({ type: "popup.setPolicy", groupId, policyLevel });
    if (response?.ok !== true) return showError(describeError(response?.error));
    await refreshUntil(() => policyOf(groupId) === policyLevel);
  } finally { currentPolicySelect.disabled = false; }
}

void refresh();

async function protectCurrentSite(): Promise<void> {
  const scope = scopeInput.value.trim().replace(/^\./, "").toLowerCase();
  if (scope === "") return showError("Alan adı boş olamaz.");
  if (scopeOverlapsExisting(scope)) return showError(describeError("scope_overlaps_existing"));
  protectButton.disabled = true;
  try {
    // The permission prompt must run inside this click's user-gesture context, so it happens
    // here rather than in the service worker. Without it the extension cannot read or restore
    // the scope's cookies at all, so a refusal must stop the whole operation.
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
    const before = groupCount();
    await refreshUntil(() => groupCount() !== before);
  } catch { showError("Koruma eklenemedi."); }
  finally { protectButton.disabled = false; }
}

async function unprotect(group: GroupSummary): Promise<void> {
  const warning = group.state === "sealed"
    ? `“${group.scope}” şu anda kilitli. Kaldırırsanız kasadaki tek oturum kopyası kalıcı olarak silinecek ve site oturumu geri getirilemeyecek. Devam edilsin mi?`
    : `“${group.scope}” korumadan çıkarılacak ve kasası kalıcı olarak silinecek. Devam edilsin mi?`;
  if (!window.confirm(warning)) return;
  const response = await sendAwaitingHost({ type: "popup.unprotect", groupId: group.id });
  if (response?.ok !== true) return showError(describeError(response?.error));
  const before = groupCount();
  await refreshUntil(() => groupCount() !== before);
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

// Mirrors the host/protocol overlap check so a duplicate/nested scope is rejected instantly, from
// the data already on screen, instead of walking through a permission prompt and a host round-trip
// only to be told the same thing seconds later.
function scopeOverlapsExisting(scope: string): boolean {
  return latestGroups.some((group) => {
    const existing = group.scope.toLowerCase();
    return scope === existing || scope.endsWith(`.${existing}`) || existing.endsWith(`.${scope}`);
  });
}

function policyOf(groupId: string): string | undefined {
  return latestGroups.find((group) => group.id === groupId)?.policyLevel;
}

async function refresh(): Promise<void> {
  const tab = await activeTab();
  const state = await send({ type: "popup.state", url: tab?.url }) ?? {};
  connectionWarning.hidden = state.connected === true;
  if (state.error !== undefined) showError(describeError(state.error)); else errorText.hidden = true;

  const groups = state.groups ?? [];
  latestGroups = groups;
  renderAlert(state.alert, groups);
  const host = state.host ?? "";
  currentGroup = groups.find((group) => hostInScope(group.scope, host));
  currentSection.hidden = host === "";
  currentHost.textContent = host;
  protectedBox.hidden = currentGroup === undefined;
  unprotectedBox.hidden = currentGroup !== undefined;
  if (currentGroup === undefined) scopeInput.value = state.suggestedScope ?? host;
  // Only overwrite the selector when it is not mid-edit, so a change in flight is not clobbered.
  else if (!currentPolicySelect.disabled) currentPolicySelect.value = currentGroup.policyLevel;

  groupList.replaceChildren(...groups.map(renderGroup));
  groupsEmpty.hidden = groups.length > 0;
}

function renderAlert(alert: StoredAlert | undefined, groups: GroupSummary[]): void {
  if (alert === undefined) return;
  const group = groups.find((item) => item.id === alert.accountGroupId);
  const scopes = group?.scope ?? (alert.affectedScopes !== undefined && alert.affectedScopes.length > 0 ? alert.affectedScopes.join(", ") : undefined);
  const where = scopes === undefined ? "" : ` — ${scopes}`;
  const when = new Date(alert.observedAtUnixMs).toLocaleTimeString();
  const times = alert.occurrences > 1 ? ` · ${alert.occurrences} kez` : "";
  alertBox.textContent = `${SIGNAL_LABELS[alert.signal] ?? alert.signal}${where} (${when}${times})`;
  alertBox.hidden = false;
}

function renderGroup(group: GroupSummary): HTMLLIElement {
  const item = document.createElement("li");
  const scope = document.createElement("span");
  scope.className = "scope";
  scope.textContent = group.scope;
  const state = document.createElement("span");
  state.className = "state";
  // A site the host protects but the extension cannot reach is not protected in practice, so it
  // is reported as such instead of silently failing on every cookie operation.
  state.textContent = group.hasPermission
    ? `${POLICY_LABELS[group.policyLevel] ?? group.policyLevel} · ${STATE_LABELS[group.state] ?? group.state} · ${group.cookieCount} çerez`
    : "izin yok — koruma çalışmıyor";
  if (!group.hasPermission) state.classList.add("blocked");
  item.append(scope, state);
  if (!group.hasPermission) {
    const grant = document.createElement("button");
    grant.type = "button";
    grant.textContent = "İzin ver";
    grant.addEventListener("click", () => { void grantPermission(group); });
    item.append(grant);
  }
  const remove = document.createElement("button");
  remove.type = "button";
  remove.textContent = "Kaldır";
  remove.addEventListener("click", () => { void unprotect(group); });
  item.append(remove);
  return item;
}

// Optional host permissions are tied to the extension install while the protected-site list lives
// with the host, so a reinstall can leave the two out of step. Re-granting repairs it in place
// rather than forcing the user to remove and re-add the site.
async function grantPermission(group: GroupSummary): Promise<void> {
  const granted = await requestScopePermission(group.scope);
  if (!granted) return showError("Chrome izni verilmedi.");
  await refresh();
}

function hostInScope(scope: string, host: string): boolean {
  const normalized = host.toLowerCase();
  return normalized === scope || normalized.endsWith(`.${scope}`);
}

function describeError(code: string | undefined): string {
  switch (code) {
    case "scope_overlaps_existing": return "Bu alan adı zaten korunan bir siteyle çakışıyor.";
    case "scope_invalid": return "Alan adı geçersiz.";
    case "group_limit_reached": return "Korunan site sınırına ulaşıldı.";
    case "operation_pending": return "Bu site üzerinde bir işlem sürüyor; birazdan tekrar deneyin.";
    case "monitor_transition_requires_unlocked_session": return "Yalnız izleme moduna geçmeden önce kilitli oturumu bir kez açın; aksi halde kasadaki tek oturum kopyası kaybolur.";
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

function activeTab(): Promise<chrome.tabs.Tab | undefined> {
  return new Promise((resolve) => {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => resolve(tabs[0]));
  });
}

// Granting a host permission restarts the service worker, so the native connection is briefly
// down right after the Chrome prompt is accepted. Retrying here rather than waiting inside the
// background message handler avoids a deadlock: the handshake reply is processed on the same
// serialized queue the handler itself runs on.
async function sendAwaitingHost(message: Record<string, unknown>): Promise<PopupState | undefined> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const response = await send(message);
    if (response?.error !== "native_host_not_connected") return awaitConfigAck(response);
    await new Promise((resolve) => { setTimeout(resolve, 200); });
  }
  return { ok: false, error: "native_host_not_connected" };
}

async function awaitConfigAck(response: PopupState | undefined): Promise<PopupState | undefined> {
  if (response?.pending !== true || typeof response.requestId !== "string") return response;
  for (let attempt = 0; attempt < 50; attempt += 1) {
    await new Promise((resolve) => { setTimeout(resolve, 100); });
    const status = await send({ type: "popup.operation", requestId: response.requestId });
    if (status?.pending !== true) return status;
  }
  return { ok: false, error: "operation_timeout" };
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
