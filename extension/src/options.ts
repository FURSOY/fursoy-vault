import { enhanceSelect, enhanceSelects } from "./custom-select.js";
import { translate, type Locale } from "./i18n.js";
import { currentLocale } from "./locale.js";
import { isValidProtectionScope, normalizeProtectionScopeInput } from "./protocol.js";
import { permissionOrigins, shouldOfferGrantAll } from "./site-permissions.js";

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
  groups?: GroupSummary[];
  error?: string;
  log?: StoredAlert[];
  pending?: boolean;
  requestId?: string;
  reconnecting?: boolean;
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
  permission_missing: "common.signal.permissionMissing",
};

function signalLabel(signal: string): string {
  if (signal === "remote_debugging_port" || signal === "remote_debugging_pipe") return t("options.signal.remoteDebugging");
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

const POLICY_OPTION_KEYS: Record<string, string> = {
  critical: "options.addCard.policyOption.critical",
  balanced: "options.addCard.policyOption.balanced",
  convenient: "options.addCard.policyOption.convenient",
  monitor: "options.addCard.policyOption.monitor",
};

function policyOptionLabel(level: string): string {
  const key = POLICY_OPTION_KEYS[level];
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

const SEVERITY_KEYS: Record<string, string> = { high: "common.severity.high", medium: "common.severity.medium", info: "common.severity.info" };
function severityLabel(severity: string): string {
  const key = SEVERITY_KEYS[severity];
  return key !== undefined ? t(key) : severity;
}

const connectionWarning = required<HTMLElement>("connection");
const errorText = required<HTMLElement>("error");
const groupRows = required<HTMLTableSectionElement>("groups");
const groupsTable = required<HTMLTableElement>("groups-table");
const groupsEmpty = required<HTMLElement>("groups-empty");
const addForm = required<HTMLFormElement>("add-form");
const scopeInput = required<HTMLInputElement>("scope");
const addError = required<HTMLElement>("add-error");
const policySelect = required<HTMLSelectElement>("policy");
const addButton = required<HTMLButtonElement>("add");
const logRows = required<HTMLTableSectionElement>("log");
const logTable = required<HTMLTableElement>("log-table");
const logEmpty = required<HTMLElement>("log-empty");
const clearLogButton = required<HTMLButtonElement>("clear-log");
const siteCount = required<HTMLElement>("site-count");
const protectedCount = required<HTMLElement>("protected-count");
const cookieCount = required<HTMLElement>("cookie-count");
const eventCount = required<HTMLElement>("event-count");
const companionState = required<HTMLElement>("companion-state");
const extensionVersion = required<HTMLElement>("extension-version");
const navElement = required<HTMLElement>("nav");
const metricsSection = required<HTMLElement>("metrics");
const grantAllPermissions = required<HTMLElement>("grant-all-permissions");
const grantAllPermissionsCopy = required<HTMLElement>("grant-all-permissions-copy");
const grantAllPermissionsButton = required<HTMLButtonElement>("grant-all-permissions-button");

extensionVersion.textContent = `v${chrome.runtime.getManifest().version}`;
enhanceSelects();

const navigationItems = Array.from(document.querySelectorAll<HTMLAnchorElement>(".nav-item"));
const navigationSections = navigationItems
  .map((item) => document.querySelector<HTMLElement>(item.hash))
  .filter((section): section is HTMLElement => section !== null);
window.addEventListener("scroll", updateActiveNavigation, { passive: true });
updateActiveNavigation();

addForm.addEventListener("submit", (event) => { event.preventDefault(); void addSite(); });
scopeInput.addEventListener("input", clearAddError);
scopeInput.addEventListener("blur", () => { normalizeScopeField(); });
clearLogButton.addEventListener("click", () => { void clearLog(); });
grantAllPermissionsButton.addEventListener("click", () => { void grantAllMissingPermissions(); });

function applyTranslations(): void {
  document.documentElement.lang = locale;
  document.title = t("options.pageTitle");
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((element) => {
    const key = element.dataset.i18n;
    if (key !== undefined) element.textContent = t(key);
  });
  navElement.setAttribute("aria-label", t("options.nav.ariaLabel"));
  metricsSection.setAttribute("aria-label", t("options.metrics.ariaLabel"));
  scopeInput.placeholder = t("onboarding.addsite.scopePlaceholder");
  enhanceSelects();
}

void (async () => {
  locale = await currentLocale();
  applyTranslations();
  await refreshInitialState();
})();

async function addSite(): Promise<void> {
  const scope = normalizeScopeField();
  if (scope === "") return showAddError(t("onboarding.addsite.error.empty"));
  if (!isValidProtectionScope(scope)) return showAddError(t("onboarding.addsite.error.invalid"));
  if (scopeOverlapsExisting(scope)) return showAddError(describeError("scope_overlaps_existing"));
  clearAddError();
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
      return showError(t("onboarding.addsite.error.permission"));
    }
    const response = await sendAwaitingHost({ type: "popup.protect" });
    if (response?.ok !== true) return showError(describeError(response?.error));
    scopeInput.value = "";
    const before = groupCount();
    await refreshUntil(() => groupCount() !== before);
  } catch { showError(t("onboarding.addsite.error.generic")); }
  finally { addButton.disabled = false; }
}

