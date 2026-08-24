import {
  HOST_NAME, MIN_HOST_VERSION, PROTOCOL_VERSION, REQUIRED_CAPABILITIES, categorizeCookieSetFailure, cookieBelongsToGroup, compareSemanticVersions,
  cookieIdentity, cookieRecord, cookieRoundTripMatches, cookieSetDetails, cookieUrl, groupForCookie, groupForUrl,
  guessScope, navigationPatterns, policyParameters, validateConfig,
  type AccountGroup, type AccountGroupsConfig, type CookieRecord, type CookieSetFailureCategory,
  type Envelope, type LoadedConfig, type PolicyLevel, type WireMessage,
} from "./protocol.js";
import { OrderedChunkAssembler, chunkRecords } from "./cookie-chunks.js";
import {
  addToBoundedOutbox, makeMonitorEvent, MonitorDeliveryWindow, notificationDecision, notificationText,
  validateMonitorEvent, type MonitorEvent, type MonitorSignal, type NotificationDecisionState,
} from "./monitor.js";
import { chainedEvictionAfterCompletion, decideStartup, shouldRetryReconciliation, stateAfterDisconnect, stateAfterHostError } from "./state-machine.js";
import { applyPlatform, currentLocale } from "./locale.js";
import type { Locale } from "./i18n.js";
import { ConnectionReadiness } from "./connection-readiness.js";
import { mayAbortWithEmptySnapshot, OperationCoordinator, type OperationReference } from "./operation-coordinator.js";
import { GuardedRemovalPlan } from "./guarded-removal.js";

const EXTENSION_VERSION = chrome.runtime.getManifest().version;

// Resolved once per service-worker lifetime; a restart re-resolves it from the same persisted
// storage value, same as loadedConfig re-adopts its cache. Notifications sent before this resolves
// (a narrow startup window) fall back to the default locale rather than waiting, since a toast
// unrelated to the config handshake must not be blocked on it.
let uiLocale: Locale = "tr";
void applyPlatform().then(() => currentLocale()).then((resolved) => { uiLocale = resolved; });

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
  reconciliationAttempts?: number;
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

interface RecoveryCandidate {
  profileId: string;
  displayName: string;
  browser: string;
  lastUsedUnixMs: number;
  siteCount: number;
}

const STATE_KEY = "fcp-runtime-v2";
const LEGACY_STATE_KEY = "fcp-mvp-runtime-v1";
const WIKIPEDIA_GROUP_ID = "7a144677-3f5c-4a86-a767-16fd3ca315b8";
const IDLE_BASE_SECONDS = 60;
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
const PROFILE_ID_KEY = "fcp-profile-id-v1";
const OPERATION_REFERENCE_KEY = "fcp-operation-reference-v1";

// Q24: the host is the config's single source of truth. The cache exists only so the extension
// can still evict fail-closed while the host is unreachable; the handshake always overwrites it.
let loadedConfig: LoadedConfig | undefined;
let configWaiters: Array<(value: LoadedConfig) => void> = [];
let recoveryCandidates: RecoveryCandidate[] = [];
// Optional, unlike REQUIRED_CAPABILITIES: a host that cannot observe processes is fully usable,
// it just must not be offered the monitor-only level. Assumed absent until a handshake says
// otherwise, so a stale value can never make the UI offer a level that would do nothing.
let hostSupportsMonitoring = false;
// Intentionally memory-only: a native port keeps the worker alive during transfer, while a
// restart abandons the lease and lets handshake reconciliation fail closed. Cookie values never
// enter extension storage merely to support chunk assembly.
const pendingInjectChunks = new OrderedChunkAssembler<CookieRecord>();

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
const removalSnapshots = new Map<string, chrome.cookies.Cookie[]>();
const operationCoordinator = new OperationCoordinator({
  load: () => getLocal(OPERATION_REFERENCE_KEY),
  save: (value) => value === undefined ? removeLocal([OPERATION_REFERENCE_KEY]) : setLocal(OPERATION_REFERENCE_KEY, value),
});
const monitorDelivery = new MonitorDeliveryWindow();

class RedactedCookieSetFailure extends Error {
  constructor(readonly category: CookieSetFailureCategory | "no_result") {
    super(`cookie_set_${category}`);
    this.name = "RedactedCookieSetFailure";
  }
}

chrome.idle.setDetectionInterval(IDLE_BASE_SECONDS);
chrome.alarms.create(MONITOR_POLL_ALARM, { periodInMinutes: 0.5 });
setInterval(() => enqueue(pollNativeMonitor), 15_000);

chrome.permissions.onAdded.addListener(() => enqueue(async () => {
  await flushPendingAdd();
  const loaded = loadedConfig;
  if (loaded === undefined) return;
  const root = await loadState(loaded);
  let changed = false;
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    if (state?.lastEvent === "permission_missing" && await hasScopePermission(group)) {
      state.lastEvent = "permission_restored";
      changed = true;
    }
  }
  if (changed) await saveState(root);
}));
chrome.permissions.onRemoved.addListener(() => enqueue(async () => {
  const loaded = loadedConfig;
  if (loaded === undefined) return;
  for (const group of loaded.config.groups) {
    if (!await hasScopePermission(group)) await markPermissionMissing(loaded, group);
  }
}));

