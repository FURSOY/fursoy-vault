import {
  HOST_NAME, PROTOCOL_VERSION, categorizeCookieSetFailure, cookieBelongsToGroup,
  cookieIdentity, cookieRecord, cookieSetDetails, cookieUrl, groupForCookie, groupForUrl,
  guessScope, navigationPatterns, policyParameters, validateConfig,
  type AccountGroup, type AccountGroupsConfig, type CookieRecord, type CookieSetFailureCategory,
  type Envelope, type LoadedConfig, type PolicyLevel, type WireMessage,
} from "./protocol.js";
import {
  addToBoundedOutbox, makeMonitorEvent, notificationDecision, notificationText,
  validateMonitorEvent, type MonitorEvent, type MonitorSignal, type NotificationDecisionState,
} from "./monitor.js";

type GroupState = "uninitialized" | "sealed" | "unlocking" | "leased" | "evicting" | "degraded";
type LeasePurpose = "inject" | "enroll";
type SessionInvalidationReason = "external_logout" | "restore_rejected" | "scope_empty";

// ADR-020: eviction triggers that mean "the user stopped using this site". If the protected
// scope holds no cookies at that moment there is nothing to vault, so the group is discarded
// rather than sealed — see requestEviction.
const USER_IDLE_TRIGGERS = new Set(["last_tab_closed", "idle", "locked", "expiry"]);

// A protected group whose vault is empty still has to be captured the next time the user stops
// using the site; that capture is what replaces the removed "did a login just happen" heuristic.
function awaitsCapture(state: GroupRuntimeState | undefined): boolean {
  return state?.groupState === "leased" || state?.groupState === "uninitialized";
}

interface GroupRuntimeState {
  groupState: GroupState;
  leaseId?: string;
  reconciliation: boolean;
  relevantTabs: number[];
  lastEvent: string;
  pendingLeaseRequest?: LeasePurpose;
  evictionRequestPending?: boolean;
  injectAfterReconciliation?: boolean;
  invalidationPending?: boolean;
  invalidationReason?: SessionInvalidationReason;
  evictAfterEnrollment?: string;
  pendingNavigationUnlocks?: Record<string, string>;
  navigationUnlockRequestTabId?: number;
  navigationUnlockError?: string;
}

interface RuntimeState {
  version: 2;
  configDigest: string;
  groups: Record<string, GroupRuntimeState>;
}

interface HandshakeGroupState {
  account_group_id: string;
  group_state: GroupState;
  reconciliation_required: boolean;
  lease_id?: string | null;
  lease_expiry_unix_ms?: number | null;
}

const STATE_KEY = "fcp-runtime-v2";
const LEGACY_STATE_KEY = "fcp-mvp-runtime-v1";
const WIKIPEDIA_GROUP_ID = "7a144677-3f5c-4a86-a767-16fd3ca315b8";
const IDLE_BASE_SECONDS = 60;
const NAVIGATION_LOAD_TIMEOUT_MS = 20_000;
const UNLOCK_PAGE_URL = chrome.runtime.getURL("unlock.html");
// chrome.notifications.create does not support SVG for iconUrl; it silently fails to show
// a notification if given one. Use a PNG raster icon instead.
const MONITOR_ICON_URL = chrome.runtime.getURL("monitor-icon.png");
const MONITOR_OUTBOX_KEY = "fcp-monitor-outbox-v1";
const MONITOR_RATE_KEY = "fcp-monitor-rate-v1";
const MONITOR_POLL_ALARM = "fcp-monitor-poll";
const CONFIG_CACHE_KEY = "fcp-config-cache-v1";
const LAST_ALERT_KEY = "fcp-last-alert-v1";
const ALERT_LOG_KEY = "fcp-alert-log-v1";
const ALERT_LOG_LIMIT = 100;
const PENDING_ADD_KEY = "fcp-pending-add-v1";
const PENDING_ADD_TTL_MS = 120_000;

// Q24: the host is the config's single source of truth. The cache exists only so the extension
// can still evict fail-closed while the host is unreachable; the handshake always overwrites it.
let loadedConfig: LoadedConfig | undefined;
let configWaiters: Array<(value: LoadedConfig) => void> = [];

function awaitConfig(): Promise<LoadedConfig> {
  if (loadedConfig !== undefined) return Promise.resolve(loadedConfig);
  return new Promise((resolve) => { configWaiters.push(resolve); });
}

function publishConfig(next: LoadedConfig): void {
  loadedConfig = next;
  const waiters = configWaiters;
  configWaiters = [];
  for (const waiter of waiters) waiter(next);
}

async function adoptConfig(config: AccountGroupsConfig, digest: string): Promise<void> {
  validateConfig(config);
  publishConfig({ config, digest });
  await setLocal(CONFIG_CACHE_KEY, { config, digest });
}

async function restoreCachedConfig(): Promise<void> {
  if (loadedConfig !== undefined) return;
  const cached = await getLocal(CONFIG_CACHE_KEY) as LoadedConfig | undefined;
  if (cached === undefined || typeof cached.digest !== "string") return;
  try {
    validateConfig(cached.config);
    publishConfig({ config: cached.config, digest: cached.digest });
  } catch { await setLocal(CONFIG_CACHE_KEY, undefined); }
}

let queue: Promise<void> = Promise.resolve();
let client: NativeClient | undefined;
const mutatingGroups = new Set<string>();
const expectedRemovals = new Map<string, number>();

class RedactedCookieSetFailure extends Error {
  constructor(readonly category: CookieSetFailureCategory | "no_result") {
    super(`cookie_set_${category}`);
    this.name = "RedactedCookieSetFailure";
  }
}

chrome.idle.setDetectionInterval(IDLE_BASE_SECONDS);
chrome.alarms.create(MONITOR_POLL_ALARM, { periodInMinutes: 0.5 });
setInterval(() => enqueue(pollNativeMonitor), 15_000);

chrome.permissions.onAdded.addListener(() => enqueue(flushPendingAdd));

chrome.webNavigation.onBeforeNavigate.addListener((details) => enqueue(async () => {
  if (details.frameId !== 0) return;
  const loaded = await awaitConfig();
  const group = groupForUrl(loaded.config, details.url);
  if (group !== undefined) await interceptSealedNavigation(loaded, group, details.tabId, details.url);
}));

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (isUnlockPageMessage(message)) {
    enqueue(async () => {
      try { respond(await handleUnlockPageMessage(await awaitConfig(), message.type, sender.tab?.id)); }
      catch { respond({ ok: false, status: "error", error: "unlock_controller_failed" }); }
    });
    return true;
  }
  if (isPopupMessage(message)) {
    enqueue(async () => {
      try { respond(await handlePopupMessage(message)); }
      catch (error: unknown) { respond({ ok: false, error: error instanceof Error ? error.message : "popup_controller_failed" }); }
    });
    return true;
  }
  return;
});

interface PendingAdd {
  scope: string;
  displayName: string;
  policyLevel: PolicyLevel;
  stagedAtUnixMs: number;
}

function isPendingAdd(value: unknown): value is PendingAdd {
  const pending = value as Partial<PendingAdd> | undefined;
  return typeof pending?.scope === "string" && typeof pending.displayName === "string" &&
    typeof pending.policyLevel === "string" && typeof pending.stagedAtUnixMs === "number";
}

