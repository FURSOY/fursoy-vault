import { enhanceSelects, syncCustomSelect } from "./custom-select.js";
import { translate, type Locale } from "./i18n.js";
import { currentLocale } from "./locale.js";
import { isValidProtectionScope, normalizeProtectionScopeInput } from "./protocol.js";

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

let locale: Locale = "tr";

function t(key: string, params?: Readonly<Record<string, string | number>>): string {
  return translate(locale, key, params);
}

const COMMON_SIGNAL_KEYS: Record<string, string> = {
  host_disconnect_active_lease: "common.signal.hostDisconnectActiveLease",
  host_disconnect: "common.signal.hostDisconnect",
  reconnect_success: "common.signal.reconnectSuccess",
  reconciliation_failed: "common.signal.reconciliationFailed",
  audit_integrity_recovered: "common.signal.auditIntegrityRecovered",
  lease_outside_cookie_created: "common.signal.leaseOutsideCookieCreated",
  selector_changed: "common.signal.selectorChanged",
  monitor_queue_overflow: "common.signal.monitorQueueOverflow",
  process_inspection_unavailable: "common.signal.processInspectionUnavailable",
};

function signalLabel(signal: string): string {
  if (signal === "remote_debugging_port" || signal === "remote_debugging_pipe") return t("popup.signal.remoteDebugging");
  const key = COMMON_SIGNAL_KEYS[signal];
  return key !== undefined ? t(key) : signal;
}

const POLICY_SHORT_KEYS: Record<string, string> = {
  critical: "common.policyShort.critical", balanced: "common.policyShort.balanced",
  convenient: "common.policyShort.convenient", monitor: "common.policyShort.monitor",
};
function policyShortLabel(level: string): string {
  const key = POLICY_SHORT_KEYS[level];
  return key !== undefined ? t(key) : level;
}

const STATE_KEYS: Record<string, string> = {
  uninitialized: "common.state.uninitialized", sealed: "common.state.sealed", unlocking: "common.state.unlocking",
  leased: "common.state.leased", evicting: "common.state.evicting", degraded: "common.state.degraded",
};
function stateLabel(state: string): string {
  const key = STATE_KEYS[state];
  return key !== undefined ? t(key) : state;
}

const connectionWarning = required<HTMLElement>("connection");
const errorText = required<HTMLElement>("error");
const currentSection = required<HTMLElement>("current");
const currentHost = required<HTMLElement>("current-host");
const unprotectedBox = required<HTMLElement>("current-unprotected");
const protectedBox = required<HTMLElement>("current-protected");
const protectionStateCandidate = protectedBox.querySelector<HTMLElement>(".protection-state");
if (protectionStateCandidate === null) throw new Error("missing protection state");
const protectionState: HTMLElement = protectionStateCandidate;
const currentStateTitle = required<HTMLElement>("current-state-title");
const currentStateBody = required<HTMLElement>("current-state-body");
const currentPolicySelect = required<HTMLSelectElement>("current-policy");
const unprotectCurrentButton = required<HTMLButtonElement>("unprotect-current");
const scopeInput = required<HTMLInputElement>("scope");
const policySelect = required<HTMLSelectElement>("policy");
const protectButton = required<HTMLButtonElement>("protect");
const groupList = required<HTMLUListElement>("groups");
const groupsEmpty = required<HTMLElement>("groups-empty");
const groupsCount = required<HTMLElement>("groups-count");
const alertBox = required<HTMLElement>("alert");
const openOptionsButton = required<HTMLButtonElement>("open-options");
const liveIndicator = document.querySelector<HTMLElement>(".live-indicator");

enhanceSelects();

protectButton.addEventListener("click", () => { void protectCurrentSite(); });
openOptionsButton.addEventListener("click", () => { chrome.runtime.openOptionsPage(); });
unprotectCurrentButton.addEventListener("click", () => {
  if (currentGroup !== undefined) void unprotect(currentGroup);
});
currentPolicySelect.addEventListener("change", () => { void changeCurrentPolicy(); });

// The group covering the active tab, so the top section can act on it directly.
let currentGroup: GroupSummary | undefined;

function applyTranslations(): void {
  document.documentElement.lang = locale;
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((element) => {
    const key = element.dataset.i18n;
    if (key !== undefined) element.textContent = t(key);
  });
  openOptionsButton.setAttribute("aria-label", t("popup.openOptionsLabel"));
  openOptionsButton.title = t("popup.openOptionsTitle");
  liveIndicator?.setAttribute("aria-label", t("popup.liveIndicator"));
  syncCustomSelect(currentPolicySelect);
  syncCustomSelect(policySelect);
}

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

void (async () => {
  locale = await currentLocale();
  applyTranslations();
  await refreshInitialState();
})();

async function protectCurrentSite(): Promise<void> {
  const scope = normalizeProtectionScopeInput(scopeInput.value);
  if (scope === "") return showError(t("onboarding.addsite.error.empty"));
  if (!isValidProtectionScope(scope)) return showError(t("onboarding.addsite.error.invalid"));
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
      return showError(t("onboarding.addsite.error.permission"));
    }
    const response = await sendAwaitingHost({ type: "popup.protect" });
    if (response?.ok !== true) return showError(describeError(response?.error));
    const before = groupCount();
    await refreshUntil(() => groupCount() !== before);
  } catch { showError(t("onboarding.addsite.error.generic")); }
  finally { protectButton.disabled = false; }
}