chrome.webNavigation.onBeforeNavigate.addListener((details) => enqueue(async () => {
  if (details.frameId !== 0) return;
  // See the matching comment on chrome.tabs.onUpdated below — any page navigating (including the
  // very tab this extension itself opens) can fire before the first handshake ever completes.
  const loaded = loadedConfig;
  if (loaded === undefined) return;
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
  requestId?: string;
}

function isPendingAdd(value: unknown): value is PendingAdd {
  const pending = value as Partial<PendingAdd> | undefined;
  return typeof pending?.scope === "string" && typeof pending.displayName === "string" &&
    typeof pending.policyLevel === "string" && typeof pending.stagedAtUnixMs === "number";
}

type OperationStatus = "pending" | "success" | "error";
type ConfigOperation = { type: "group.add" | "group.remove" | "group.set_policy" | "profile.recover"; status: OperationStatus; scope?: string; groupId?: string; error?: string; updatedAt: number };
const configOperations = new Map<string, ConfigOperation>();

async function flushPendingAdd(): Promise<string | undefined> {
  const stored = await getLocal(PENDING_ADD_KEY);
  if (!isPendingAdd(stored)) return undefined;
  if (Date.now() - stored.stagedAtUnixMs > PENDING_ADD_TTL_MS) {
    await setLocal(PENDING_ADD_KEY, undefined);
    return undefined;
  }
  // Keep the intent if the host is not up yet; the handshake retries this.
  if (!client?.ready) { void connect(); return undefined; }
  const group = loadedConfig?.config.groups.find((item) => item.scope === stored.scope);
  if (group !== undefined) { await setLocal(PENDING_ADD_KEY, undefined); return stored.requestId; }
  if (stored.requestId !== undefined && configOperations.get(stored.requestId)?.status === "pending") return stored.requestId;
  const requestId = startConfigOperation("group.add", { scope: stored.scope, display_name: stored.displayName, policy_level: stored.policyLevel });
  await setLocal(PENDING_ADD_KEY, { ...stored, requestId });
  return requestId;
}

type PopupMessage =
  | { type: "popup.state"; url?: string }
  | { type: "popup.stageProtect"; scope: string; displayName: string; policyLevel: PolicyLevel }
  | { type: "popup.cancelProtect" }
  | { type: "popup.log" }
  | { type: "popup.clearLog" }
  | { type: "popup.operation"; requestId: string }
  | { type: "popup.protect"; scope: string; displayName: string; policyLevel: PolicyLevel }
  | { type: "popup.recover"; profileId: string }
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
      ok: true, connected: client?.ready === true, host,
      suggestedScope: guessScope(host), groups, recoveryCandidates, error, alert,
      supportsMonitoring: hostSupportsMonitoring,
    };
  }
  if (message.type === "popup.log") {
    const stored = await getLocal(ALERT_LOG_KEY);
    const log = Array.isArray(stored) ? stored.filter(isStoredAlert) : [];
    return { ok: true, log: [...log].reverse() };
  }
  if (message.type === "popup.clearLog") {
    await setLocal(ALERT_LOG_KEY, []);
    await setLocal(LAST_ALERT_KEY, undefined);
    await setBadge("", "#b3261e");
    return { ok: true };
  }
  if (message.type === "popup.operation") {
    const operation = configOperations.get(message.requestId);
    return operation === undefined ? { ok: false, error: "operation_state_lost" } : {
      ok: operation.status === "success", pending: operation.status === "pending", error: operation.error,
    };
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
  if (!client?.ready) { void connect(); return { ok: false, error: "native_host_not_connected" }; }
  if (message.type === "popup.recover") {
    if (!isProfileId(message.profileId) || !recoveryCandidates.some((candidate) => candidate.profileId === message.profileId)) {
      return { ok: false, error: "unknown_recovery_candidate" };
    }
    const requestId = send("recovery.adopt", { profile_id: message.profileId });
    configOperations.set(requestId, {
      type: "profile.recover", groupId: message.profileId, status: "pending", updatedAt: Date.now(),
    });
    return { ok: true, pending: true, requestId };
  }
  if (message.type === "popup.protect") {
    const requestId = await flushPendingAdd();
    return requestId === undefined ? { ok: false, error: "native_host_not_connected" } : { ok: true, pending: true, requestId };
  }
  if (message.type === "popup.setPolicy") {
    const requestId = startConfigOperation("group.set_policy", { account_group_id: message.groupId, policy_level: message.policyLevel });
    return { ok: true, pending: true, requestId };
  }
  const requestId = startConfigOperation("group.remove", { account_group_id: message.groupId });
  return { ok: true, pending: true, requestId };
}

async function popupGroupSummaries(loaded: LoadedConfig): Promise<Array<Record<string, unknown>>> {
  const root = await loadState(loaded);
  const summaries: Array<Record<string, unknown>> = [];
  for (const group of loaded.config.groups) {
    const hasPermission = await hasScopePermission(group);
    summaries.push({
      id: group.id, scope: group.scope, displayName: group.display_name,
      policyLevel: group.policy_level, state: root.groups[group.id]?.groupState ?? "uninitialized",
      hasPermission,
      // Never call chrome.cookies for a scope Chrome has not authorized: that rejects the whole
      // popup.state response and makes later groups appear to have vanished instead of showing
      // "permission required".
      cookieCount: hasPermission ? (await getCookies(group)).length : 0,
    });
  }
  return summaries;
}

function isPopupMessage(value: unknown): value is PopupMessage {
  const type = typeof value === "object" && value !== null ? (value as { type?: unknown }).type : undefined;
  return type === "popup.state" || type === "popup.protect" || type === "popup.unprotect" ||
    type === "popup.log" || type === "popup.clearLog" || type === "popup.operation" || type === "popup.stageProtect" ||
    type === "popup.cancelProtect" || type === "popup.setPolicy" || type === "popup.recover";
}