async function flushPendingAdd(): Promise<void> {
  const stored = await getLocal(PENDING_ADD_KEY);
  if (!isPendingAdd(stored)) return;
  if (Date.now() - stored.stagedAtUnixMs > PENDING_ADD_TTL_MS) {
    await setLocal(PENDING_ADD_KEY, undefined);
    return;
  }
  // Keep the intent if the host is not up yet; the handshake retries this.
  if (!client?.connected) { void connect(); return; }
  const group = loadedConfig?.config.groups.find((item) => item.scope === stored.scope);
  if (group !== undefined) { await setLocal(PENDING_ADD_KEY, undefined); return; }
  send("group.add", { scope: stored.scope, display_name: stored.displayName, policy_level: stored.policyLevel });
  await setLocal(PENDING_ADD_KEY, undefined);
}

type PopupMessage =
  | { type: "popup.state"; url?: string }
  | { type: "popup.stageProtect"; scope: string; displayName: string; policyLevel: PolicyLevel }
  | { type: "popup.cancelProtect" }
  | { type: "popup.log" }
  | { type: "popup.clearLog" }
  | { type: "popup.protect"; scope: string; displayName: string; policyLevel: PolicyLevel }
  | { type: "popup.unprotect"; groupId: string }
  | { type: "popup.setPolicy"; groupId: string; policyLevel: PolicyLevel };

// ADR-020: protection starts and ends with an explicit user gesture in the popup. Nothing about
// the page's session state is inspected — whatever cookies the scope holds are what gets vaulted.
async function handlePopupMessage(message: PopupMessage): Promise<Record<string, unknown>> {
  if (message.type === "popup.state") {
    await restoreCachedConfig();
    const groups = loadedConfig === undefined ? [] : await popupGroupSummaries(loadedConfig);
    let host = "";
    try {
      const url = message.url === undefined ? undefined : new URL(message.url);
      if (url?.protocol === "http:" || url?.protocol === "https:") host = url.hostname;
    } catch { host = ""; }
    const error = lastConfigError;
    lastConfigError = undefined;
    const storedAlert = await getLocal(LAST_ALERT_KEY);
    const alert = isStoredAlert(storedAlert) ? storedAlert : undefined;
    if (alert !== undefined) {
      // Opening the popup is the acknowledgement: the alert is shown here, so the badge that
      // pointed at it is cleared. Leaving it lit forever taught the user to ignore it.
      await setLocal(LAST_ALERT_KEY, undefined);
      await setBadge("", "#b3261e");
    }
    return {
      ok: true, connected: client?.connected === true, host,
      suggestedScope: guessScope(host), groups, error, alert,
    };
  }
  if (message.type === "popup.log") {
    const stored = await getLocal(ALERT_LOG_KEY);
    const log = Array.isArray(stored) ? stored.filter(isStoredAlert) : [];
    return { ok: true, log: [...log].reverse() };
  }
  if (message.type === "popup.clearLog") {
    await setLocal(ALERT_LOG_KEY, []);
    return { ok: true };
  }
  if (message.type === "popup.stageProtect") {
    await setLocal(PENDING_ADD_KEY, {
      scope: message.scope, displayName: message.displayName,
      policyLevel: message.policyLevel, stagedAtUnixMs: Date.now(),
    });
    return { ok: true };
  }
  if (message.type === "popup.cancelProtect") {
    await setLocal(PENDING_ADD_KEY, undefined);
    return { ok: true };
  }
  if (!client?.connected) { void connect(); return { ok: false, error: "native_host_not_connected" }; }
  if (message.type === "popup.protect") {
    await flushPendingAdd();
    return { ok: true };
  }
  if (message.type === "popup.setPolicy") {
    send("group.set_policy", { account_group_id: message.groupId, policy_level: message.policyLevel });
    return { ok: true };
  }
  send("group.remove", { account_group_id: message.groupId });
  return { ok: true };
}

async function popupGroupSummaries(loaded: LoadedConfig): Promise<Array<Record<string, unknown>>> {
  const root = await loadState(loaded);
  const summaries: Array<Record<string, unknown>> = [];
  for (const group of loaded.config.groups) {
    summaries.push({
      id: group.id, scope: group.scope, displayName: group.display_name,
      policyLevel: group.policy_level, state: root.groups[group.id]?.groupState ?? "uninitialized",
      hasPermission: await hasScopePermission(group),
      cookieCount: (await getCookies(group)).length,
    });
  }
  return summaries;
}

function isPopupMessage(value: unknown): value is PopupMessage {
  const type = typeof value === "object" && value !== null ? (value as { type?: unknown }).type : undefined;
  return type === "popup.state" || type === "popup.protect" || type === "popup.unprotect" ||
    type === "popup.log" || type === "popup.clearLog" || type === "popup.stageProtect" ||
    type === "popup.cancelProtect" || type === "popup.setPolicy";
}

chrome.tabs.onUpdated.addListener((tabId, change, tab) => enqueue(async () => {
  if (change.status !== "complete" && change.url === undefined) return;
  const loaded = await awaitConfig();
  const activeGroup = groupForUrl(loaded.config, tab.url);
  const root = await loadState(loaded);
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    if (state === undefined) continue;
    state.relevantTabs = activeGroup?.id === group.id
      ? unique([...state.relevantTabs, tabId])
      : state.relevantTabs.filter((id) => id !== tabId);
    if (activeGroup?.id === group.id && tab.status === "complete" && state.reconciliation) {
      state.injectAfterReconciliation = true;
    }
  }
  await saveState(root);
  if (activeGroup === undefined || tab.status !== "complete") return;
  const state = root.groups[activeGroup.id];
  if (state === undefined) return;
  const gatePending = state.pendingNavigationUnlocks?.[String(tabId)] !== undefined;
  if (state.groupState === "sealed" && !state.reconciliation && !gatePending) {
    await requestLease(loaded, activeGroup, "inject");
  } else if (state.groupState === "degraded") {
    await requestEviction(loaded, activeGroup, "startup_reconciliation", state.leaseId);
  }
}));

chrome.tabs.onRemoved.addListener((tabId) => enqueue(async () => {
  const loaded = await awaitConfig();
  const root = await loadState(loaded);
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    if (state === undefined) continue;
    delete state.pendingNavigationUnlocks?.[String(tabId)];
    if (state.navigationUnlockRequestTabId === tabId) state.navigationUnlockRequestTabId = undefined;
    state.relevantTabs = await relevantTabIds(group);
  }
  await saveState(root);
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    if (state?.relevantTabs.length !== 0 || !awaitsCapture(state)) continue;
    const grace = policyParameters(group.policy_level).lastTabGraceMs;
    if (grace === 0) await requestEviction(loaded, group, "last_tab_closed", state.leaseId);
    else chrome.alarms.create(alarmName("last_tab", group.id), { when: Date.now() + grace });
  }
}));