async function unprotect(group: GroupSummary, trigger: HTMLButtonElement): Promise<void> {
  const warning = group.state === "sealed"
    ? t("options.unprotectWarningSealed", { scope: group.scope })
    : t("common.unprotectWarning", { scope: group.scope });
  if (trigger.dataset.confirmRemove !== group.id) {
    trigger.dataset.confirmRemove = group.id;
    trigger.textContent = t("common.confirmPermanentRemoval");
    trigger.title = warning;
    setTimeout(() => {
      if (trigger.dataset.confirmRemove !== group.id) return;
      delete trigger.dataset.confirmRemove;
      trigger.textContent = t("common.remove");
      trigger.title = "";
    }, 8_000);
    return;
  }
  trigger.disabled = true;
  try {
    const before = groupCount();
    const response = await sendAwaitingHost({ type: "popup.unprotect", groupId: group.id });
    if (response?.ok !== true) return showError(describeError(response?.error));
    await refreshUntil(() => groupCount() !== before);
  } finally { trigger.disabled = false; }
}

async function grantPermission(group: GroupSummary): Promise<void> {
  const granted = await requestScopePermission(group.scope);
  if (!granted) return showError(t("common.error.permissionDenied"));
  await refresh();
}

async function clearLog(): Promise<void> {
  await send({ type: "popup.clearLog" });
  await refresh();
}

// Every config mutation is answered asynchronously by the host, so the UI polls until the
// change it asked for is actually visible rather than reading state once and trusting it.
let latestGroups: GroupSummary[] = [];

// Mirrors the host/protocol overlap check so a duplicate/nested scope is rejected instantly, from
// the data already on screen, instead of walking through a permission prompt and a host round-trip
// only to be told the same thing seconds later.
function scopeOverlapsExisting(scope: string): boolean {
  return latestGroups.some((group) => {
    const existing = group.scope.toLowerCase();
    return scope === existing || scope.endsWith(`.${existing}`) || existing.endsWith(`.${scope}`);
  });
}

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

function policyOf(groupId: string): string | undefined {
  return latestGroups.find((group) => group.id === groupId)?.policyLevel;
}

async function refresh(): Promise<void> {
  const state = await send({ type: "popup.state" }) ?? {};
  connectionWarning.hidden = state.connected === true;
  companionState.classList.toggle("offline", state.connected !== true);
  const companionLabel = companionState.querySelector<HTMLElement>("b");
  if (companionLabel !== null) companionLabel.textContent = state.connected === true ? t("options.connectedLabel") : t("options.disconnectedLabel");
  if (state.error !== undefined) showError(describeError(state.error)); else errorText.hidden = true;

  const groups = state.groups ?? [];
  latestGroups = groups;
  renderBulkPermission(groups);
  siteCount.textContent = String(groups.length);
  protectedCount.textContent = String(groups.filter((group) => group.hasPermission).length);
  cookieCount.textContent = String(groups.reduce((total, group) => total + (group.hasPermission ? group.cookieCount : 0), 0));
  groupRows.replaceChildren(...groups.map(renderGroupRow));
  groupsEmpty.hidden = groups.length > 0;
  groupsTable.hidden = groups.length === 0;

  const logResponse = await send({ type: "popup.log" }) ?? {};
  const log = logResponse.log ?? [];
  eventCount.textContent = String(log.length);
  logRows.replaceChildren(...log.map((entry) => renderLogRow(entry, groups)));
  logEmpty.hidden = log.length > 0;
  logTable.hidden = log.length === 0;
}

function renderBulkPermission(groups: GroupSummary[]): void {
  const missing = groups.filter((group) => !group.hasPermission);
  grantAllPermissions.hidden = !shouldOfferGrantAll(missing.length);
  if (grantAllPermissions.hidden) return;
  grantAllPermissionsCopy.textContent = t("common.multiplePermissionsMissing", { count: missing.length });
  grantAllPermissionsButton.textContent = t("common.grantAllPermissions");
}