chrome.tabs.onUpdated.addListener((tabId, change, tab) => enqueue(async () => {
  if (change.status !== "complete" && change.url === undefined) return;
  // Not awaitConfig(): this can fire (a tab loading) before the very first handshake ever
  // completes, and awaiting here would deadlock the whole serialized queue behind itself — the
  // handshake response that would resolve it is queued right behind this same task. Nothing
  // useful to do yet if no config has ever been adopted; a later real event re-triggers this.
  const loaded = loadedConfig;
  if (loaded === undefined) return;
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
  // See the matching comment on chrome.tabs.onUpdated above.
  const loaded = loadedConfig;
  if (loaded === undefined) return;
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
  // See the matching comment on chrome.tabs.onUpdated above.
  const loaded = loadedConfig;
  if (loaded === undefined) return;
  const root = await loadState(loaded);
  if (idleState === "active") {
    for (const group of loaded.config.groups) chrome.alarms.clear(alarmName("idle", group.id));
    return;
  }
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    if (idleState === "locked") {
      if (client?.ready) send("auth.cache.clear", { account_group_id: group.id, reason: "locked" });
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
  // See the matching comment on chrome.tabs.onUpdated above.
  const loaded = loadedConfig;
  if (loaded === undefined) return;
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
  // See the matching comment on chrome.tabs.onUpdated above.
  const loaded = loadedConfig;
  if (loaded === undefined) return;
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
  await operationCoordinator.restore();
  if (client?.connected) return;
  let nextClient!: NativeClient;
  nextClient = new NativeClient(await getOrCreateProfileId(), loadedConfig?.digest, handleHostMessage, flushPendingAdd, async () => {
    if (client !== nextClient) return;
    monitorDelivery.resetConnection();
    // Re-learned from the next handshake. A host can be replaced by a different build across a
    // reconnect, so carrying the old answer forward could offer a level the new one cannot honour.
    hostSupportsMonitoring = false;
    // Scheduled first and unconditionally: awaitConfig() below only resolves once a config has
    // ever been adopted (cache or a successful handshake), which never happens if the very first
    // connection attempt in a profile fails before any config exists — gating the retry behind it
    // deadlocked the whole reconnect loop after exactly one failure, with nothing left to retry it.
    setTimeout(() => { void connect(); }, 1_000);
    // Also non-blocking, same reason: if the very first connection ever disconnects before its
    // own handshake completed, there is no config yet and nothing here to fail-closed evict.
    const loaded = loadedConfig;
    if (loaded === undefined) return;
    const root = await loadState(loaded);
    const activeGroups: string[] = [];
    const upgradeRequired = !nextClient.ready;
    if (upgradeRequired) lastConfigError = "upgrade_required";
    for (const group of loaded.config.groups) {
      const state = root.groups[group.id];
      if (state === undefined) continue;
      // A failed pre-Hello connection proves only that the host is unavailable/incompatible. It
      // does not prove that a sealed browser scope or its committed vault object is inconsistent.
      // Only an exposed active lease needs fail-closed cookie cleanup and reconciliation.
      const disconnected = stateAfterDisconnect(state.groupState);
      if (!disconnected.activeLease) continue;
      activeGroups.push(group.id);
      try { await removeAllCookies(group); } catch { /* continue fail-closed cleanup for other groups */ }
      state.groupState = disconnected.state;
      state.reconciliation = disconnected.reconciliation;
      state.lastEvent = "native_disconnect_fail_closed";
    }
    await saveState(root);
    if (activeGroups.length === 0) await queueMonitorEvent("host_disconnect", undefined, false);
    else for (const groupId of activeGroups) await queueMonitorEvent("host_disconnect_active_lease", groupId, false);
  });
  client = nextClient;
  nextClient.start();
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
    if (message.requestId !== undefined) {
      const operation = configOperations.get(message.requestId);
      if (operation !== undefined) {
        configOperations.set(message.requestId, { ...operation, status: "success", updatedAt: Date.now() });
        if (operation.type === "group.add") {
          const pending = await getLocal(PENDING_ADD_KEY);
          if (isPendingAdd(pending) && pending.requestId === message.requestId) await setLocal(PENDING_ADD_KEY, undefined);
        } else if (operation.type === "group.remove" && operation.scope !== undefined) {
          if (operation.groupId !== undefined) await operationCoordinator.discardGroup(operation.groupId);
          await removeUnusedScopePermission(operation.scope);
        }
      }
    }
    lastConfigError = undefined;
    return;
  }
  if (message.type === "config.rejected") {
    const reason = requiredString(message.payload, "reason");
    lastConfigError = reason;
    if (message.requestId !== undefined) {
      const operation = configOperations.get(message.requestId);
      if (operation !== undefined) configOperations.set(message.requestId, { ...operation, status: "error", error: reason, updatedAt: Date.now() });
      const pending = await getLocal(PENDING_ADD_KEY);
      if (isPendingAdd(pending) && pending.requestId === message.requestId) await setLocal(PENDING_ADD_KEY, undefined);
    }
    return;
  }
  if (message.type === "recovery.adopted") {
    if (message.requestId === undefined) throw new Error("recovery result lacks correlation");
    const operation = configOperations.get(message.requestId);
    const profileId = requiredString(message.payload, "profile_id");
    if (operation?.type !== "profile.recover" || operation.groupId !== profileId || !isProfileId(profileId)) {
      throw new Error("recovery result binding mismatch");
    }
    await setLocal(PROFILE_ID_KEY, profileId.toLowerCase());
    await removeLocal([CONFIG_CACHE_KEY, STATE_KEY, LEGACY_STATE_KEY, OPERATION_REFERENCE_KEY, PENDING_ADD_KEY]);
    loadedConfig = undefined;
    recoveryCandidates = [];
    configOperations.set(message.requestId, { ...operation, status: "success", updatedAt: Date.now() });
    // Let the initiating onboarding page observe success, then reconnect under the recovered
    // namespace. No domain or cookie data is copied through extension storage.
    setTimeout(() => client?.close(), 500);
    return;
  }
  if (message.type === "operation.error") {
    const requestId = requiredString(message.payload, "request_id");
    const code = requiredString(message.payload, "code");
    if (message.requestId !== requestId) throw new Error("operation error correlation mismatch");
    const operation = configOperations.get(requestId);
    if (operation !== undefined) configOperations.set(requestId, { ...operation, status: "error", error: code, updatedAt: Date.now() });
    const groupId = optionalString(message.payload, "account_group_id");
    const loaded = loadedConfig;
    const group = groupId === undefined ? undefined : loaded?.config.groups.find((item) => item.id === groupId);
    if (loaded !== undefined && group !== undefined) {
      const root = await loadState(loaded);
      const state = requiredGroupState(root, group.id);
      const pendingLease = state.pendingLeaseRequest;
      state.pendingLeaseRequest = undefined;
      state.evictionRequestPending = false;
      // Carry the host's own code to the unlock page. A navigation unlock happens on its own
      // document, so an error that only reaches the popup is an error nobody sees: the person is
      // looking at the unlock screen, waiting.
      if (state.navigationUnlockRequestTabId !== undefined) {
        state.navigationUnlockRequestTabId = undefined;
        state.navigationUnlockError = code;
      }
      const recovery = stateAfterHostError(state.groupState, pendingLease);
      state.groupState = recovery.state;
      state.reconciliation = recovery.reconciliation;
      state.lastEvent = `host_error:${code}`;
      await saveState(root);
    }
    lastConfigError = code;
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
      {
      const purpose = state.pendingLeaseRequest;
      state.leaseId = requiredString(message.payload, "lease_id");
      chrome.alarms.create(alarmName("expiry", group.id), { when: requiredNumber(message.payload, "expiry_unix_ms") });
      state.pendingLeaseRequest = undefined;
      state.lastEvent = "lease_grant";
      await saveState(root);
      if (purpose === "enroll") {
        const operation = await operationCoordinator.begin(group.id, state.leaseId, "enrollment");
        state.evictionRequestPending = true;
        state.groupState = "evicting";
        await saveState(root);
        sendOperationBegin(operation, "initial_enrollment");
      }
      break;
      }
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
          // The deny reason is the host's, not a guess: "the user refused" and "the vault is busy"
          // are different things to be told.
          state.navigationUnlockError = requiredString(message.payload, "reason");
        }
      }
      state.lastEvent = `lease_deny:${requiredString(message.payload, "reason")}`;
      await saveState(root);
      if (deniedEviction) await queueMonitorEvent("reconciliation_failed", group.id);
      break;
    }
    case "cookies.inject.chunk": await receiveInjectChunk(loaded, group, message.payload, root, state); break;
    case "evict.request":
      if (requiredString(message.payload, "phase") !== "snapshot_required") throw new Error("unsupported eviction phase");
      state.groupState = "evicting";
      state.evictionRequestPending = true;
      state.lastEvent = `snapshot_required:${requiredString(message.payload, "reason")}`;
      await saveState(root);
      sendCookieSnapshotChunks(
        group.id,
        optionalString(message.payload, "lease_id"),
        requiredString(message.payload, "operation_id"),
        (await getCookies(group)).map(cookieRecord),
      );
      break;
    case "evict.confirmed":
      if (message.payload.operation_sequence !== undefined) await handleEvictConfirmedV7(group, message.payload, root, state);
      else await finishEviction(loaded, group, message.payload, root, state);
      break;
    case "operation.snapshot_required": await handleOperationSnapshotRequired(group, message.payload, root, state); break;
    case "operation.status": await handleOperationStatus(group, message.payload, root, state); break;
    case "evict.remove.authorized": await handleRemovalAuthorized(group, message.payload, root, state); break;
    case "operation.completed": await handleOperationCompleted(loaded, group, message.payload, root, state); break;
    case "session.invalidated":
      clearGroupAlarms(group.id);
      try { await removeAllCookies(group); } catch { state.lastEvent = "session_invalidated_cleanup_failed"; }
      await operationCoordinator.discardGroup(group.id);
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
    const hasPermission = await hasScopePermission(group);
    state.relevantTabs = hasPermission ? await relevantTabIds(group) : [];
    state.lastEvent = hasPermission ? "config_updated" : "permission_missing";
  }
  await saveState(root);
}