chrome.idle.onStateChanged.addListener((idleState) => enqueue(async () => {
  const loaded = await awaitConfig();
  const root = await loadState(loaded);
  if (idleState === "active") {
    for (const group of loaded.config.groups) chrome.alarms.clear(alarmName("idle", group.id));
    return;
  }
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    if (idleState === "locked") {
      if (client?.connected) send("auth.cache.clear", { account_group_id: group.id, reason: "locked" });
      if (awaitsCapture(state)) await requestEviction(loaded, group, "locked", state?.leaseId);
      continue;
    }
    if (!awaitsCapture(state)) continue;
    const threshold = policyParameters(group.policy_level).idleThresholdSeconds;
    const remainingMs = Math.max(0, (threshold - IDLE_BASE_SECONDS) * 1_000);
    if (remainingMs === 0) await requestEviction(loaded, group, "idle", state?.leaseId);
    else chrome.alarms.create(alarmName("idle", group.id), { when: Date.now() + remainingMs });
  }
}));

chrome.alarms.onAlarm.addListener((alarm) => enqueue(async () => {
  if (alarm.name === MONITOR_POLL_ALARM) {
    await pollNativeMonitor();
    return;
  }
  const parsed = parseAlarmName(alarm.name);
  if (parsed === undefined) return;
  const loaded = await awaitConfig();
  const group = loaded.config.groups.find((item) => item.id === parsed.groupId);
  if (group === undefined) return;
  const root = await loadState(loaded);
  const state = root.groups[group.id];
  if (parsed.kind === "expiry") {
    if (state?.groupState === "leased") await requestEviction(loaded, group, "expiry", state.leaseId);
    return;
  }
  if (!awaitsCapture(state)) return;
  if (parsed.kind === "last_tab" && (await relevantTabIds(group)).length === 0) await requestEviction(loaded, group, "last_tab_closed", state?.leaseId);
  else if (parsed.kind === "idle") {
    const idleState = await queryIdleState(policyParameters(group.policy_level).idleThresholdSeconds);
    if (idleState === "idle" || idleState === "locked") await requestEviction(loaded, group, idleState, state?.leaseId);
  }
}));

chrome.cookies.onChanged.addListener((info) => enqueue(async () => {
  const loaded = await awaitConfig();
  const group = groupForCookie(loaded.config, info.cookie);
  if (group === undefined) return;
  // ADR-020: cookie removals carry no meaning. The system does not try to tell a real logout
  // from a session-cookie rotation; a stale vault self-heals on the next capture.
  if (info.removed) return;
  if (mutatingGroups.has(group.id)) return;
  const root = await loadState(loaded);
  const state = root.groups[group.id];
  if (state?.groupState === "sealed") {
    // PLAN §13.2.2. Sealed means the scope holds no cookies, so whatever appeared — prefetch,
    // analytics, a third-party context — is removed silently. It is never treated as a reason to
    // unlock: raising Hello without the user asking is exactly what Faz 5.1 set out to remove.
    await removeCookie(group, info.cookie);
  } else if (state?.groupState === "degraded") {
    await requestEviction(loaded, group, "degraded_cookie_detected", state.leaseId);
  }
}));

// connect() awaits before it assigns `client`, so two callers could each open a port and Chrome
// would spawn two hosts. Two hosts write the same audit chain and corrupt it, which then fails
// every later start closed (PLAN §23.1), so the second caller is turned away here.
let connecting = false;

async function connect(): Promise<void> {
  if (client?.connected || connecting) return;
  connecting = true;
  try { await openNativeConnection(); } finally { connecting = false; }
}

async function openNativeConnection(): Promise<void> {
  await restoreCachedConfig();
  if (client?.connected) return;
  client = new NativeClient(loadedConfig?.digest, handleHostMessage, async () => {
    const loaded = await awaitConfig();
    const root = await loadState(loaded);
    const activeGroups: string[] = [];
    for (const group of loaded.config.groups) {
      const state = root.groups[group.id];
      if (state === undefined || (state.groupState !== "leased" && state.groupState !== "evicting")) continue;
      activeGroups.push(group.id);
      try { await removeAllCookies(group); } catch { /* continue fail-closed cleanup for other groups */ }
      state.groupState = "degraded";
      state.lastEvent = "native_disconnect_fail_closed";
    }
    await saveState(root);
    if (activeGroups.length === 0) await queueMonitorEvent("host_disconnect", undefined, false);
    else for (const groupId of activeGroups) await queueMonitorEvent("host_disconnect_active_lease", groupId, false);
    setTimeout(() => { void connect(); }, 1_000);
  });
  client.start();
}

let lastConfigError: string | undefined;

async function handleHostMessage(message: WireMessage): Promise<void> {
  if (message.type === "handshake.ack") {
    await handleHandshakeAck(message.payload);
    return;
  }
  if (message.type === "config.updated") {
    await adoptConfig(message.payload.config as AccountGroupsConfig, requiredString(message.payload, "config_digest"));
    // Runtime state is keyed by the config digest, so a config change resets it. The host sends
    // the authoritative per-group state alongside the new config to restore it without a
    // needless reconciliation round for groups that did not change.
    if (!Array.isArray(message.payload.groups)) throw new Error("config.updated groups must be an array");
    await applyHostGroupStates(await awaitConfig(), message.payload.groups as HandshakeGroupState[]);
    lastConfigError = undefined;
    return;
  }
  if (message.type === "config.rejected") {
    lastConfigError = requiredString(message.payload, "reason");
    return;
  }
  if (message.type === "monitor.alert") {
    await handleMonitorAlert(message.payload.event);
    return;
  }
  const loaded = await awaitConfig();
  const groupId = requiredString(message.payload, "account_group_id");
  const group = loaded.config.groups.find((item) => item.id === groupId);
  if (group === undefined) throw new Error("host referenced unknown account group");
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  switch (message.type) {
    case "lease.grant":
      state.leaseId = requiredString(message.payload, "lease_id");
      chrome.alarms.create(alarmName("expiry", group.id), { when: requiredNumber(message.payload, "expiry_unix_ms") });
      state.pendingLeaseRequest = undefined;
      state.lastEvent = "lease_grant";
      await saveState(root);
      break;
    case "lease.deny": {
      const deniedLease = state.pendingLeaseRequest;
      const deniedEviction = state.evictionRequestPending === true;
      state.pendingLeaseRequest = undefined;
      state.evictionRequestPending = false;
      if (deniedEviction) state.groupState = "degraded";
      else if (deniedLease === "enroll") state.groupState = "uninitialized";
      else if (deniedLease === "inject") {
        state.groupState = "sealed";
        if (state.navigationUnlockRequestTabId !== undefined) {
          state.navigationUnlockRequestTabId = undefined;
          state.navigationUnlockError = "hello_rejected_or_unlock_denied";
        }
      }
      state.lastEvent = `lease_deny:${requiredString(message.payload, "reason")}`;
      await saveState(root);
      if (deniedEviction) await queueMonitorEvent("reconciliation_failed", group.id);
      break;
    }
    case "cookies.inject": await injectCookies(loaded, group, message.payload, root, state); break;
    case "evict.request":
      if (requiredString(message.payload, "phase") !== "snapshot_required") throw new Error("unsupported eviction phase");
      state.groupState = "evicting";
      state.evictionRequestPending = true;
      state.lastEvent = `snapshot_required:${requiredString(message.payload, "reason")}`;
      await saveState(root);
      send("cookies.snapshot", {
        account_group_id: group.id,
        lease_id: optionalString(message.payload, "lease_id"),
        operation_id: requiredString(message.payload, "operation_id"),
        cookies: (await getCookies(group)).map(cookieRecord),
      });
      break;
    case "evict.confirmed": await finishEviction(loaded, group, message.payload, root, state); break;
    case "session.invalidated":
      clearGroupAlarms(group.id);
      try { await removeAllCookies(group); } catch { state.lastEvent = "session_invalidated_cleanup_failed"; }
      resetGroupState(state, "session_invalidated");
      await saveState(root);
      break;
    default: throw new Error(`unsupported host message ${message.type}`);
  }
}

