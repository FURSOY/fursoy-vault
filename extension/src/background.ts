import {
  HOST_NAME, PROTOCOL_VERSION, categorizeCookieSetFailure, cookieIdentity, cookieRecord,
  cookieSetDetails, groupForCookie, groupForUrl, hasRequiredEnrollmentCookies,
  loadAccountGroupsConfig, policyParameters, selectorForCookie,
  type AccountGroup, type AccountGroupsConfig, type CookieRecord, type CookieSetFailureCategory,
  type Envelope, type LoadedConfig, type WireMessage,
} from "./protocol.js";
import {
  addToBoundedOutbox, makeMonitorEvent, notificationDecision, notificationText,
  validateMonitorEvent, type MonitorEvent, type MonitorSignal, type NotificationDecisionState,
} from "./monitor.js";

type GroupState = "uninitialized" | "sealed" | "unlocking" | "leased" | "evicting" | "degraded";
type LeasePurpose = "inject" | "enroll";
type SessionInvalidationReason = "external_logout" | "restore_rejected";

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
const ENROLLMENT_STABLE_MS = 3_000;
const ENROLLMENT_TIMEOUT_MS = 20_000;
const NAVIGATION_LOAD_TIMEOUT_MS = 20_000;
const UNLOCK_PAGE_URL = chrome.runtime.getURL("unlock.html");
const MONITOR_ICON_URL = chrome.runtime.getURL("monitor-icon.svg");
const MONITOR_OUTBOX_KEY = "fcp-monitor-outbox-v1";
const MONITOR_RATE_KEY = "fcp-monitor-rate-v1";
const MONITOR_POLL_ALARM = "fcp-monitor-poll";
const configPromise = loadAccountGroupsConfig();

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

chrome.webNavigation.onBeforeNavigate.addListener((details) => enqueue(async () => {
  if (details.frameId !== 0) return;
  const loaded = await configPromise;
  const group = groupForUrl(loaded.config, details.url);
  if (group !== undefined) await interceptSealedNavigation(loaded, group, details.tabId, details.url);
}));

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (!isUnlockPageMessage(message)) return;
  enqueue(async () => {
    try { respond(await handleUnlockPageMessage(await configPromise, message.type, sender.tab?.id)); }
    catch { respond({ ok: false, status: "error", error: "unlock_controller_failed" }); }
  });
  return true;
});

chrome.tabs.onUpdated.addListener((tabId, change, tab) => enqueue(async () => {
  if (change.status !== "complete" && change.url === undefined) return;
  const loaded = await configPromise;
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
  const loaded = await configPromise;
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
    if (state?.relevantTabs.length !== 0 || state.groupState !== "leased") continue;
    const grace = policyParameters(group.policy_level).lastTabGraceMs;
    if (grace === 0) await requestEviction(loaded, group, "last_tab_closed", state.leaseId);
    else chrome.alarms.create(alarmName("last_tab", group.id), { when: Date.now() + grace });
  }
}));