async function handleHandshakeAck(payload: Record<string, unknown>): Promise<void> {
  if (requiredNumber(payload, "protocol_version") !== PROTOCOL_VERSION) throw new Error("host protocol mismatch");
  const hostVersion = requiredString(payload, "host_version");
  if (compareSemanticVersions(hostVersion, MIN_HOST_VERSION) < 0) throw new Error("native host version is too old");
  if (compareSemanticVersions(EXTENSION_VERSION, requiredString(payload, "min_extension_version")) < 0) throw new Error("extension version is too old for native host");
  const hostCapabilities = payload.capabilities;
  if (!Array.isArray(hostCapabilities) || !REQUIRED_CAPABILITIES.every((capability) => hostCapabilities.includes(capability))) {
    throw new Error("native host capability mismatch");
  }
  hostSupportsMonitoring = hostCapabilities.includes("process_monitoring");
  if (!Array.isArray(payload.recovery_candidates)) throw new Error("handshake recovery candidates must be an array");
  recoveryCandidates = payload.recovery_candidates.map(parseRecoveryCandidate);
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
    const hasPermission = await hasScopePermission(group);
    state.groupState = summary.group_state;
    state.leaseId = summary.lease_id ?? undefined;
    state.relevantTabs = hasPermission ? await relevantTabIds(group) : [];
    state.pendingLeaseRequest = undefined;
    state.evictionRequestPending = false;
    state.reconciliation = summary.reconciliation_required;
    state.injectAfterReconciliation = summary.reconciliation_required && state.relevantTabs.length > 0;
    state.lastEvent = hasPermission ? "handshake_ack" : "permission_missing";
    if (state.groupState === "uninitialized") {
      state.invalidationPending = false;
      state.invalidationReason = undefined;
    }
    // Keep the host's authoritative group and vault state, but do not infer an empty cookie jar or
    // run reconciliation without Chrome access. The permission-repair button can resume normal
    // observation later; hiding/removing the group would be a data-loss illusion.
    if (!hasPermission) continue;
    const cookies = await getCookies(group);
    const startup = decideStartup({
      state: state.groupState,
      cookieCount: cookies.length,
      relevantTabCount: state.relevantTabs.length,
      leaseExpiry: summary.lease_expiry_unix_ms,
      reconciliationRequired: summary.reconciliation_required,
      pendingInvalidation: pendingInvalidation !== undefined,
      now: Date.now(),
    });
    if (startup.action === "invalidate" && pendingInvalidation !== undefined) {
      actions.push(() => requestSessionInvalidation(loaded, group, pendingInvalidation));
    } else if (startup.action === "evict") {
      actions.push(() => requestEviction(loaded, group, startup.reason, state.leaseId));
    } else if (startup.action === "schedule_expiry") {
      chrome.alarms.create(alarmName("expiry", group.id), { when: startup.when });
    } else if (startup.action === "clean_sealed") {
      // A cookie event can race the first handshake/service-worker wake and be observed before a
      // config is available. `sealed` is a stronger invariant than tab presence: the browser store
      // must be empty even when a protected tab is already open. Clean first; the open tab is sent
      // through the ordinary unlock gate by the reconciliation/navigation flow afterwards.
      actions.push(async () => {
        await removeAllCookies(group);
        const remaining = (await getCookies(group)).length;
        if (remaining !== 0) {
          const latestRoot = await loadState(loaded);
          const latestState = requiredGroupState(latestRoot, group.id);
          latestState.groupState = "degraded";
          latestState.reconciliation = true;
          latestState.lastEvent = "sealed_startup_cleanup_failed";
          await saveState(latestRoot);
          await queueMonitorEvent("reconciliation_failed", group.id);
        } else if (state.relevantTabs.length > 0) {
          const tabId = state.relevantTabs[0];
          if (tabId !== undefined) await offerUnlockOnTab(loaded, group, tabId);
        }
      });
    }
  }
  await saveState(root);
  // A native port exists before the host has authenticated the handshake. Only expose it to
  // mutation/monitoring code after every ACK field and group summary has been validated and the
  // authoritative state has been adopted. UI messages are serialized behind this handler, so
  // they cannot observe a half-applied handshake.
  client?.markHandshakeReady();
  for (const statusQuery of operationCoordinator.statusQueries()) send("operation.status.query", statusQuery);
  for (const action of actions) await action();
  const pending = await monitorOutbox();
  if (pending.some((event) => event.signal === "host_disconnect" || event.signal === "host_disconnect_active_lease")) {
    await queueMonitorEvent("reconnect_success", undefined, false);
  }
  await flushMonitorOutbox();
  await pollNativeMonitor();
  await flushPendingAdd();
}