async function applyHostGroupStates(loaded: LoadedConfig, summaries: HandshakeGroupState[]): Promise<void> {
  const root = await loadState(loaded);
  for (const group of loaded.config.groups) {
    const summary = summaries.find((item) => item.account_group_id === group.id);
    if (summary === undefined) continue;
    const state = requiredGroupState(root, group.id);
    state.groupState = summary.group_state;
    state.leaseId = summary.lease_id ?? undefined;
    state.reconciliation = summary.reconciliation_required;
    state.relevantTabs = await relevantTabIds(group);
    state.lastEvent = "config_updated";
  }
  await saveState(root);
}

async function handleHandshakeAck(payload: Record<string, unknown>): Promise<void> {
  if (requiredNumber(payload, "protocol_version") !== PROTOCOL_VERSION) throw new Error("host protocol mismatch");
  await adoptConfig(payload.config as AccountGroupsConfig, requiredString(payload, "config_digest"));
  const loaded = await awaitConfig();
  if (!Array.isArray(payload.groups)) throw new Error("handshake groups must be an array");
  const summaries = payload.groups as HandshakeGroupState[];
  if (summaries.length !== loaded.config.groups.length) throw new Error("handshake group count mismatch");
  const root = await loadState(loaded);
  const actions: Array<() => Promise<void>> = [];
  for (const group of loaded.config.groups) {
    const summary = summaries.find((item) => item.account_group_id === group.id);
    if (summary === undefined) throw new Error("handshake omitted configured group");
    const state = requiredGroupState(root, group.id);
    const pendingInvalidation = state.invalidationPending === true ? state.invalidationReason : undefined;
    state.groupState = summary.group_state;
    state.leaseId = summary.lease_id ?? undefined;
    state.relevantTabs = await relevantTabIds(group);
    const cookies = await getCookies(group);
    state.pendingLeaseRequest = undefined;
    state.evictionRequestPending = false;
    state.reconciliation = summary.reconciliation_required;
    state.injectAfterReconciliation = summary.reconciliation_required && state.relevantTabs.length > 0;
    state.lastEvent = "handshake_ack";
    if (state.groupState === "uninitialized") {
      state.invalidationPending = false;
      state.invalidationReason = undefined;
    }
    if (pendingInvalidation !== undefined && state.groupState !== "uninitialized") {
      actions.push(() => requestSessionInvalidation(loaded, group, pendingInvalidation));
    } else if (state.groupState === "leased" && (cookies.length === 0 || state.relevantTabs.length === 0)) {
      const reason = cookies.length === 0 ? "startup_reconciliation" : "last_tab_closed";
      actions.push(() => requestEviction(loaded, group, reason, state.leaseId));
    } else if (state.groupState === "leased" && typeof summary.lease_expiry_unix_ms === "number") {
      if (summary.lease_expiry_unix_ms <= Date.now()) actions.push(() => requestEviction(loaded, group, "expiry", state.leaseId));
      else chrome.alarms.create(alarmName("expiry", group.id), { when: summary.lease_expiry_unix_ms });
    } else if (state.groupState === "sealed" && cookies.length > 0 && state.relevantTabs.length === 0) {
      actions.push(() => removeAllCookies(group));
    } else if (summary.reconciliation_required) {
      actions.push(() => requestEviction(loaded, group, "startup_reconciliation", state.leaseId));
    }
  }
  await saveState(root);
  for (const action of actions) await action();
  const pending = await monitorOutbox();
  if (pending.some((event) => event.signal === "host_disconnect" || event.signal === "host_disconnect_active_lease")) {
    await queueMonitorEvent("reconnect_success", undefined, false);
  }
  await flushMonitorOutbox();
  await pollNativeMonitor();
  await flushPendingAdd();
}

async function injectCookies(loaded: LoadedConfig, group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const leaseId = requiredString(payload, "lease_id");
  if (!Array.isArray(payload.cookies)) throw new Error("cookies.inject cookies must be an array");
  const records = payload.cookies;
  let health = "inject_execution_failed";
  let success = false;
  let stage = "cookie_set";
  try {
    // A cookie whose expiry has passed since it was vaulted cannot be restored: chrome.cookies.set
    // accepts it, drops it immediately and reports no cookie, which would fail the whole inject
    // over an entry the browser would have discarded anyway. Short-lived cookies (bot-management
    // and similar) hit this routinely once a scope's full cookie set is vaulted.
    const restorable = records.filter((raw) => !isExpired(raw as CookieRecord));
    mutatingGroups.add(group.id);
    try {
      for (const raw of restorable) {
        const cookie = raw as CookieRecord;
        if (!cookieBelongsToGroup(group, cookie)) throw new Error("vault cookie outside group scope");
        await setCookie(group, cookie);
      }
    } finally { mutatingGroups.delete(group.id); }
    stage = "cookie_roundtrip";
    const expectedRecords = restorable as CookieRecord[];
    // Each chrome.cookies.set() callback above already confirmed its own write, but an
    // immediate getAll() can still occasionally miss the last write or two — a brief,
    // internal-to-Chrome read lag, not a real failure. A couple of short retries absorb that
    // without masking a genuinely persistent mismatch, which still fails after all attempts.
    //
    // The check is a subset check (every vaulted cookie landed), not set equality: a site's own
    // page script can set additional cookies (e.g. YouTube's `GPS`) in the same window we are
    // restoring into, and that is not a restore failure — only a vaulted cookie failing to land
    // is.
    let roundtripMatched = false;
    let missing: CookieRecord[] = [];
    for (let attempt = 0; attempt < 3 && !roundtripMatched; attempt += 1) {
      if (attempt > 0) await new Promise((resolve) => setTimeout(resolve, 100));
      const actualIdentities = new Set((await getCookies(group)).map(cookieIdentity));
      missing = expectedRecords.filter((record) => !actualIdentities.has(cookieIdentity(record)));
      roundtripMatched = missing.length === 0;
    }
    if (!roundtripMatched) {
      health = "cookie_roundtrip_failed";
      for (const record of missing) {
        console.error(`FCP roundtrip missing cookie group=${group.scope} name=${record.name} domain=${record.domain} path=${record.path}`);
      }
    } else {
      // ADR-020: a verified round-trip is the whole success condition. No site-specific health
      // check runs, so the extension never has to decide whether a restored session is "real".
      const navigationTabId = state.navigationUnlockRequestTabId;
      const navigationTarget = navigationTabId === undefined ? undefined : state.pendingNavigationUnlocks?.[String(navigationTabId)];
      if (navigationTabId !== undefined && navigationTarget !== undefined) {
        stage = "navigation_gate_redirect";
        await updateTab(navigationTabId, navigationTarget);
        await waitForRelevantTabComplete(loaded.config, group, navigationTabId);
      } else {
        state.relevantTabs = await relevantTabIds(group);
        await saveState(root);
      }
      health = "restored";
    }
    success = health === "restored";
  } catch (error: unknown) {
    health = stage === "cookie_set" && error instanceof RedactedCookieSetFailure ? `cookie_set_${error.category}` : `${stage}_failed`;
    console.error(`FCP group ${group.id} inject failed: ${health}`);
  }
  if (!success) {
    chrome.alarms.clear(alarmName("expiry", group.id));
    try { await removeAllCookies(group); } catch { health = "inject_cleanup_failed"; }
  }
  state.groupState = success ? "leased" : "degraded";
  state.leaseId = leaseId;
  if (success && state.navigationUnlockRequestTabId !== undefined) {
    const completedTabId = state.navigationUnlockRequestTabId;
    const remaining = Object.entries(state.pendingNavigationUnlocks ?? {}).filter(([tabId]) => Number(tabId) !== completedTabId);
    state.pendingNavigationUnlocks = {};
    state.navigationUnlockRequestTabId = undefined;
    state.navigationUnlockError = undefined;
    for (const [tabId, target] of remaining) if (groupForUrl(loaded.config, target)?.id === group.id) void updateTab(Number(tabId), target).catch(() => undefined);
  } else if (!success && state.navigationUnlockRequestTabId !== undefined) {
    state.navigationUnlockRequestTabId = undefined;
    state.navigationUnlockError = `inject_failed:${health}`;
  }
  state.lastEvent = `inject:${health}`;
  await saveState(root);
  send("inject.result", { account_group_id: group.id, lease_id: leaseId, success, health_check: health });
  if (!success) setTimeout(() => enqueue(() => requestEviction(loaded, group, "startup_reconciliation", leaseId)), 100);
}