async function grantAllMissingPermissions(): Promise<void> {
  const missing = latestGroups.filter((group) => !group.hasPermission);
  if (!shouldOfferGrantAll(missing.length)) return;
  grantAllPermissionsButton.disabled = true;
  try {
    const granted = await requestPermissions(missing.map((group) => group.scope));
    if (!granted) return showError(t("common.error.permissionDenied"));
    await refresh();
  } finally { grantAllPermissionsButton.disabled = false; }
}

function updateActiveNavigation(): void {
  const marker = window.scrollY + 150;
  let active = navigationSections[0];
  for (const section of navigationSections) {
    if (section.offsetTop <= marker) active = section;
  }
  for (const item of navigationItems) {
    item.classList.toggle("active", item.hash === `#${active?.id ?? "overview"}`);
  }
}

function renderGroupRow(group: GroupSummary): HTMLTableRowElement {
  const row = document.createElement("tr");
  row.className = `group-row state-${group.state}`;
  row.append(cell(group.scope, "scope"), policyCell(group));
  // A site the host protects but the extension cannot reach is not protected in practice.
  const state = document.createElement("td");
  const statePill = document.createElement("span");
  statePill.className = `state-pill state-pill-${group.hasPermission ? group.state : "blocked"}`;
  statePill.textContent = group.hasPermission ? stateLabel(group.state) : t("options.sites.permissionRequired");
  if (!group.hasPermission) statePill.classList.add("blocked");
  state.append(statePill);
  row.append(state, cell(group.hasPermission ? String(group.cookieCount) : "—", "numeric"));

  const actions = document.createElement("td");
  actions.className = "actions";
  if (!group.hasPermission) actions.append(button(t("common.grantPermission"), () => { void grantPermission(group); }));
  const remove = button(t("common.remove"), () => { void unprotect(group, remove); });
  actions.append(remove);
  row.append(actions);
  return row;
}

function policyCell(group: GroupSummary): HTMLTableCellElement {
  const container = document.createElement("td");
  const select = document.createElement("select");
  for (const level of ["critical", "balanced", "convenient", "monitor"] as const) {
    const option = document.createElement("option");
    option.value = level;
    option.textContent = policyOptionLabel(level);
    option.selected = level === group.policyLevel;
    select.append(option);
  }
  select.addEventListener("change", () => { void changePolicy(group, select); });
  container.append(select);
  enhanceSelect(select);
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
  const severity = document.createElement("td");
  const severityPill = document.createElement("span");
  severityPill.className = `severity-pill severity-${entry.severity}`;
  severityPill.textContent = severityLabel(entry.severity);
  severity.append(severityPill);
  const site = group?.scope ?? (entry.affectedScopes !== undefined && entry.affectedScopes.length > 0
    ? entry.affectedScopes.join(", ")
    : "—");
  row.append(
    cell(new Date(entry.observedAtUnixMs).toLocaleString(), "numeric"),
    severity,
    cell(signalLabel(entry.signal)),
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

function describeError(code: string | undefined): string {
  switch (code) {
    case "scope_overlaps_existing": return t("onboarding.addsite.error.overlap");
    case "scope_invalid": return t("common.error.scopeInvalid");
    case "group_limit_reached": return t("common.error.groupLimit");
    case "operation_pending": return t("common.error.operationPending");
    case "monitor_transition_requires_unlocked_session": return t("common.error.monitorRequiresUnlocked");
    case "native_host_not_connected": return t("common.error.hostNotConnected");
    case "upgrade_required": return t("common.error.upgradeRequired");
    case "unknown_group": return t("common.error.unknownGroup");
    default: return t("common.error.generic");
  }
}

function showError(message: string): void {
  errorText.textContent = message;
  errorText.hidden = false;
}

function normalizeScopeField(): string {
  const scope = normalizeProtectionScopeInput(scopeInput.value);
  if (scope !== "") scopeInput.value = scope;
  return scope;
}

function clearAddError(): void {
  scopeInput.removeAttribute("aria-invalid");
  addError.hidden = true;
}

function showAddError(message: string): void {
  scopeInput.setAttribute("aria-invalid", "true");
  addError.textContent = message;
  addError.hidden = false;
  scopeInput.focus();
}

function requestScopePermission(scope: string): Promise<boolean> {
  return requestPermissions([scope]);
}

function requestPermissions(scopes: readonly string[]): Promise<boolean> {
  return new Promise((resolve) => {
    chrome.permissions.request({ origins: permissionOrigins(scopes) }, (granted) => {
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