function parseRecoveryCandidate(value: unknown): RecoveryCandidate {
  if (typeof value !== "object" || value === null) throw new Error("invalid recovery candidate");
  const raw = value as Record<string, unknown>;
  const profileId = requiredString(raw, "profile_id");
  const displayName = requiredString(raw, "display_name");
  const browser = requiredString(raw, "browser");
  const lastUsedUnixMs = requiredNumber(raw, "last_used_unix_ms");
  const siteCount = requiredNumber(raw, "site_count");
  if (!isProfileId(profileId) || displayName.length > 80 || browser.length > 40 ||
      !Number.isSafeInteger(lastUsedUnixMs) || lastUsedUnixMs < 0 ||
      !Number.isSafeInteger(siteCount) || siteCount < 1 || siteCount > 32) {
    throw new Error("invalid recovery candidate fields");
  }
  return { profileId: profileId.toLowerCase(), displayName, browser, lastUsedUnixMs, siteCount };
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
    // immediate getAll() can still occasionally miss or briefly expose a normalized version of
    // the last write. Real sites with larger, frequently rotated cookie sets make that window
    // wider than a few hundred milliseconds. Re-apply only the records that did not round-trip,
    // with a short bounded backoff, while keeping the exact-value comparison as the final gate.
    // A persistent mismatch still fails closed; this only prevents a successful second attempt
    // from flashing an error and asking for another Hello gesture a few seconds later.
    //
    // The check is a subset check (every vaulted cookie landed), not set equality: a site's own
    // page script can set additional cookies (e.g. YouTube's `GPS`) in the same window we are
    // restoring into, and that is not a restore failure — only a vaulted cookie failing to land
    // is.
    const roundtripRetryDelaysMs = [0, 150, 350, 750, 1_500];
    let roundtripMatched = false;
    let missing: CookieRecord[] = [];
    for (let attempt = 0; attempt < roundtripRetryDelaysMs.length && !roundtripMatched; attempt += 1) {
      if (attempt > 0) {
        await new Promise((resolve) => setTimeout(resolve, roundtripRetryDelaysMs[attempt]));
        mutatingGroups.add(group.id);
        try {
          for (const cookie of missing) await setCookie(group, cookie);
        } finally { mutatingGroups.delete(group.id); }
      }
      const actualByIdentity = new Map((await getCookies(group)).map((cookie) => [cookieIdentity(cookie), cookie]));
      missing = expectedRecords.filter((record) => {
        const actual = actualByIdentity.get(cookieIdentity(record));
        return actual === undefined || !cookieRoundTripMatches(record, actual);
      });
      roundtripMatched = missing.length === 0;
    }
    if (!roundtripMatched) {
      health = "cookie_roundtrip_failed";
      console.error(`FCP cookie roundtrip mismatch count=${missing.length}`);
    } else {
      // ADR-020: a verified round-trip is the whole success condition. No site-specific health
      // check runs, so the extension never has to decide whether a restored session is "real".
      const navigationTabId = state.navigationUnlockRequestTabId;
      const navigationTarget = navigationTabId === undefined ? undefined : state.pendingNavigationUnlocks?.[String(navigationTabId)];
      if (navigationTabId !== undefined && navigationTarget !== undefined) {
        stage = "navigation_gate_redirect";
        await updateTab(navigationTabId, navigationTarget);
        // Acceptance by chrome.tabs.update is the gate's success boundary. Waiting for a fully
        // loaded in-scope URL incorrectly turns legitimate login redirects, slow pages or later
        // user navigation into cookie-injection failures after round-trip verification passed.
        // tabs.onUpdated owns the eventual relevant-tab bookkeeping.
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
    // Keep the successful tab-to-target mapping until the unlock document has actually left.
    // Its final 300 ms status poll can otherwise land after this state change but before Chrome
    // commits the navigation, briefly rendering a false red error on a successful unlock.
    state.navigationUnlockRequestTabId = undefined;
    state.navigationUnlockError = undefined;
    for (const [tabId, target] of remaining) if (groupForUrl(loaded.config, target)?.id === group.id) void updateTab(Number(tabId), target).catch(() => undefined);
    scheduleNavigationContextCleanup(loaded, group.id);
  } else if (!success && state.navigationUnlockRequestTabId !== undefined) {
    state.navigationUnlockRequestTabId = undefined;
    state.navigationUnlockError = `inject_failed:${health}`;
  }
  state.lastEvent = `inject:${health}`;
  await saveState(root);
  send("inject.result", { account_group_id: group.id, lease_id: leaseId, success, health_check: health });
  if (!success) setTimeout(() => enqueue(() => requestEviction(loaded, group, "startup_reconciliation", leaseId)), 100);
}

function scheduleNavigationContextCleanup(loaded: LoadedConfig, groupId: string): void {
  setTimeout(() => enqueue(async () => {
    const root = await loadState(loaded);
    const state = root.groups[groupId];
    if (state?.pendingNavigationUnlocks === undefined) return;
    let changed = false;
    for (const tabIdText of Object.keys(state.pendingNavigationUnlocks)) {
      const tabId = Number(tabIdText);
      try {
        const tab = await callbackPromise<chrome.tabs.Tab>((done) => chrome.tabs.get(tabId, done));
        if (tab.url === UNLOCK_PAGE_URL) continue;
      } catch { /* a closed tab cannot still consume its unlock context */ }
      delete state.pendingNavigationUnlocks[tabIdText];
      changed = true;
    }
    if (changed) await saveState(root);
  }), 2_000);
}

async function receiveInjectChunk(loaded: LoadedConfig, group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const leaseId = requiredString(payload, "lease_id");
  const chunkIndex = requiredInteger(payload, "chunk_index");
  const chunkCount = requiredInteger(payload, "chunk_count");
  const cookieCount = requiredInteger(payload, "cookie_count");
  if (!Array.isArray(payload.cookies)) throw new Error("cookies.inject.chunk cookies must be an array");
  const records = pendingInjectChunks.receive(
    group.id, leaseId, chunkIndex, chunkCount, cookieCount, payload.cookies as CookieRecord[],
  );
  if (records === undefined) return;
  await injectCookies(loaded, group, { lease_id: leaseId, cookies: records }, root, state);
}

function sendCookieSnapshotChunks(groupId: string, leaseId: string | undefined, operationId: string, cookies: CookieRecord[]): void {
  const chunks = chunkRecords(cookies);
  for (let chunkIndex = 0; chunkIndex < chunks.length; chunkIndex += 1) {
    send("cookies.snapshot.chunk", {
      account_group_id: groupId,
      lease_id: leaseId,
      operation_id: operationId,
      chunk_index: chunkIndex,
      chunk_count: chunks.length,
      cookie_count: cookies.length,
      cookies: chunks[chunkIndex],
    });
  }
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
  const recoveryQuery = operationCoordinator.recoveryQuery(group.id);
  if (recoveryQuery !== undefined) {
    // Startup/browser triggers are observations, not authority to supersede an already-issued
    // durable operation. Resume it by identity; its semantic status determines the next action.
    state.evictionRequestPending = true;
    state.groupState = "evicting";
    state.lastEvent = `operation_recovery_pending:${reason}`;
    await saveState(root);
    send("operation.status.query", recoveryQuery);
    return;
  }
  state.evictionRequestPending = true;
  state.groupState = "evicting";
  state.lastEvent = `eviction_request_pending:${reason}`;
  await saveState(root);
  const kind = reason === "startup_reconciliation" || reason === "degraded_cookie_detected" ? "reconciliation" : "eviction";
  const operation = await operationCoordinator.begin(group.id, leaseId, kind);
  sendOperationBegin(operation, reason);
}

function sendOperationBegin(operation: OperationReference, reason: string, retry = 0): void {
  send("operation.begin", { account_group_id: operation.groupId, lease_id: operation.leaseId ?? null,
    attempt_id: operation.attemptId, kind: operation.kind, reason });
  // Native messaging delivery and service-worker scheduling have separate completion boundaries.
  // Until snapshot_required durably binds the host identity, repeat the exact idempotent begin.
  // The host returns the same issued identity for this attempt. A bounded failure closes the port
  // so startup recovery can classify the journal instead of leaving Evicting alive indefinitely.
  const delayMs = 750 * (retry + 1);
  setTimeout(() => enqueue(async () => {
    const current = operationCoordinator.current(operation.groupId);
    if (current?.attemptId !== operation.attemptId || current.phase !== "begin_pending") return;
    if (!client?.ready) return;
    if (retry < 2) sendOperationBegin(current, reason, retry + 1);
    else client.close();
  }), delayMs);
}

function sendV7Snapshot(operation: OperationReference, purpose: "commit" | "removal_precheck", cookies: chrome.cookies.Cookie[]): void {
  if (operation.operationId === undefined || operation.operationSequence === undefined) throw new Error("operation is not host-issued");
  const records = cookies.map(cookieRecord);
  const chunks = chunkRecords(records);
  for (let chunkIndex = 0; chunkIndex < chunks.length; chunkIndex += 1) {
    send("cookies.snapshot.chunk", { account_group_id: operation.groupId,
      operation_id: operation.operationId, operation_sequence: operation.operationSequence,
      lease_id: operation.leaseId ?? null, attempt_id: operation.attemptId, purpose,
      chunk_index: chunkIndex, chunk_count: chunks.length, cookie_count: records.length,
      cookies: chunks[chunkIndex] });
  }
}

async function handleOperationSnapshotRequired(group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  let operation: OperationReference;
  try {
    operation = await operationCoordinator.bindIssued(group.id, requiredString(payload, "operation_id"),
      requiredInteger(payload, "operation_sequence"), optionalString(payload, "lease_id"), requiredString(payload, "attempt_id"));
  } catch (error: unknown) {
    // The host has durably issued an operation but the extension could not durably bind it. The
    // only safe recovery is to end this connection: host startup recovery aborts a snapshot-less
    // NotCommitted operation, instead of leaving the group Evicting forever on a live port.
    client?.close();
    throw error;
  }
  try {
    sendV7Snapshot(operation, "commit", await getCookies(group));
    state.lastEvent = "v7_snapshot_required";
  } catch (error: unknown) {
    if (!mayAbortWithEmptySnapshot(operation.kind)) throw error;
    // Enrollment has no authoritative vault object yet. If Chrome refuses the observation (most
    // commonly a permission race), finish through the host's durable scope-empty abort instead
    // of marooning a NotCommitted journal record and an Evicting lease forever.
    sendV7Snapshot(operation, "commit", []);
    state.lastEvent = "enrollment_snapshot_unavailable";
  }
  // Host messages are serialized behind this handler, so the response cannot overtake this
  // projection write. Sending first prevents a repairable Chrome storage failure from blocking
  // the authoritative snapshot/abort message forever.
  state.groupState = "evicting";
  state.evictionRequestPending = true;
  await saveState(root);
}

async function prepareGuardedRemoval(group: AccountGroup, payload: Record<string, unknown>): Promise<void> {
  const operation = operationCoordinator.assertBinding(payload);
  await operationCoordinator.phase(group.id, "removal_precheck");
  const fresh = await getCookies(group);
  removalSnapshots.set(group.id, fresh);
  send("evict.remove.prepare", { account_group_id: group.id, operation_id: operation.operationId,
    operation_sequence: operation.operationSequence, lease_id: operation.leaseId ?? null,
    attempt_id: operation.attemptId });
  sendV7Snapshot(operation, "removal_precheck", fresh);
}

async function handleEvictConfirmedV7(group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const operation = operationCoordinator.assertBinding(payload);
  const disposition = requiredString(payload, "cookie_disposition");
  await operationCoordinator.phase(group.id, "committed");
  if (disposition === "retain_leased") {
    const remaining = (await getCookies(group)).length;
    send("evict.result", { account_group_id: group.id, operation_id: operation.operationId,
      operation_sequence: operation.operationSequence, lease_id: operation.leaseId ?? null,
      attempt_id: operation.attemptId, success: remaining > 0, remaining_cookie_count: remaining });
    state.lastEvent = remaining > 0 ? "enrollment_retained_leased" : "enrollment_cookie_missing";
    await saveState(root);
    return;
  }
  if (disposition !== "remove") throw new Error("unsupported v7 cookie disposition");
  await prepareGuardedRemoval(group, payload);
}

async function handleRemovalAuthorized(group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const operation = operationCoordinator.assertBinding(payload);
  const authorized = removalSnapshots.get(group.id);
  if (authorized === undefined) throw new Error("authorized snapshot is unavailable after worker restart");
  await operationCoordinator.phase(group.id, "removal_authorized");
  let success = true;
  const plan = new GuardedRemovalPlan(authorized.map(cookieRecord));
  for (;;) {
    const currentCookies = await getCookies(group);
    const step = plan.next(currentCookies.map(cookieRecord));
    if ("mutation" in step) { success = false; break; }
    if (step.done) break;
    const actual = currentCookies.find((cookie) => cookieIdentity(cookie) === cookieIdentity(step.record));
    if (actual === undefined || !cookieRoundTripMatches(step.record, actual)) { success = false; break; }
    await removeCookie(group, actual);
    if ((await getCookies(group)).some((cookie) => cookieIdentity(cookie) === cookieIdentity(step.record))) { success = false; break; }
  }
  const remaining = (await getCookies(group)).length;
  success = success && remaining === 0;
  send("evict.result", { account_group_id: group.id, operation_id: operation.operationId,
    operation_sequence: operation.operationSequence, lease_id: operation.leaseId ?? null,
    attempt_id: operation.attemptId, success, remaining_cookie_count: remaining });
  removalSnapshots.delete(group.id);
  state.groupState = success ? "sealed" : "degraded";
  state.reconciliation = !success;
  state.lastEvent = success ? "eviction_complete" : "cookie_rotation_reconciliation_required";
  await saveState(root);
}

async function handleOperationStatus(group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const operation = operationCoordinator.assertBinding(payload);
  const action = requiredString(payload, "required_action");
  if (action === "send_snapshot") sendV7Snapshot(operation, "commit", await getCookies(group));
  else if (action === "prepare_removal" || action === "verify_browser_state") await prepareGuardedRemoval(group, payload);
  else if (action === "completed") await operationCoordinator.complete(group.id);
  else if (action === "reconciliation_required") {
    await operationCoordinator.phase(group.id, "reconciliation_required");
    state.groupState = "degraded"; state.reconciliation = true; state.lastEvent = "operation_reconciliation_required";
    await saveState(root);
  } else if (action === "classify_durability") {
    setTimeout(() => { const query = operationCoordinator.statusQuery(group.id); if (query !== undefined && client?.ready) send("operation.status.query", query); }, 250);
  } else throw new Error("unsupported operation status action");
}

async function handleOperationCompleted(loaded: LoadedConfig, group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const operation = operationCoordinator.assertBinding(payload);
  const success = payload.success === true;
  const chained = chainedEvictionAfterCompletion(operation.kind, success, state.evictAfterEnrollment);
  if (success) {
    state.groupState = operation.kind === "enrollment" ? "leased" : "sealed";
    state.reconciliation = false;
    state.evictionRequestPending = false;
    if (operation.kind !== "enrollment") state.leaseId = undefined;
    state.lastEvent = "operation_completed";
    state.evictAfterEnrollment = undefined;
    state.reconciliationAttempts = 0;
    await operationCoordinator.complete(group.id);
  } else {
    state.groupState = "degraded"; state.reconciliation = true;
    state.evictionRequestPending = false;
    state.reconciliationAttempts = (state.reconciliationAttempts ?? 0) + 1;
    state.lastEvent = "operation_failed_reconciliation_required";
    await operationCoordinator.phase(group.id, "reconciliation_required");
  }
  await saveState(root);
  // Enrollment retains the just-captured browser cookies. If it was initiated because the user
  // had already closed/left the site, immediately continue with the original eviction trigger
  // after the enrollment operation has been durably completed and removed from local authority.
  if (chained !== undefined) await requestEviction(loaded, group, chained, state.leaseId);
  else if (shouldRetryReconciliation(success, state.reconciliationAttempts ?? 0)) {
    // Cookie-heavy sites can rotate/expire a cookie between the authoritative vault commit and
    // the fresh browser-removal precheck. The host correctly rejects that stale snapshot. Retry
    // immediately with a new durable reconciliation instead of exposing this expected race as a
    // lasting user-visible degraded state. The bound prevents a continuously mutating scope from
    // spinning forever; after two retries it remains fail-closed and visibly degraded.
    await requestEviction(loaded, group, "degraded_cookie_detected", state.leaseId);
  }
}

async function markPermissionMissing(loaded: LoadedConfig, group: AccountGroup): Promise<void> {
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.lastEvent === "permission_missing") return;
  state.lastEvent = "permission_missing";
  await saveState(root);
  await handleMonitorAlert(makeMonitorEvent("permission_missing", group.id));
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
  if (!client?.ready) { void connect(); return; }
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
  if (match === undefined) {
    try {
      const tab = await callbackPromise<chrome.tabs.Tab>((done) => chrome.tabs.get(tabId, done));
      if (tab.url !== UNLOCK_PAGE_URL) return { ok: true, status: "redirecting" };
    } catch { /* a closed tab has no recoverable navigation context */ }
    return { ok: false, status: "error", error: "navigation_context_missing" };
  }
  if (groupForUrl(loaded.config, match.target)?.id !== match.group.id) return { ok: false, status: "error", error: "navigation_context_invalid" };
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
  if (!client?.ready) { void connect(); return { ok: false, status: "error", error: "native_host_not_connected" }; }
  state.navigationUnlockRequestTabId = tabId;
  state.navigationUnlockError = undefined;
  state.lastEvent = "navigation_unlock_user_gesture";
  await saveState(root);
  await requestLease(loaded, group, "inject");
  return { ok: true, status: "unlocking" };
}