async function finishEviction(loaded: LoadedConfig, group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const disposition = requiredString(payload, "cookie_disposition");
  const shouldInject = state.reconciliation && state.injectAfterReconciliation === true && state.relevantTabs.length > 0;
  const operationId = requiredString(payload, "operation_id");
  if (disposition === "retain_leased") {
    const count = (await getCookies(group)).length;
    send("evict.result", { account_group_id: group.id, lease_id: optionalString(payload, "lease_id"), operation_id: operationId, success: count > 0, remaining_cookie_count: count });
    state.groupState = count > 0 ? "leased" : "degraded";
    state.reconciliation = false;
    state.evictionRequestPending = false;
    state.injectAfterReconciliation = false;
    state.lastEvent = count > 0 ? "enrollment_retained_leased" : "enrollment_cookie_missing";
    // An enrollment started only to capture a scope the user has already stopped using
    // (see requestEviction) hands straight over to the eviction it was standing in for.
    const chained = state.evictAfterEnrollment;
    state.evictAfterEnrollment = undefined;
    await saveState(root);
    if (count === 0) await queueMonitorEvent("reconciliation_failed", group.id);
    else if (chained !== undefined) await requestEviction(loaded, group, chained, state.leaseId);
    return;
  }
  if (disposition !== "remove") throw new Error("unsupported cookie disposition");
  await removeAllCookies(group);
  const remaining = (await getCookies(group)).length;
  send("evict.result", { account_group_id: group.id, lease_id: optionalString(payload, "lease_id"), operation_id: operationId, success: remaining === 0, remaining_cookie_count: remaining });
  state.groupState = remaining === 0 ? "sealed" : "degraded";
  clearGroupAlarms(group.id);
  state.leaseId = undefined;
  state.reconciliation = false;
  state.evictionRequestPending = false;
  state.injectAfterReconciliation = false;
  state.lastEvent = remaining === 0 ? "eviction_complete" : "eviction_failed";
  await saveState(root);
  if (remaining !== 0) await queueMonitorEvent("reconciliation_failed", group.id);
  if (state.invalidationPending === true && state.invalidationReason !== undefined) {
    send("session.invalidate", { account_group_id: group.id, lease_id: state.leaseId, reason: state.invalidationReason });
  } else if (remaining === 0 && shouldInject) {
    const tabId = state.relevantTabs[0];
    if (tabId !== undefined) await offerUnlockOnTab(loaded, group, tabId);
  }
}

async function requestLease(loaded: LoadedConfig, group: AccountGroup, purpose: LeasePurpose): Promise<void> {
  if (policyParameters(group.policy_level).monitoringOnly) return;
  if (!await hasScopePermission(group)) return void markPermissionMissing(loaded, group);
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.invalidationPending || state.pendingLeaseRequest !== undefined || state.evictionRequestPending) return;
  if ((purpose === "enroll" && state.groupState !== "uninitialized") || (purpose === "inject" && state.groupState !== "sealed")) return;
  state.pendingLeaseRequest = purpose;
  state.groupState = "unlocking";
  state.lastEvent = `lease_request_pending:${purpose}`;
  await saveState(root);
  send("lease.request", { account_group_id: group.id, purpose });
}

async function requestEviction(loaded: LoadedConfig, group: AccountGroup, reason: string, leaseId: string | undefined): Promise<void> {
  if (!await hasScopePermission(group)) return void markPermissionMissing(loaded, group);
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.invalidationPending || state.pendingLeaseRequest !== undefined || state.evictionRequestPending) return;
  if (USER_IDLE_TRIGGERS.has(reason)) {
    const present = (await getCookies(group)).length;
    // ADR-020: nothing in the scope means nothing to protect. Discard the vault rather than
    // sealing an empty jar, which would cost a Hello gesture on the next visit for no session.
    if (present === 0) {
      if (state.groupState === "leased") await requestSessionInvalidation(loaded, group, "scope_empty");
      return;
    }
    // The scope holds cookies but was never captured: enroll first, then evict what was
    // captured. Replaces the old "did a login just happen" heuristic.
    if (state.groupState === "uninitialized") {
      state.evictAfterEnrollment = reason;
      await saveState(root);
      await requestLease(loaded, group, "enroll");
      return;
    }
  }
  const valid = state.groupState === "leased" ||
    (state.groupState === "degraded" && reason === "degraded_cookie_detected") ||
    (reason === "startup_reconciliation" && state.groupState !== "uninitialized");
  if (!valid) return;
  state.evictionRequestPending = true;
  state.groupState = "evicting";
  state.lastEvent = `eviction_request_pending:${reason}`;
  await saveState(root);
  send("evict.request", { account_group_id: group.id, lease_id: leaseId, operation_id: crypto.randomUUID(), phase: "begin", reason });
}

async function markPermissionMissing(loaded: LoadedConfig, group: AccountGroup): Promise<void> {
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.lastEvent === "permission_missing") return;
  state.lastEvent = "permission_missing";
  await saveState(root);
  console.warn(`FCP group ${group.id} is configured but the extension lacks host permission for its scope`);
}