async function unprotect(group: GroupSummary): Promise<void> {
  const warning = group.state === "sealed"
    ? t("popup.unprotectWarningSealed", { scope: group.scope })
    : t("common.unprotectWarning", { scope: group.scope });
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

// A cold service worker needs a short storage + native-handshake window. Do not flash an offline
// warning for those normal startup snapshots; reveal it only if the retry window is exhausted.
async function refreshInitialState(): Promise<void> {
  for (let attempt = 0; attempt < 15; attempt += 1) {
    await refresh();
    if (connectionWarning.hidden) return;
    connectionWarning.hidden = true;
    await new Promise((resolve) => { setTimeout(resolve, 200); });
  }
  await refresh();
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
  else if (!currentPolicySelect.disabled) {
    currentPolicySelect.value = currentGroup.policyLevel;
    syncCustomSelect(currentPolicySelect);
  }
  if (currentGroup !== undefined) renderCurrentProtectionState(currentGroup);

  groupList.replaceChildren(...groups.map(renderGroup));
  groupsEmpty.hidden = groups.length > 0;
  groupsCount.textContent = String(groups.length);
}

function renderCurrentProtectionState(group: GroupSummary): void {
  protectionState.classList.toggle("protection-problem", !group.hasPermission || group.state === "degraded");
  if (!group.hasPermission) {
    currentStateTitle.textContent = t("popup.permissionRequiredTitle");
    currentStateBody.textContent = t("popup.permissionRequiredBody");
    return;
  }
  const copy: Record<string, [string, string]> = {
    uninitialized: [t("popup.state.uninitializedTitle"), t("popup.state.uninitializedBody")],
    sealed: [t("popup.state.sealedTitle"), t("popup.state.sealedBody")],
    unlocking: [t("popup.state.unlockingTitle"), t("popup.state.unlockingBody")],
    leased: [t("popup.state.leasedTitle"), t("popup.state.leasedBody")],
    evicting: [t("popup.state.evictingTitle"), t("popup.state.evictingBody")],
    degraded: [t("popup.state.degradedTitle"), t("popup.state.degradedBody")],
  };
  const [title, body] = copy[group.state] ?? [t("popup.protectionActiveTitle"), t("popup.protectionActiveBody")];
  currentStateTitle.textContent = title;
  currentStateBody.textContent = body;
}

function renderAlert(alert: StoredAlert | undefined, groups: GroupSummary[]): void {
  if (alert === undefined) {
    alertBox.hidden = true;
    return;
  }
  const group = groups.find((item) => item.id === alert.accountGroupId);
  const scopes = group?.scope ?? (alert.affectedScopes !== undefined && alert.affectedScopes.length > 0 ? alert.affectedScopes.join(", ") : undefined);
  const where = scopes === undefined ? "" : ` — ${scopes}`;
  const when = new Date(alert.observedAtUnixMs).toLocaleTimeString();
  const times = alert.occurrences > 1 ? t("popup.alertOccurrences", { count: alert.occurrences }) : "";
  alertBox.textContent = `${signalLabel(alert.signal)}${where} (${when}${times})`;
  alertBox.hidden = false;
}

function renderGroup(group: GroupSummary): HTMLLIElement {
  const item = document.createElement("li");
  item.className = `site-item state-${group.state}`;
  const scope = document.createElement("span");
  scope.className = "scope";
  scope.textContent = group.scope;
  const state = document.createElement("span");
  state.className = "state";
  // A site the host protects but the extension cannot reach is not protected in practice, so it
  // is reported as such instead of silently failing on every cookie operation.
  state.textContent = group.hasPermission
    ? t("popup.groupSummary", { policy: policyShortLabel(group.policyLevel), state: stateLabel(group.state), count: group.cookieCount })
    : t("popup.noPermission");
  if (!group.hasPermission) state.classList.add("blocked");
  const actions = document.createElement("span");
  actions.className = "site-actions";
  item.append(scope, state);
  if (!group.hasPermission) {
    const grant = document.createElement("button");
    grant.type = "button";
    grant.textContent = t("common.grantPermission");
    grant.addEventListener("click", () => { void grantPermission(group); });
    actions.append(grant);
  }
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "remove-button";
  remove.textContent = t("common.remove");
  remove.addEventListener("click", () => { void unprotect(group); });
  actions.append(remove);
  item.append(actions);
  return item;
}

// Optional host permissions are tied to the extension install while the protected-site list lives
// with the host, so a reinstall can leave the two out of step. Re-granting repairs it in place
// rather than forcing the user to remove and re-add the site.
async function grantPermission(group: GroupSummary): Promise<void> {
  const granted = await requestScopePermission(group.scope);
  if (!granted) return showError(t("common.error.permissionDenied"));
  await refresh();
}

function hostInScope(scope: string, host: string): boolean {
  const normalized = host.toLowerCase();
  return normalized === scope || normalized.endsWith(`.${scope}`);
}

function describeError(code: string | undefined): string {
  switch (code) {
    case "scope_overlaps_existing": return t("onboarding.addsite.error.overlap");
    case "scope_invalid": return t("common.error.scopeInvalid");
    case "group_limit_reached": return t("common.error.groupLimit");
    case "operation_pending": return t("common.error.operationPending");
    case "monitor_transition_requires_unlocked_session": return t("common.error.monitorRequiresUnlocked");
    case "native_host_not_connected": return t("common.error.hostNotConnected");
    case "unknown_group": return t("common.error.unknownGroup");
    default: return t("common.error.generic");
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