class NativeClient {
  private port?: chrome.runtime.Port;
  private disconnectHandled = false;
  private readonly readiness = new ConnectionReadiness();
  private readonly nonce = randomNonce();
  private outgoing = 0;
  private incoming = 0;
  constructor(
    private readonly profileId: string,
    private readonly cachedConfigDigest: string | undefined,
    private readonly onMessage: (message: WireMessage) => Promise<void>,
    private readonly onReady: () => Promise<unknown>,
    private readonly onDisconnect: () => Promise<void>,
  ) {}
  get connected(): boolean { return this.readiness.connected; }
  get ready(): boolean { return this.readiness.ready; }
  markHandshakeReady(): void {
    this.readiness.accepted();
  }
  close(): void { this.port?.disconnect(); }
  start(): void {
    const port = chrome.runtime.connectNative(HOST_NAME);
    this.port = port;
    this.readiness.opened();
    port.onMessage.addListener((raw) => enqueue(() => this.receive(raw)));
    port.onDisconnect.addListener(() => {
      // Chrome reports native-host startup/protocol failures through runtime.lastError. Merely
      // reading it acknowledges the error and prevents an "Unchecked runtime.lastError" console
      // flood; state recovery remains centralized in onDisconnect and exposes no provider text.
      void chrome.runtime.lastError?.message;
      this.handlePortDisconnect();
    });
    this.send("handshake", {
      protocol_version: PROTOCOL_VERSION,
      extension_id: chrome.runtime.id,
      profile_id: this.profileId,
      extension_version: EXTENSION_VERSION,
      min_host_version: MIN_HOST_VERSION,
      capabilities: [...REQUIRED_CAPABILITIES],
      cached_config_digest: this.cachedConfigDigest ?? null,
    });
  }
  send(type: string, payload: Record<string, unknown>): string {
    const port = this.port;
    if (port === undefined) throw new Error("native host is not connected");
    this.outgoing += 1;
    const requestId = crypto.randomUUID();
    try {
      port.postMessage({ v: PROTOCOL_VERSION, conn_nonce: this.nonce, seq: this.outgoing, id: requestId, type, payload });
    } catch (error: unknown) {
      // The port can die between the connected check and this write (a service-worker restart
      // after a permission grant does exactly that). Drop it here so the reconnect path runs
      // instead of repeatedly writing into a dead port.
      this.handlePortDisconnect();
      throw new Error("native host disconnected before write completed");
    }
    return requestId;
  }
  trySend(type: string, payload: Record<string, unknown>): boolean {
    if (!this.ready) return false;
    try {
      this.send(type, payload);
      return true;
    } catch {
      // Best-effort monitor traffic must not turn an already-detected disconnect into a global
      // controller failure. handlePortDisconnect has scheduled the ordinary reconnect path and
      // the durable monitor outbox remains available for that connection.
      return false;
    }
  }
  private handlePortDisconnect(): void {
    if (this.disconnectHandled) return;
    this.disconnectHandled = true;
    this.port = undefined;
    this.readiness.closed();
    enqueue(this.onDisconnect);
  }
  private async receive(raw: unknown): Promise<void> {
    if (!isEnvelope(raw)) throw new Error("malformed native envelope");
    if (raw.v !== PROTOCOL_VERSION || raw.conn_nonce !== this.nonce || raw.seq !== this.incoming + 1) throw new Error("native nonce/sequence validation failed");
    this.incoming = raw.seq;
    await this.onMessage({ type: raw.type, payload: raw.payload, requestId: raw.id });
    if (raw.type === "handshake.ack") {
      if (!this.ready) throw new Error("handshake ACK was not accepted");
      await this.onReady();
    }
  }
}