async function requestSessionInvalidation(loaded: LoadedConfig, group: AccountGroup, reason: SessionInvalidationReason): Promise<void> {
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.groupState === "uninitialized" || state.invalidationPending) return;
  state.invalidationPending = true;
  state.invalidationReason = reason;
  state.lastEvent = `session_invalidation_pending:${reason}`;
  await saveState(root);
  if (!state.evictionRequestPending) send("session.invalidate", { account_group_id: group.id, lease_id: state.leaseId, reason });
}

// Reconciliation can leave a sealed group with the site still open; the tab is put through the
// same gate as an ordinary navigation.
async function offerUnlockOnTab(loaded: LoadedConfig, group: AccountGroup, tabId: number): Promise<void> {
  try {
    const tab = await callbackPromise<chrome.tabs.Tab>((done) => chrome.tabs.get(tabId, done));
    if (tab.url === undefined || groupForUrl(loaded.config, tab.url)?.id !== group.id) return;
    await interceptSealedNavigation(loaded, group, tabId, tab.url);
  } catch { /* the tab closed while reconciliation ran */ }
}

// The interstitial exists to hold the target URL and to give a place to land on failure, not to
// demand a second click: reaching a protected site is itself the user's request to open it, so
// Hello starts immediately. A cancelled prompt returns to the interstitial with a retry button.
async function startUnlockOnTab(loaded: LoadedConfig, group: AccountGroup, tabId: number, event: string): Promise<void> {
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.groupState !== "sealed" || state.pendingLeaseRequest !== undefined || state.evictionRequestPending || state.reconciliation) return;
  if (!client?.connected) { void connect(); return; }
  state.navigationUnlockRequestTabId = tabId;
  state.navigationUnlockError = undefined;
  state.lastEvent = event;
  await saveState(root);
  await requestLease(loaded, group, "inject");
}

async function interceptSealedNavigation(loaded: LoadedConfig, group: AccountGroup, tabId: number, targetUrl: string): Promise<void> {
  // Without permission the unlock gate could never complete, so the page is left alone rather
  // than trapping the user on an interstitial that cannot succeed.
  if (!await hasScopePermission(group)) return void markPermissionMissing(loaded, group);
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.groupState !== "sealed" || state.reconciliation || state.invalidationPending || state.pendingLeaseRequest !== undefined || state.evictionRequestPending) return;
  state.pendingNavigationUnlocks ??= {};
  state.pendingNavigationUnlocks[String(tabId)] = targetUrl;
  state.navigationUnlockError = undefined;
  state.lastEvent = "navigation_unlock_intercepted";
  await saveState(root);
  await updateTab(tabId, UNLOCK_PAGE_URL);
  await startUnlockOnTab(loaded, group, tabId, "navigation_auto_unlock");
}

async function handleUnlockPageMessage(loaded: LoadedConfig, type: "unlock.status" | "unlock.start", tabId: number | undefined): Promise<Record<string, unknown>> {
  if (tabId === undefined) return { ok: false, status: "error", error: "missing_tab_context" };
  const root = await loadState(loaded);
  let match: { group: AccountGroup; state: GroupRuntimeState; target: string } | undefined;
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    const target = state?.pendingNavigationUnlocks?.[String(tabId)];
    if (state !== undefined && target !== undefined) { match = { group, state, target }; break; }
  }
  if (match === undefined || groupForUrl(loaded.config, match.target)?.id !== match.group.id) return { ok: false, status: "error", error: "navigation_context_missing" };
  const { group, state, target } = match;
  if (state.groupState === "leased") {
    delete state.pendingNavigationUnlocks?.[String(tabId)];
    await saveState(root);
    await updateTab(tabId, target);
    return { ok: true, status: "redirecting" };
  }
  if (type === "unlock.status") {
    if (state.groupState === "sealed" && state.pendingLeaseRequest === undefined) return { ok: true, status: state.navigationUnlockError === undefined ? "ready" : "error", error: state.navigationUnlockError };
    if (state.groupState === "unlocking" && state.navigationUnlockRequestTabId === tabId) return { ok: true, status: "unlocking" };
    return { ok: true, status: "recovering" };
  }
  if (state.groupState !== "sealed" || state.pendingLeaseRequest !== undefined || state.evictionRequestPending || state.reconciliation) return { ok: false, status: "recovering", error: "group_not_ready" };
  if (!client?.connected) { void connect(); return { ok: false, status: "error", error: "native_host_not_connected" }; }
  state.navigationUnlockRequestTabId = tabId;
  state.navigationUnlockError = undefined;
  state.lastEvent = "navigation_unlock_user_gesture";
  await saveState(root);
  await requestLease(loaded, group, "inject");
  return { ok: true, status: "unlocking" };
}

class NativeClient {
  private port?: chrome.runtime.Port;
  private readonly nonce = randomNonce();
  private outgoing = 0;
  private incoming = 0;
  constructor(private readonly cachedConfigDigest: string | undefined, private readonly onMessage: (message: WireMessage) => Promise<void>, private readonly onDisconnect: () => Promise<void>) {}
  get connected(): boolean { return this.port !== undefined; }
  start(): void {
    const port = chrome.runtime.connectNative(HOST_NAME);
    this.port = port;
    port.onMessage.addListener((raw) => enqueue(() => this.receive(raw)));
    port.onDisconnect.addListener(() => { this.port = undefined; enqueue(this.onDisconnect); });
    this.send("handshake", { protocol_version: PROTOCOL_VERSION, extension_id: chrome.runtime.id, cached_config_digest: this.cachedConfigDigest ?? null });
  }
  send(type: string, payload: Record<string, unknown>): void {
    const port = this.port;
    if (port === undefined) throw new Error("native host is not connected");
    this.outgoing += 1;
    try {
      port.postMessage({ v: PROTOCOL_VERSION, conn_nonce: this.nonce, seq: this.outgoing, id: crypto.randomUUID(), type, payload });
    } catch (error: unknown) {
      // The port can die between the connected check and this write (a service-worker restart
      // after a permission grant does exactly that). Drop it here so the reconnect path runs
      // instead of repeatedly writing into a dead port.
      this.port = undefined;
      throw error instanceof Error ? error : new Error("native host write failed");
    }
  }
  private async receive(raw: unknown): Promise<void> {
    if (!isEnvelope(raw)) throw new Error("malformed native envelope");
    if (raw.v !== PROTOCOL_VERSION || raw.conn_nonce !== this.nonce || raw.seq !== this.incoming + 1) throw new Error("native nonce/sequence validation failed");
    this.incoming = raw.seq;
    await this.onMessage({ type: raw.type, payload: raw.payload });
  }
}

function send(type: string, payload: Record<string, unknown>): void {
  if (!client?.connected) throw new Error("native host is not connected");
  client.send(type, payload);
}