chrome.idle.onStateChanged.addListener((idleState) => enqueue(async () => {
  const loaded = await configPromise;
  const root = await loadState(loaded);
  if (idleState === "active") {
    for (const group of loaded.config.groups) chrome.alarms.clear(alarmName("idle", group.id));
    return;
  }
  for (const group of loaded.config.groups) {
    const state = root.groups[group.id];
    if (idleState === "locked") {
      if (client?.connected) send("auth.cache.clear", { account_group_id: group.id, reason: "locked" });
      if (state?.groupState === "leased") await requestEviction(loaded, group, "locked", state.leaseId);
      continue;
    }
    if (state?.groupState !== "leased") continue;
    const threshold = policyParameters(group.policy_level).idleThresholdSeconds;
    const remainingMs = Math.max(0, (threshold - IDLE_BASE_SECONDS) * 1_000);
    if (remainingMs === 0) await requestEviction(loaded, group, "idle", state.leaseId);
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
  const loaded = await configPromise;
  const group = loaded.config.groups.find((item) => item.id === parsed.groupId);
  if (group === undefined) return;
  const root = await loadState(loaded);
  const state = root.groups[group.id];
  if (state?.groupState !== "leased") return;
  if (parsed.kind === "expiry") await requestEviction(loaded, group, "expiry", state.leaseId);
  else if (parsed.kind === "last_tab" && (await relevantTabIds(group)).length === 0) await requestEviction(loaded, group, "last_tab_closed", state.leaseId);
  else if (parsed.kind === "idle") {
    const idleState = await queryIdleState(policyParameters(group.policy_level).idleThresholdSeconds);
    if (idleState === "idle" || idleState === "locked") await requestEviction(loaded, group, idleState, state.leaseId);
  }
}));

chrome.cookies.onChanged.addListener((info) => enqueue(async () => {
  const loaded = await configPromise;
  const group = groupForCookie(loaded.config, info.cookie);
  if (group === undefined) return;
  if (info.removed) {
    if (consumeExpectedRemoval(group.id, info.cookie)) return;
    await queueMonitorEvent("selector_changed", group.id);
    await delay(750);
    const root = await loadState(loaded);
    const state = root.groups[group.id];
    if (state !== undefined && ["leased", "evicting", "sealed", "degraded"].includes(state.groupState) &&
        !hasRequiredEnrollmentCookies(group, await getCookies(group))) {
      await requestSessionInvalidation(loaded, group, "external_logout");
    }
    return;
  }
  if (mutatingGroups.has(group.id)) return;
  const root = await loadState(loaded);
  const state = root.groups[group.id];
  if (state?.groupState === "uninitialized") {
    const stable = await waitForStableEnrollmentCookies(group);
    if (stable.length > 0) await requestLease(loaded, group, "enroll");
  } else if (state?.groupState === "sealed") {
    await queueMonitorEvent("lease_outside_cookie_created", group.id);
    await requestEviction(loaded, group, "site_cookie_recreated", undefined);
  } else if (state?.groupState === "leased") {
    await queueMonitorEvent("selector_changed", group.id);
  } else if (state?.groupState === "degraded") {
    await requestEviction(loaded, group, "degraded_cookie_detected", state.leaseId);
  }
}));

async function connect(): Promise<void> {
  if (client?.connected) return;
  const loaded = await configPromise;
  client = new NativeClient(loaded.digest, handleHostMessage, async () => {
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

async function handleHostMessage(message: WireMessage): Promise<void> {
  const loaded = await configPromise;
  if (message.type === "handshake.ack") {
    await handleHandshakeAck(loaded, message.payload);
    return;
  }
  if (message.type === "monitor.alert") {
    await handleMonitorAlert(message.payload.event);
    return;
  }
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

async function handleHandshakeAck(loaded: LoadedConfig, payload: Record<string, unknown>): Promise<void> {
  if (requiredNumber(payload, "protocol_version") !== PROTOCOL_VERSION || requiredString(payload, "config_digest") !== loaded.digest) throw new Error("host config/protocol mismatch");
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
    } else if (state.groupState === "uninitialized" && cookies.length > 0) {
      actions.push(async () => { if ((await waitForStableEnrollmentCookies(group)).length > 0) await requestLease(loaded, group, "enroll"); });
    } else if (state.groupState === "leased" && (cookies.length === 0 || state.relevantTabs.length === 0)) {
      const reason = cookies.length === 0 ? "startup_reconciliation" : "last_tab_closed";
      actions.push(() => requestEviction(loaded, group, reason, state.leaseId));
    } else if (state.groupState === "leased" && typeof summary.lease_expiry_unix_ms === "number") {
      if (summary.lease_expiry_unix_ms <= Date.now()) actions.push(() => requestEviction(loaded, group, "expiry", state.leaseId));
      else chrome.alarms.create(alarmName("expiry", group.id), { when: summary.lease_expiry_unix_ms });
    } else if (state.groupState === "sealed" && cookies.length > 0) {
      actions.push(() => requestEviction(loaded, group, "site_cookie_recreated", undefined));
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
}

async function injectCookies(loaded: LoadedConfig, group: AccountGroup, payload: Record<string, unknown>, root: RuntimeState, state: GroupRuntimeState): Promise<void> {
  const leaseId = requiredString(payload, "lease_id");
  if (!Array.isArray(payload.cookies)) throw new Error("cookies.inject cookies must be an array");
  const records = payload.cookies;
  let health = "inject_execution_failed";
  let success = false;
  let stage = "cookie_set";
  try {
    mutatingGroups.add(group.id);
    try {
      for (const raw of records) {
        const cookie = raw as CookieRecord;
        if (selectorForCookie(group, cookie) === undefined) throw new Error("vault cookie outside group");
        await setCookie(group, cookie);
      }
    } finally { mutatingGroups.delete(group.id); }
    stage = "cookie_roundtrip";
    const installed = await getCookies(group);
    const expected = records.map((raw) => cookieIdentity(raw as CookieRecord)).sort();
    const actual = installed.map(cookieIdentity).sort();
    if (expected.length !== actual.length || expected.some((identity, index) => identity !== actual[index])) {
      health = "cookie_roundtrip_failed";
    } else {
      const navigationTabId = state.navigationUnlockRequestTabId;
      const navigationTarget = navigationTabId === undefined ? undefined : state.pendingNavigationUnlocks?.[String(navigationTabId)];
      let tabId: number | undefined;
      if (navigationTabId !== undefined && navigationTarget !== undefined) {
        stage = "navigation_gate_redirect";
        await updateTab(navigationTabId, navigationTarget);
        await waitForRelevantTabComplete(loaded.config, group, navigationTabId);
        tabId = navigationTabId;
      } else {
        stage = "health_tab_query";
        state.relevantTabs = await relevantTabIds(group);
        await saveState(root);
        tabId = state.relevantTabs[0];
      }
      stage = "health_execution";
      health = tabId === undefined ? "no_relevant_tab" : await healthCheckWithBackoff(group, tabId);
    }
    success = health === "authenticated";
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
  state.invalidationPending = health === "logged_out" || health === "invalid_session";
  state.invalidationReason = state.invalidationPending ? "restore_rejected" : undefined;
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
  if (!success && !state.invalidationPending) setTimeout(() => enqueue(() => requestEviction(loaded, group, "startup_reconciliation", leaseId)), 100);
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
    await saveState(root);
    if (count === 0) await queueMonitorEvent("reconciliation_failed", group.id);
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
    await requestLease(loaded, group, "inject");
  }
}

async function requestLease(loaded: LoadedConfig, group: AccountGroup, purpose: LeasePurpose): Promise<void> {
  if (policyParameters(group.policy_level).monitoringOnly) return;
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
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.invalidationPending || state.pendingLeaseRequest !== undefined || state.evictionRequestPending) return;
  const valid = state.groupState === "leased" ||
    (state.groupState === "sealed" && reason === "site_cookie_recreated") ||
    (state.groupState === "degraded" && reason === "degraded_cookie_detected") ||
    (reason === "startup_reconciliation" && state.groupState !== "uninitialized");
  if (!valid) return;
  state.evictionRequestPending = true;
  state.groupState = "evicting";
  state.lastEvent = `eviction_request_pending:${reason}`;
  await saveState(root);
  send("evict.request", { account_group_id: group.id, lease_id: leaseId, operation_id: crypto.randomUUID(), phase: "begin", reason });
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

async function interceptSealedNavigation(loaded: LoadedConfig, group: AccountGroup, tabId: number, targetUrl: string): Promise<void> {
  const root = await loadState(loaded);
  const state = requiredGroupState(root, group.id);
  if (state.groupState !== "sealed" || state.reconciliation || state.invalidationPending || state.pendingLeaseRequest !== undefined || state.evictionRequestPending) return;
  state.pendingNavigationUnlocks ??= {};
  state.pendingNavigationUnlocks[String(tabId)] = targetUrl;
  state.navigationUnlockError = undefined;
  state.lastEvent = "navigation_unlock_intercepted";
  await saveState(root);
  await updateTab(tabId, UNLOCK_PAGE_URL);
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
  constructor(private readonly configDigest: string, private readonly onMessage: (message: WireMessage) => Promise<void>, private readonly onDisconnect: () => Promise<void>) {}
  get connected(): boolean { return this.port !== undefined; }
  start(): void {
    const port = chrome.runtime.connectNative(HOST_NAME);
    this.port = port;
    port.onMessage.addListener((raw) => enqueue(() => this.receive(raw)));
    port.onDisconnect.addListener(() => { this.port = undefined; enqueue(this.onDisconnect); });
    this.send("handshake", { protocol_version: PROTOCOL_VERSION, extension_id: chrome.runtime.id, config_digest: this.configDigest });
  }
  send(type: string, payload: Record<string, unknown>): void {
    this.outgoing += 1;
    this.port?.postMessage({ v: PROTOCOL_VERSION, conn_nonce: this.nonce, seq: this.outgoing, id: crypto.randomUUID(), type, payload });
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
  await setBadge(event.severity === "high" ? "!" : "•", event.severity === "high" ? "#b3261e" : "#b06000");
}

function isNotificationDecisionState(value: unknown): value is NotificationDecisionState {
  return typeof value === "object" && value !== null && Object.values(value).every((item) => typeof item === "number" && Number.isFinite(item));
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

async function removeAllCookies(group: AccountGroup): Promise<void> {
  mutatingGroups.add(group.id);
  try {
    for (const cookie of await getCookies(group)) {
      const selector = selectorForCookie(group, cookie);
      if (selector === undefined) continue;
      markExpectedRemoval(group.id, cookie);
      await callbackPromise((done) => chrome.cookies.remove({ url: selector.url, name: cookie.name, storeId: cookie.storeId, partitionKey: cookie.partitionKey }, done));
    }
  } finally { mutatingGroups.delete(group.id); }
}

async function getCookies(group: AccountGroup): Promise<chrome.cookies.Cookie[]> {
  const sets = await Promise.all(group.cookie_selectors.map((selector) => callbackPromise<chrome.cookies.Cookie[]>((done) => chrome.cookies.getAll({ url: selector.url, name: selector.name }, done))));
  const uniqueCookies = new Map<string, chrome.cookies.Cookie>();
  for (const cookie of sets.flat()) if (selectorForCookie(group, cookie) !== undefined) uniqueCookies.set(cookieIdentity(cookie), cookie);
  return [...uniqueCookies.values()];
}

async function waitForStableEnrollmentCookies(group: AccountGroup): Promise<chrome.cookies.Cookie[]> {
  const deadline = Date.now() + ENROLLMENT_TIMEOUT_MS;
  let signature: string | undefined;
  let stableSince = 0;
  let reported = "";
  while (Date.now() < deadline) {
    const cookies = await getCookies(group);
    const metadata = cookies.map((cookie) => {
      const selector = selectorForCookie(group, cookie);
      return selector === undefined ? undefined : { selector: selector.id, domain: cookie.domain, path: cookie.path, httpOnly: cookie.httpOnly, secure: cookie.secure, sameSite: cookie.sameSite, session: cookie.session };
    }).filter((item) => item !== undefined).sort((left, right) => left.selector.localeCompare(right.selector));
    const serialized = JSON.stringify(metadata);
    if (serialized !== reported) { reported = serialized; console.info(`FCP group ${group.id} selector diagnostic`, metadata); }
    if (hasRequiredEnrollmentCookies(group, cookies)) {
      const next = cookies.map((cookie) => `${cookieIdentity(cookie)}\u0000${cookie.value}`).sort().join("\u0001");
      if (next !== signature) { signature = next; stableSince = Date.now(); }
      else if (Date.now() - stableSince >= ENROLLMENT_STABLE_MS) return cookies;
    } else { signature = undefined; stableSince = 0; }
    await delay(250);
  }
  return [];
}

async function relevantTabIds(group: AccountGroup): Promise<number[]> {
  const tabs = await callbackPromise<chrome.tabs.Tab[]>((done) => chrome.tabs.query({ url: group.navigation_patterns }, done));
  return tabs.flatMap((tab) => tab.id !== undefined && groupForUrl({ version: 1, compatibility_version: 1, groups: [group] }, tab.url) !== undefined ? [tab.id] : []);
}

async function healthCheck(group: AccountGroup, tabId: number): Promise<string> {
  if (group.health_check.kind === "wikipedia_userinfo") {
    const results = await callbackPromise<chrome.scripting.InjectionResult<{ status: number; authenticated: boolean }>[]>((done) => chrome.scripting.executeScript({
      target: { tabId }, world: "MAIN",
      func: async () => {
        const response = await fetch("/w/api.php?action=query&meta=userinfo&format=json&formatversion=2", { credentials: "include", cache: "no-store" });
        const body = await response.json() as { query?: { userinfo?: { id?: number; anon?: boolean } } };
        const user = body.query?.userinfo;
        return { status: response.status, authenticated: response.ok && user?.anon !== true && typeof user?.id === "number" && user.id > 0 };
      },
    }, done));
    const result = results[0]?.result;
    return result?.status === 200 && result.authenticated ? "authenticated" : "logged_out";
  }
  const results = await callbackPromise<chrome.scripting.InjectionResult<{ status: number; state?: string }>[]>((done) => chrome.scripting.executeScript({
    target: { tabId }, world: "MAIN",
    func: async () => {
      const response = await fetch("/api/protected", { credentials: "include", cache: "no-store" });
      const body = await response.json() as { state?: string };
      return { status: response.status, state: body.state };
    },
  }, done));
  const result = results[0]?.result;
  if (result?.status === 200 && result.state === "authenticated") return "authenticated";
  return result?.state === "invalid_session" ? "invalid_session" : "logged_out";
}

async function healthCheckWithBackoff(group: AccountGroup, tabId: number): Promise<string> {
  let result = "invalid_health_response";
  for (const wait of [0, 100, 200, 400, 800]) {
    if (wait > 0) await delay(wait);
    result = await healthCheck(group, tabId);
    if (result === "authenticated" || result === "logged_out" || result === "invalid_session") return result;
    if ((await getCookies(group)).length === 0) return "cookie_disappeared_before_health";
  }
  return result;
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