async function getOrCreateProfileId(): Promise<string> {
  const stored = await getLocal(PROFILE_ID_KEY);
  if (typeof stored === "string" && isProfileId(stored)) return stored.toLowerCase();
  const created = crypto.randomUUID();
  await setLocal(PROFILE_ID_KEY, created);
  return created;
}

function isProfileId(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
}

function send(type: string, payload: Record<string, unknown>): string {
  if (!client?.ready) throw new Error("native host is not ready");
  return client.send(type, payload);
}

function startConfigOperation(type: "group.add" | "group.remove" | "group.set_policy", payload: Record<string, unknown>): string {
  const groupId = typeof payload.account_group_id === "string" ? payload.account_group_id : undefined;
  const scope = type === "group.remove" && groupId !== undefined
    ? loadedConfig?.config.groups.find((group) => group.id === groupId)?.scope
    : undefined;
  const requestId = send(type, payload);
  configOperations.set(requestId, { type, scope, groupId, status: "pending", updatedAt: Date.now() });
  return requestId;
}

async function removeUnusedScopePermission(scope: string): Promise<void> {
  const origins = [`*://${scope}/*`, `*://*.${scope}/*`];
  const stillRequired = loadedConfig?.config.groups.some((group) =>
    navigationPatterns(group).some((pattern) => origins.includes(pattern))) === true;
  if (stillRequired) return;
  await new Promise<void>((resolve) => {
    chrome.permissions.remove({ origins }, () => {
      void chrome.runtime.lastError?.message;
      resolve();
    });
  });
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
  if (!client?.ready) return;
  for (const event of monitorDelivery.takeUnsent(await monitorOutbox())) {
    if (!client?.trySend("monitor.event", { ...event })) break;
  }
}

async function pollNativeMonitor(): Promise<void> {
  if (!client?.ready) return;
  await flushMonitorOutbox();
  client?.trySend("monitor.poll", { max_events: 32 });
}