async function queueMonitorEvent(signal: MonitorSignal, groupId?: string, flush = true): Promise<void> {
  const previous = await monitorOutbox();
  const next = addToBoundedOutbox(previous, makeMonitorEvent(signal, groupId));
  const overflowed = previous.length === 128 && next.length === 128 &&
    !previous.some((event) => event.signal === signal && (event.account_group_id ?? undefined) === groupId);
  const finalOutbox = overflowed
    ? addToBoundedOutbox(next, makeMonitorEvent("monitor_queue_overflow"))
    : next;
  await setLocal(MONITOR_OUTBOX_KEY, finalOutbox);
  if (flush) await flushMonitorOutbox();
}

async function monitorOutbox(): Promise<MonitorEvent[]> {
  const value = await getLocal(MONITOR_OUTBOX_KEY);
  if (!Array.isArray(value)) return [];
  return value.filter(validateMonitorEvent);
}

async function flushMonitorOutbox(): Promise<void> {
  if (!client?.connected) return;
  for (const event of await monitorOutbox()) send("monitor.event", { ...event });
}

async function pollNativeMonitor(): Promise<void> {
  if (!client?.connected) return;
  await flushMonitorOutbox();
  send("monitor.poll", { max_events: 32 });
}

async function handleMonitorAlert(value: unknown): Promise<void> {
  if (!validateMonitorEvent(value)) throw new Error("invalid monitor alert");
  const event = value;
  if (event.source === "extension") {
    const pending = await monitorOutbox();
    await setLocal(MONITOR_OUTBOX_KEY, pending.filter((item) => item.event_id !== event.event_id));
  }
  if (event.severity === "info") return;
  // The rate limit exists to stop notification spam, not to hide events. The toast is the noisy,
  // interrupting channel and stays limited; the badge and the popup record are cheap and are
  // always refreshed, so an event that arrives right after the user acknowledged the previous one
  // still leaves a visible trace instead of passing silently.
  await recordAlert(event);
  const previousValue = await getLocal(MONITOR_RATE_KEY);
  const previous = isNotificationDecisionState(previousValue) ? previousValue : {};
  const decision = notificationDecision(event, previous, Date.now());
  if (!decision.show) return;
  await setLocal(MONITOR_RATE_KEY, decision.next);
  const content = notificationText(event);
  await createNotification(`fcp-monitor-${event.event_id}`, {
    type: "basic", iconUrl: MONITOR_ICON_URL, title: content.title, message: content.message,
    priority: event.severity === "high" ? 2 : 1,
  });
}

async function recordAlert(event: MonitorEvent): Promise<void> {
  const storedValue = await getLocal(LAST_ALERT_KEY);
  const stored = isStoredAlert(storedValue) ? storedValue : undefined;
  const sameAsStored = stored?.signal === event.signal && stored.accountGroupId === (event.account_group_id ?? null);
  const entry: StoredAlert = {
    signal: event.signal, severity: event.severity,
    accountGroupId: event.account_group_id ?? null, observedAtUnixMs: event.observed_at_unix_ms,
    occurrences: sameAsStored ? stored.occurrences + 1 : 1,
  };
  await setLocal(LAST_ALERT_KEY, entry);
  await appendAlertLog(entry);
  await setBadge(event.severity === "high" ? "!" : "•", event.severity === "high" ? "#b3261e" : "#b06000");
}

// One browser launch can emit the same signal many times over (PLAN §30 dedup finding), so the
// newest entry is coalesced rather than pushed repeatedly; the raw sequence stays in the audit log.
async function appendAlertLog(entry: StoredAlert): Promise<void> {
  const stored = await getLocal(ALERT_LOG_KEY);
  const log = Array.isArray(stored) ? stored.filter(isStoredAlert) : [];
  const newest = log[log.length - 1];
  if (newest !== undefined && newest.signal === entry.signal && newest.accountGroupId === entry.accountGroupId) {
    log[log.length - 1] = entry;
  } else {
    log.push(entry);
  }
  await setLocal(ALERT_LOG_KEY, log.slice(-ALERT_LOG_LIMIT));
}

interface StoredAlert {
  signal: string;
  severity: string;
  accountGroupId: string | null;
  observedAtUnixMs: number;
  occurrences: number;
}

function isStoredAlert(value: unknown): value is StoredAlert {
  const alert = value as Partial<StoredAlert> | undefined;
  return typeof alert?.signal === "string" && typeof alert.severity === "string" &&
    typeof alert.observedAtUnixMs === "number" && typeof alert.occurrences === "number";
}

function isNotificationDecisionState(value: unknown): value is NotificationDecisionState {
  return typeof value === "object" && value !== null && Object.values(value).every((item) => typeof item === "number" && Number.isFinite(item));
}

function isExpired(cookie: CookieRecord): boolean {
  return !cookie.session && typeof cookie.expiration_date === "number" && cookie.expiration_date * 1_000 <= Date.now();
}

async function setCookie(group: AccountGroup, cookie: CookieRecord): Promise<void> {
  const details = cookieSetDetails(group, cookie);
  await new Promise<void>((resolve, reject) => {
    try {
      chrome.cookies.set(details, (value) => {
        const error = chrome.runtime.lastError;
        if (error !== undefined) reject(new RedactedCookieSetFailure(categorizeCookieSetFailure(error.message)));
        else if (value === undefined) reject(new RedactedCookieSetFailure("no_result"));
        else resolve();
      });
    } catch (error: unknown) {
      reject(new RedactedCookieSetFailure(categorizeCookieSetFailure(error instanceof Error ? error.message : undefined)));
    }
  });
}

async function removeCookie(group: AccountGroup, cookie: chrome.cookies.Cookie): Promise<void> {
  mutatingGroups.add(group.id);
  try {
    markExpectedRemoval(group.id, cookie);
    await callbackPromise((done) => chrome.cookies.remove({ url: cookieUrl(cookie), name: cookie.name, storeId: cookie.storeId, partitionKey: cookie.partitionKey }, done));
  } finally { mutatingGroups.delete(group.id); }
}

async function removeAllCookies(group: AccountGroup): Promise<void> {
  mutatingGroups.add(group.id);
  try {
    for (const cookie of await getCookies(group)) {
      markExpectedRemoval(group.id, cookie);
      await callbackPromise((done) => chrome.cookies.remove({ url: cookieUrl(cookie), name: cookie.name, storeId: cookie.storeId, partitionKey: cookie.partitionKey }, done));
    }
  } finally { mutatingGroups.delete(group.id); }
}

// ADR-020: every cookie under the group's registrable domain, with no name filtering. Chrome's
// `domain` filter already covers the scope and its subdomains.
async function getCookies(group: AccountGroup): Promise<chrome.cookies.Cookie[]> {
  const found = await callbackPromise<chrome.cookies.Cookie[]>((done) => chrome.cookies.getAll({ domain: group.scope }, done));
  const uniqueCookies = new Map<string, chrome.cookies.Cookie>();
  for (const cookie of found) if (cookieBelongsToGroup(group, cookie)) uniqueCookies.set(cookieIdentity(cookie), cookie);
  return [...uniqueCookies.values()];
}

// The host's config and the extension's optional host permissions are independent: uninstalling
// the extension drops its granted origins while the host keeps the protected-site list on disk.
// Operating on a scope we cannot read or write would fail on every cookie call, so it is checked
// up front and surfaced to the user instead (PLAN §29.2).
function hasScopePermission(group: AccountGroup): Promise<boolean> {
  return new Promise((resolve) => {
    chrome.permissions.contains({ origins: navigationPatterns(group) }, (granted) => {
      resolve(chrome.runtime.lastError === undefined && granted);
    });
  });
}

async function relevantTabIds(group: AccountGroup): Promise<number[]> {
  const tabs = await callbackPromise<chrome.tabs.Tab[]>((done) => chrome.tabs.query({ url: navigationPatterns(group) }, done));
  return tabs.flatMap((tab) => tab.id !== undefined && groupForUrl({ version: 2, compatibility_version: 2, groups: [group] }, tab.url) !== undefined ? [tab.id] : []);
}


async function loadState(loaded: LoadedConfig): Promise<RuntimeState> {
  const stored = await storageGet(STATE_KEY);
  const existing = stored[STATE_KEY] as RuntimeState | undefined;
  if (existing?.version === 2 && existing.configDigest === loaded.digest) {
    for (const group of loaded.config.groups) existing.groups[group.id] ??= initialGroupState();
    return existing;
  }
  const root: RuntimeState = { version: 2, configDigest: loaded.digest, groups: {} };
  for (const group of loaded.config.groups) root.groups[group.id] = initialGroupState();
  const legacy = (await storageGet(LEGACY_STATE_KEY))[LEGACY_STATE_KEY] as Partial<GroupRuntimeState> | undefined;
  if (legacy?.groupState !== undefined && root.groups[WIKIPEDIA_GROUP_ID] !== undefined) root.groups[WIKIPEDIA_GROUP_ID] = { ...initialGroupState(), ...legacy };
  await saveState(root);
  return root;
}

function saveState(state: RuntimeState): Promise<void> { return callbackPromise<void>((done) => chrome.storage.session.set({ [STATE_KEY]: state }, () => done())); }
function storageGet(key: string): Promise<Record<string, unknown>> { return callbackPromise((done) => chrome.storage.session.get(key, done)); }
async function getLocal(key: string): Promise<unknown> { return (await callbackPromise<Record<string, unknown>>((done) => chrome.storage.local.get(key, done)))[key]; }
function setLocal(key: string, value: unknown): Promise<void> { return callbackPromise<void>((done) => chrome.storage.local.set({ [key]: value }, () => done())); }
function createNotification(id: string, options: chrome.notifications.NotificationOptions): Promise<string> { return callbackPromise((done) => chrome.notifications.create(id, options, done)); }
async function setBadge(text: string, color: string): Promise<void> {
  await callbackPromise<void>((done) => chrome.action.setBadgeBackgroundColor({ color }, done));
  await callbackPromise<void>((done) => chrome.action.setBadgeText({ text }, done));
}
function initialGroupState(): GroupRuntimeState { return { groupState: "uninitialized", reconciliation: false, relevantTabs: [], lastEvent: "startup", pendingNavigationUnlocks: {} }; }
function requiredGroupState(root: RuntimeState, groupId: string): GroupRuntimeState { const state = root.groups[groupId]; if (state === undefined) throw new Error("runtime group missing"); return state; }
function resetGroupState(state: GroupRuntimeState, event: string): void { Object.assign(state, initialGroupState(), { lastEvent: event }); }

function clearGroupAlarms(groupId: string): void { for (const kind of ["expiry", "idle", "last_tab"] as const) chrome.alarms.clear(alarmName(kind, groupId)); }
function alarmName(kind: "expiry" | "idle" | "last_tab", groupId: string): string { return `fcp:${kind}:${groupId}`; }
function parseAlarmName(value: string): { kind: "expiry" | "idle" | "last_tab"; groupId: string } | undefined { const match = /^fcp:(expiry|idle|last_tab):([0-9a-f-]+)$/.exec(value); return match === null ? undefined : { kind: match[1] as "expiry" | "idle" | "last_tab", groupId: match[2] as string }; }
function queryIdleState(seconds: number): Promise<chrome.idle.IdleState> { return callbackPromise((done) => chrome.idle.queryState(seconds, done)); }
function updateTab(tabId: number, url: string): Promise<chrome.tabs.Tab> { return callbackPromise((done) => chrome.tabs.update(tabId, { url }, done)); }
async function waitForRelevantTabComplete(config: AccountGroupsConfig, group: AccountGroup, tabId: number): Promise<void> { const deadline = Date.now() + NAVIGATION_LOAD_TIMEOUT_MS; while (Date.now() < deadline) { const tab = await callbackPromise<chrome.tabs.Tab>((done) => chrome.tabs.get(tabId, done)); if (tab.status === "complete" && groupForUrl(config, tab.url)?.id === group.id) return; await delay(100); } throw new Error("target navigation timeout"); }
function markExpectedRemoval(groupId: string, cookie: chrome.cookies.Cookie): void { expectedRemovals.set(`${groupId}\u0000${cookieIdentity(cookie)}`, Date.now() + 30_000); }
function consumeExpectedRemoval(groupId: string, cookie: chrome.cookies.Cookie): boolean { const key = `${groupId}\u0000${cookieIdentity(cookie)}`; const expiry = expectedRemovals.get(key); expectedRemovals.delete(key); return expiry !== undefined && expiry >= Date.now(); }
function randomNonce(): string { return [...crypto.getRandomValues(new Uint8Array(32))].map((byte) => byte.toString(16).padStart(2, "0")).join(""); }
function isUnlockPageMessage(value: unknown): value is { type: "unlock.status" | "unlock.start" } { const type = typeof value === "object" && value !== null ? (value as { type?: unknown }).type : undefined; return type === "unlock.status" || type === "unlock.start"; }
function isEnvelope(value: unknown): value is Envelope { const item = value as Partial<Envelope>; return typeof value === "object" && value !== null && typeof item.v === "number" && typeof item.conn_nonce === "string" && typeof item.seq === "number" && typeof item.id === "string" && typeof item.type === "string" && typeof item.payload === "object" && item.payload !== null; }
function requiredString(value: Record<string, unknown>, key: string): string { const item = value[key]; if (typeof item !== "string") throw new Error(`${key} must be string`); return item; }
function optionalString(value: Record<string, unknown>, key: string): string | undefined { const item = value[key]; if (item === null || item === undefined) return undefined; if (typeof item !== "string") throw new Error(`${key} must be string or null`); return item; }
function requiredNumber(value: Record<string, unknown>, key: string): number { const item = value[key]; if (typeof item !== "number" || !Number.isFinite(item)) throw new Error(`${key} must be number`); return item; }
function callbackPromise<T>(invoke: (done: (value: T) => void) => void): Promise<T> { return new Promise((resolve, reject) => invoke((value) => { const error = chrome.runtime.lastError; error === undefined ? resolve(value) : reject(new Error(error.message ?? "Chrome API failed")); })); }
function delay(milliseconds: number): Promise<void> { return new Promise((resolve) => setTimeout(resolve, milliseconds)); }
function unique(values: number[]): number[] { return [...new Set(values)]; }
function enqueue(task: () => Promise<void>): void { queue = queue.then(task, task).catch(() => console.error("FCP fail-closed controller error")); }

chrome.runtime.onStartup.addListener(() => { void connect(); });
void connect();