async function handleMonitorAlert(value: unknown): Promise<void> {
  if (!validateMonitorEvent(value)) throw new Error("invalid monitor alert");
  const event = value;
  if (event.source === "extension") {
    monitorDelivery.acknowledge(event.event_id);
    const pending = await monitorOutbox();
    await setLocal(MONITOR_OUTBOX_KEY, pending.filter((item) => item.event_id !== event.event_id));
  }
  if (event.severity === "info") return;
  // A process-wide signal like remote debugging carries no account_group_id, so "what's at risk"
  // is answered from current lease state at recording time rather than something the event
  // itself would ever carry. Computed once and shared by the log entry and the notification text
  // so both agree on the same snapshot.
  const affectedScopes = event.account_group_id === undefined || event.account_group_id === null
    ? await currentlyLeasedScopes()
    : [];
  // The rate limit exists to stop notification spam, not to hide events. The toast is the noisy,
  // interrupting channel and stays limited; the badge and the popup record are cheap and are
  // always refreshed, so an event that arrives right after the user acknowledged the previous one
  // still leaves a visible trace instead of passing silently.
  await recordAlert(event, affectedScopes);
  const previousValue = await getLocal(MONITOR_RATE_KEY);
  const previous = isNotificationDecisionState(previousValue) ? previousValue : {};
  const decision = notificationDecision(event, previous, Date.now());
  if (!decision.show) return;
  await setLocal(MONITOR_RATE_KEY, decision.next);
  const content = notificationText(event, affectedScopes, uiLocale);
  await createNotification(`fcp-monitor-${event.event_id}`, {
    type: "basic", iconUrl: MONITOR_ICON_URL, title: content.title, message: content.message,
    priority: event.severity === "high" ? 2 : 1,
  });
}

// A process-wide signal like remote debugging isn't tied to any one group, so "what's at risk"
// is answered from current lease state at display time rather than carried on the event itself.
async function currentlyLeasedScopes(): Promise<string[]> {
  const loaded = await awaitConfig();
  const root = await loadState(loaded);
  return loaded.config.groups
    .filter((group) => root.groups[group.id]?.groupState === "leased")
    .map((group) => group.scope);
}

async function recordAlert(event: MonitorEvent, affectedScopes: readonly string[]): Promise<void> {
  const storedValue = await getLocal(LAST_ALERT_KEY);
  const stored = isStoredAlert(storedValue) ? storedValue : undefined;
  const sameAsStored = stored?.signal === event.signal && stored.accountGroupId === (event.account_group_id ?? null);
  const entry: StoredAlert = {
    signal: event.signal, severity: event.severity,
    accountGroupId: event.account_group_id ?? null, observedAtUnixMs: event.observed_at_unix_ms,
    occurrences: sameAsStored ? stored.occurrences + 1 : 1,
    affectedScopes: affectedScopes.length > 0 ? [...affectedScopes] : undefined,
  };
  await setLocal(LAST_ALERT_KEY, entry);
  await appendAlertLog(entry);
  await setBadge("!", "#b3261e");
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
  // Only set for process-wide signals (no accountGroupId): which sites were leased at the time.
  affectedScopes?: string[];
}

function isStoredAlert(value: unknown): value is StoredAlert {
  const alert = value as Partial<StoredAlert> | undefined;
  return typeof alert?.signal === "string" && typeof alert.severity === "string" &&
    typeof alert.observedAtUnixMs === "number" && typeof alert.occurrences === "number" &&
    (alert.affectedScopes === undefined || (Array.isArray(alert.affectedScopes) && alert.affectedScopes.every((scope) => typeof scope === "string")));
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
  return [...uniqueCookies.values()].sort((left, right) => cookieIdentity(left).localeCompare(cookieIdentity(right)));
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
  return tabs.flatMap((tab) => tab.id !== undefined && groupForUrl({ version: 3, compatibility_version: 3, groups: [group] }, tab.url) !== undefined ? [tab.id] : []);
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

// Critical transition intent must survive an MV3 worker stop, browser restart, permission grant,
// and extension reload. This state contains no cookie values; secrets remain only in the native
// vault or the browser cookie store, so durable local storage is safe for the controller metadata.
function saveState(state: RuntimeState): Promise<void> { return callbackPromise<void>((done) => chrome.storage.local.set({ [STATE_KEY]: state }, () => done())); }
function storageGet(key: string): Promise<Record<string, unknown>> { return callbackPromise((done) => chrome.storage.local.get(key, done)); }
async function getLocal(key: string): Promise<unknown> { return (await callbackPromise<Record<string, unknown>>((done) => chrome.storage.local.get(key, done)))[key]; }
function setLocal(key: string, value: unknown): Promise<void> { return callbackPromise<void>((done) => chrome.storage.local.set({ [key]: value }, () => done())); }
function removeLocal(keys: string[]): Promise<void> { return callbackPromise<void>((done) => chrome.storage.local.remove(keys, () => done())); }
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
function markExpectedRemoval(groupId: string, cookie: chrome.cookies.Cookie): void { expectedRemovals.set(`${groupId}\u0000${cookieIdentity(cookie)}`, Date.now() + 30_000); }
function consumeExpectedRemoval(groupId: string, cookie: chrome.cookies.Cookie): boolean { const key = `${groupId}\u0000${cookieIdentity(cookie)}`; const expiry = expectedRemovals.get(key); expectedRemovals.delete(key); return expiry !== undefined && expiry >= Date.now(); }
function randomNonce(): string { return [...crypto.getRandomValues(new Uint8Array(32))].map((byte) => byte.toString(16).padStart(2, "0")).join(""); }
function isUnlockPageMessage(value: unknown): value is { type: "unlock.status" | "unlock.start" } { const type = typeof value === "object" && value !== null ? (value as { type?: unknown }).type : undefined; return type === "unlock.status" || type === "unlock.start"; }
function isEnvelope(value: unknown): value is Envelope { const item = value as Partial<Envelope>; return typeof value === "object" && value !== null && typeof item.v === "number" && typeof item.conn_nonce === "string" && typeof item.seq === "number" && typeof item.id === "string" && typeof item.type === "string" && typeof item.payload === "object" && item.payload !== null; }
function requiredString(value: Record<string, unknown>, key: string): string { const item = value[key]; if (typeof item !== "string") throw new Error(`${key} must be string`); return item; }
function optionalString(value: Record<string, unknown>, key: string): string | undefined { const item = value[key]; if (item === null || item === undefined) return undefined; if (typeof item !== "string") throw new Error(`${key} must be string or null`); return item; }
function requiredNumber(value: Record<string, unknown>, key: string): number { const item = value[key]; if (typeof item !== "number" || !Number.isFinite(item)) throw new Error(`${key} must be number`); return item; }
function requiredInteger(value: Record<string, unknown>, key: string): number { const item = requiredNumber(value, key); if (!Number.isSafeInteger(item) || item < 0) throw new Error(`${key} must be a non-negative integer`); return item; }
function callbackPromise<T>(invoke: (done: (value: T) => void) => void): Promise<T> { return new Promise((resolve, reject) => invoke((value) => { const error = chrome.runtime.lastError; error === undefined ? resolve(value) : reject(new Error(error.message ?? "Chrome API failed")); })); }
function delay(milliseconds: number): Promise<void> { return new Promise((resolve) => setTimeout(resolve, milliseconds)); }
function unique(values: number[]): number[] { return [...new Set(values)]; }
function enqueue(task: () => Promise<void>): void { queue = queue.then(task, task).catch((error: unknown) => console.error("FCP fail-closed controller error", error)); }

chrome.runtime.onStartup.addListener(() => { void connect(); });
chrome.runtime.onInstalled.addListener((details) => {
  if (details.reason === "install") void chrome.tabs.create({ url: "onboarding.html" });
});
void connect();
