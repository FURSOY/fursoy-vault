import { COOKIE_SELECTORS, GROUP_ID, HOST_NAME, ORIGIN, categorizeCookieSetFailure, cookieIdentity, cookieRecord, cookieSetDetails, hasRequiredEnrollmentCookies, isGroupCookie, selectorForCookie, type CookieRecord, type CookieSetFailureCategory, type Envelope, type WireMessage } from "./protocol.js";

type GroupState = "uninitialized" | "sealed" | "unlocking" | "leased" | "evicting" | "degraded";
type LeasePurpose = "inject" | "enroll";
type SessionInvalidationReason = "external_logout" | "restore_rejected";
interface RuntimeState {
  groupState: GroupState;
  leaseId?: string;
  reconciliation: boolean;
  relevantTabs: number[];
  lastEvent: string;
  pendingLeaseRequest?: LeasePurpose;
  evictionRequestPending?: boolean;
  injectAfterReconciliation?: boolean;
  recoveryReconnectPending?: boolean;
  invalidationPending?: boolean;
  invalidationReason?: SessionInvalidationReason;
}
const STATE_KEY = "fcp-mvp-runtime-v1";
const IDLE_SECONDS = 30;
const LEASE_EXPIRY_ALARM = "fcp-mvp-lease-expiry";
const RELEVANT_TAB_QUERY_PATTERNS = ["https://wikipedia.org/*", "https://*.wikipedia.org/*"];
const ENROLLMENT_STABLE_MS = 3_000;
const ENROLLMENT_TIMEOUT_MS = 20_000;
let queue: Promise<void> = Promise.resolve();
let client: NativeClient | undefined;
let mutationDepth = 0;
const expectedRemovals = new Map<string, number>();

class RedactedCookieSetFailure extends Error {
  constructor(readonly category: CookieSetFailureCategory | "no_result") {
    super(`cookie_set_${category}`);
    this.name = "RedactedCookieSetFailure";
  }
}

chrome.idle.setDetectionInterval(IDLE_SECONDS);

chrome.tabs.onUpdated.addListener((tabId, change, tab) => enqueue(async () => {
  if (change.status !== "complete" && change.url === undefined) return;
  const state = await loadState();
  const relevant = isRelevant(tab.url);
  state.relevantTabs = relevant ? unique([...state.relevantTabs, tabId]) : state.relevantTabs.filter((id) => id !== tabId);
  if (relevant && tab.status === "complete" && state.reconciliation) {
    // Handshake-time tabs.query can race navigation. Preserve the concrete onUpdated edge so
    // reconciliation completion cannot lose the user's active-tab unlock intent.
    state.injectAfterReconciliation = true;
  }
  await saveState(state);
  if (relevant && tab.status === "complete" && state.groupState === "sealed" && !state.reconciliation) await requestLease("inject");
  else if (relevant && tab.status === "complete" && state.groupState === "degraded") await requestRecoveryReconnect();
}));

chrome.tabs.onRemoved.addListener((tabId) => enqueue(async () => {
  const state = await loadState();
  // storage.session can contain a stale/missed tab id after a worker restart. Query Chrome's
  // current tab set instead of requiring the removed id to have been observed previously.
  state.relevantTabs = await relevantTabIds();
  await saveState(state);
  if (state.relevantTabs.length === 0 && state.groupState === "leased") await requestEviction("last_tab_closed", state.leaseId);
}));

chrome.idle.onStateChanged.addListener((idleState) => enqueue(async () => {
  const state = await loadState();
  if ((idleState === "idle" || idleState === "locked") && state.groupState === "leased") {
    await requestEviction(idleState, state.leaseId);
  }
}));

chrome.cookies.onChanged.addListener((info) => enqueue(async () => {
  if (!isGroupCookie(info.cookie)) return;
  if (info.removed) {
    if (consumeExpectedRemoval(info.cookie)) return;
    // A site cookie overwrite produces a removed event followed by a replacement. Wait for the
    // complete auth set before classifying this as an actual logout/expiry.
    await delay(750);
    const state = await loadState();
    if ((state.groupState === "leased" || state.groupState === "evicting" ||
         state.groupState === "sealed" || state.groupState === "degraded") &&
        !hasRequiredEnrollmentCookies(await getCookies())) {
      await requestSessionInvalidation("external_logout");
    }
    return;
  }
  if (mutationDepth > 0) return;
  const state = await loadState();
  if (state.groupState === "uninitialized") {
    const stableCookies = await waitForStableEnrollmentCookies();
    if (stableCookies.length > 0) await requestLease("enroll");
  }
  else if (state.groupState === "sealed") await requestEviction("site_cookie_recreated", undefined);
  else if (state.groupState === "degraded") await requestEviction("degraded_cookie_detected", state.leaseId);
}));

chrome.alarms.onAlarm.addListener((alarm) => enqueue(async () => {
  if (alarm.name !== LEASE_EXPIRY_ALARM) return;
  const state = await loadState();
  if (state.groupState === "leased" && state.leaseId !== undefined) {
    await requestEviction("expiry", state.leaseId);
  }
}));

function connect(): void {
  if (client?.connected) return;
  client = new NativeClient(handleHostMessage, async () => {
    const state = await loadState();
    if (state.groupState === "leased" || state.groupState === "evicting") {
      await removeAllCookies();
      state.groupState = "degraded";
      state.lastEvent = "native_disconnect_fail_closed";
      await saveState(state);
    }
    setTimeout(connect, 1_000);
  });
  client.start();
}

async function handleHostMessage(message: WireMessage): Promise<void> {
  const state = await loadState();
  switch (message.type) {
    case "handshake.ack": {
      const pendingInvalidation = state.invalidationPending === true ? state.invalidationReason : undefined;
      state.groupState = requiredString(message.payload, "group_state") as GroupState;
      state.leaseId = optionalString(message.payload, "lease_id");
      state.relevantTabs = await relevantTabIds();
      const cookies = await getCookies();
      const hostRequiresReconciliation = requiredBoolean(message.payload, "reconciliation_required");
      // A new native connection is authoritative. In-flight flags from the previous process cannot
      // still have a matching operation and must not suppress the recovery action selected below.
      state.pendingLeaseRequest = undefined;
      state.evictionRequestPending = false;
      state.reconciliation = hostRequiresReconciliation;
      state.injectAfterReconciliation = hostRequiresReconciliation && state.relevantTabs.length > 0;
      state.recoveryReconnectPending = false;
      state.lastEvent = "handshake_ack";
      if (state.groupState === "uninitialized") {
        state.invalidationPending = false;
        state.invalidationReason = undefined;
      } else if (pendingInvalidation !== undefined) {
        state.invalidationPending = true;
        state.invalidationReason = pendingInvalidation;
      }
      await saveState(state);
      if (pendingInvalidation !== undefined && state.groupState !== "uninitialized") {
        send("session.invalidate", {
          account_group_id: GROUP_ID,
          lease_id: state.leaseId,
          reason: pendingInvalidation,
        });
      } else if (state.groupState === "uninitialized" && cookies.length > 0) {
        const stableCookies = await waitForStableEnrollmentCookies();
        if (stableCookies.length > 0) await requestLease("enroll");
      } else if (state.groupState === "leased" && cookies.length === 0) {
        state.reconciliation = true;
        state.injectAfterReconciliation = state.relevantTabs.length > 0;
        await saveState(state);
        await requestEviction("startup_reconciliation", state.leaseId);
      } else if (state.groupState === "leased" && state.relevantTabs.length === 0) {
        // A cold worker can be woken by tabs.onRemoved. Decide from the current tab set before
        // starting reconciliation so the real last-tab trigger cannot be shadowed by handshake.
        await requestEviction("last_tab_closed", state.leaseId);
      } else if (state.groupState === "leased") {
        const expiry = optionalNumber(message.payload, "lease_expiry_unix_ms");
        if (expiry !== undefined && expiry <= Date.now()) await requestEviction("expiry", state.leaseId);
        else if (expiry !== undefined) chrome.alarms.create(LEASE_EXPIRY_ALARM, { when: expiry });
      } else if (state.groupState === "sealed" && cookies.length > 0) {
        await requestEviction("site_cookie_recreated", undefined);
      } else if (hostRequiresReconciliation) {
        await requestEviction("startup_reconciliation", state.leaseId);
      }
      break;
    }
    case "lease.grant":
      state.leaseId = requiredString(message.payload, "lease_id");
      chrome.alarms.create(LEASE_EXPIRY_ALARM, { when: requiredNumber(message.payload, "expiry_unix_ms") });
      state.pendingLeaseRequest = undefined;
      state.lastEvent = "lease_grant";
      await saveState(state);
      break;
    case "lease.deny": {
      if (state.invalidationPending === true) {
        state.lastEvent = "session_invalidation_retry";
        await saveState(state);
        client?.disconnect();
        break;
      }
      const deniedLease = state.pendingLeaseRequest;
      const deniedEviction = state.evictionRequestPending === true;
      state.pendingLeaseRequest = undefined;
      state.evictionRequestPending = false;
      if (deniedEviction) state.groupState = "degraded";
      else if (deniedLease === "enroll") state.groupState = "uninitialized";
      else if (deniedLease === "inject") state.groupState = "sealed";
      state.lastEvent = `lease_deny:${requiredString(message.payload, "reason")}`;
      await saveState(state);
      break;
    }
    case "cookies.inject":
      await injectCookies(message.payload, state);
      break;
    case "evict.request":
      if (requiredString(message.payload, "phase") !== "snapshot_required") throw new Error("host requested unsupported eviction phase");
      state.groupState = "evicting";
      state.evictionRequestPending = true;
      state.lastEvent = `snapshot_required:${requiredString(message.payload, "reason")}`;
      await saveState(state);
      send("cookies.snapshot", {
        account_group_id: GROUP_ID,
        lease_id: optionalString(message.payload, "lease_id"),
        operation_id: requiredString(message.payload, "operation_id"),
        cookies: (await getCookies()).map(cookieRecord),
      });
      break;
    case "evict.confirmed":
      await finishEviction(message.payload, state);
      break;
    case "session.invalidated": {
      chrome.alarms.clear(LEASE_EXPIRY_ALARM);
      let cleanupSucceeded = true;
      try { await removeAllCookies(); } catch { cleanupSucceeded = false; }
      state.groupState = "uninitialized";
      state.leaseId = undefined;
      state.reconciliation = false;
      state.pendingLeaseRequest = undefined;
      state.evictionRequestPending = false;
      state.injectAfterReconciliation = false;
      state.recoveryReconnectPending = false;
      state.invalidationPending = false;
      state.invalidationReason = undefined;
      state.lastEvent = cleanupSucceeded ? "session_invalidated" : "session_invalidated_cleanup_failed";
      await saveState(state);
      break;
    }
    default:
      throw new Error(`unsupported host message ${message.type}`);
  }
}

async function injectCookies(payload: Record<string, unknown>, state: RuntimeState): Promise<void> {
  const leaseId = requiredString(payload, "lease_id");
  const records = payload.cookies;
  if (!Array.isArray(records)) throw new Error("cookies.inject cookies must be an array");
  let health = "inject_execution_failed";
  let success = false;
  let stage = "cookie_set";
  try {
    mutationDepth += 1;
    try {
      for (const raw of records) {
        const cookie = raw as CookieRecord;
        if (selectorForCookie(cookie) === undefined) throw new Error("vault cookie is outside the account group");
        await setCookie(cookie);
      }
    } finally { mutationDepth -= 1; }
    stage = "cookie_roundtrip";
    const installed = await getCookies();
    const expectedIdentities = records.map((raw) => cookieIdentity(raw as CookieRecord)).sort();
    const installedIdentities = installed.map(cookieIdentity).sort();
    if (expectedIdentities.length !== installedIdentities.length ||
        expectedIdentities.some((identity, index) => identity !== installedIdentities[index])) {
      health = "cookie_roundtrip_failed";
    } else {
      stage = "health_tab_query";
      state.relevantTabs = await relevantTabIds();
      await saveState(state);
      const tabId = state.relevantTabs[0];
      stage = "health_execution";
      health = tabId === undefined ? "no_relevant_tab" : await healthCheckWithBackoff(tabId);
    }
    success = health === "authenticated";
  } catch (error: unknown) {
    // Never allow an API/content execution failure to strand the host in Leased. Do not serialize
    // exception text because Chrome errors may contain URLs or other environment detail.
    health = stage === "cookie_set" && error instanceof RedactedCookieSetFailure
      ? `cookie_set_${error.category}`
      : `${stage}_failed`;
    console.error(`FCP inject failed: ${health}`);
  }
  if (!success) {
    chrome.alarms.clear(LEASE_EXPIRY_ALARM);
    try {
      await removeAllCookies();
    } catch {
      health = "inject_cleanup_failed";
      console.error("FCP inject fail-closed cleanup failed");
    }
  }
  state.groupState = success ? "leased" : "degraded";
  state.leaseId = leaseId;
  state.invalidationPending = health === "logged_out" || health === "invalid_session";
  state.invalidationReason = state.invalidationPending ? "restore_rejected" : undefined;
  state.lastEvent = `inject:${health}`;
  await saveState(state);
  send("inject.result", { account_group_id: GROUP_ID, lease_id: leaseId, success, health_check: health });
}

async function finishEviction(payload: Record<string, unknown>, state: RuntimeState): Promise<void> {
  const cookieDisposition = requiredString(payload, "cookie_disposition");
  const shouldInjectAfterReconciliation =
    state.reconciliation &&
    state.injectAfterReconciliation === true &&
    state.relevantTabs.length > 0;

  if (cookieDisposition === "retain_leased") {
    const retainedCount = (await getCookies()).length;
    const retained = retainedCount > 0;
    const operationId = requiredString(payload, "operation_id");
    send("evict.result", {
      account_group_id: GROUP_ID,
      lease_id: optionalString(payload, "lease_id"),
      operation_id: operationId,
      success: retained,
      remaining_cookie_count: retainedCount,
    });
    state.groupState = retained ? "leased" : "degraded";
    state.reconciliation = false;
    state.evictionRequestPending = false;
    state.injectAfterReconciliation = false;
    state.lastEvent = retained ? "enrollment_retained_leased" : "enrollment_cookie_missing";
    await saveState(state);
    return;
  }
  if (cookieDisposition !== "remove") throw new Error("unsupported cookie disposition");

  await removeAllCookies();
  const remaining = (await getCookies()).length;
  const operationId = requiredString(payload, "operation_id");
  send("evict.result", {
    account_group_id: GROUP_ID, lease_id: optionalString(payload, "lease_id"), operation_id: operationId,
    success: remaining === 0, remaining_cookie_count: remaining,
  });
  state.groupState = remaining === 0 ? "sealed" : "degraded";
  chrome.alarms.clear(LEASE_EXPIRY_ALARM);
  state.leaseId = undefined;
  state.reconciliation = false;
  state.evictionRequestPending = false;
  state.injectAfterReconciliation = false;
  state.lastEvent = remaining === 0 ? "eviction_complete" : "eviction_failed";
  await saveState(state);
  if (state.invalidationPending === true && state.invalidationReason !== undefined) {
    // evict.result was posted first on the ordered native port, so the host has left its pending
    // eviction before processing this terminal revocation.
    send("session.invalidate", {
      account_group_id: GROUP_ID,
      lease_id: state.leaseId,
      reason: state.invalidationReason,
    });
    return;
  }
  if (remaining === 0 && shouldInjectAfterReconciliation) {
    // evict.result is posted before lease.request on the same ordered native port. The host first
    // commits Sealed, then handles this inject request. Enrollment does not set the latch, so its
    // required logged-out checkpoint remains intact.
    await requestLease("inject");
  }
}

async function requestLease(purpose: LeasePurpose): Promise<void> {
  const state = await loadState();
  if (state.invalidationPending === true || state.pendingLeaseRequest !== undefined || state.evictionRequestPending === true) return;
  if (purpose === "enroll" && state.groupState !== "uninitialized") return;
  if (purpose === "inject" && state.groupState !== "sealed") return;

  // Persist before posting. storage.session survives MV3 worker restarts, so a host crash or a
  // burst of tab/cookie events cannot turn one user action into repeated Hello prompts.
  state.pendingLeaseRequest = purpose;
  state.groupState = "unlocking";
  state.lastEvent = `lease_request_pending:${purpose}`;
  await saveState(state);
  send("lease.request", { account_group_id: GROUP_ID, purpose, requested_duration_ms: 300_000 });
}

async function requestEviction(reason: string, leaseId: string | undefined): Promise<void> {
  const state = await loadState();
  if (state.invalidationPending === true || state.pendingLeaseRequest !== undefined || state.evictionRequestPending === true) return;
  const validState = state.groupState === "leased" ||
    (state.groupState === "sealed" && reason === "site_cookie_recreated") ||
    (state.groupState === "degraded" && reason === "degraded_cookie_detected") ||
    (reason === "startup_reconciliation" && state.groupState !== "uninitialized");
  if (!validState) return;
  state.evictionRequestPending = true;
  state.groupState = "evicting";
  state.lastEvent = `eviction_request_pending:${reason}`;
  await saveState(state);
  send("evict.request", { account_group_id: GROUP_ID, lease_id: leaseId, operation_id: crypto.randomUUID(), phase: "begin", reason });
}

async function requestRecoveryReconnect(): Promise<void> {
  const state = await loadState();
  if (state.invalidationPending === true || state.recoveryReconnectPending === true || state.pendingLeaseRequest !== undefined || state.evictionRequestPending === true) return;
  state.recoveryReconnectPending = true;
  state.lastEvent = "degraded_reconciliation_requested";
  await saveState(state);
  client?.disconnect();
}

async function requestSessionInvalidation(reason: SessionInvalidationReason): Promise<void> {
  const state = await loadState();
  if (state.groupState === "uninitialized" || state.invalidationPending === true) return;
  state.invalidationPending = true;
  state.invalidationReason = reason;
  state.lastEvent = `session_invalidation_pending:${reason}`;
  await saveState(state);
  if (state.evictionRequestPending === true) return;
  send("session.invalidate", {
    account_group_id: GROUP_ID,
    lease_id: state.leaseId,
    reason,
  });
}

function send(type: string, payload: Record<string, unknown>): void {
  if (!client?.connected) throw new Error("native host is not connected");
  client.send(type, payload);
}

class NativeClient {
  private port?: chrome.runtime.Port;
  private readonly nonce = randomNonce();
  private outgoing = 0;
  private incoming = 0;
  constructor(private readonly onMessage: (message: WireMessage) => Promise<void>, private readonly onDisconnect: () => Promise<void>) {}
  get connected(): boolean { return this.port !== undefined; }
  disconnect(): void { this.port?.disconnect(); }
  start(): void {
    const port = chrome.runtime.connectNative(HOST_NAME);
    this.port = port;
    port.onMessage.addListener((raw) => enqueue(() => this.receive(raw)));
    port.onDisconnect.addListener(() => { this.port = undefined; enqueue(this.onDisconnect); });
    this.send("handshake", { protocol_version: 1, extension_id: chrome.runtime.id });
  }
  send(type: string, payload: Record<string, unknown>): void {
    this.outgoing += 1;
    this.port?.postMessage({ v: 1, conn_nonce: this.nonce, seq: this.outgoing, id: crypto.randomUUID(), type, payload });
  }
  private async receive(raw: unknown): Promise<void> {
    if (!isEnvelope(raw)) throw new Error("malformed native envelope");
    if (raw.v !== 1 || raw.conn_nonce !== this.nonce || raw.seq !== this.incoming + 1) throw new Error("native nonce/sequence validation failed");
    this.incoming = raw.seq;
    await this.onMessage({ type: raw.type, payload: raw.payload });
  }
}

async function setCookie(cookie: CookieRecord): Promise<void> {
  const details = cookieSetDetails(cookie);
  await new Promise<void>((resolve, reject) => {
    try {
      chrome.cookies.set(details, (value) => {
        const error = chrome.runtime.lastError;
        if (error !== undefined) {
          reject(new RedactedCookieSetFailure(categorizeCookieSetFailure(error.message)));
          return;
        }
        if (value === undefined) {
          reject(new RedactedCookieSetFailure("no_result"));
          return;
        }
        resolve();
      });
    } catch (error: unknown) {
      // Chrome argument-schema failures can throw before a callback exists. Categorize the local
      // exception immediately and discard its potentially identifying text just like lastError.
      reject(new RedactedCookieSetFailure(categorizeCookieSetFailure(
        error instanceof Error ? error.message : undefined,
      )));
    }
  });
}

async function removeAllCookies(): Promise<void> {
  mutationDepth += 1;
  try {
    for (const cookie of await getCookies()) {
      const selector = selectorForCookie(cookie);
      if (selector === undefined) continue;
      markExpectedRemoval(cookie);
      await callbackPromise((done) => chrome.cookies.remove({ url: selector.url, name: cookie.name, storeId: cookie.storeId, partitionKey: cookie.partitionKey }, done));
    }
  }
  finally { mutationDepth -= 1; }
}

async function getCookies(): Promise<chrome.cookies.Cookie[]> {
  const groups = await Promise.all(COOKIE_SELECTORS.map((selector) =>
    callbackPromise<chrome.cookies.Cookie[]>((done) =>
      chrome.cookies.getAll({ url: selector.url, name: selector.name }, done),
    ),
  ));
  const uniqueCookies = new Map<string, chrome.cookies.Cookie>();
  for (const cookie of groups.flat()) {
    if (isGroupCookie(cookie)) uniqueCookies.set(cookieIdentity(cookie), cookie);
  }
  return [...uniqueCookies.values()];
}

async function waitForStableEnrollmentCookies(): Promise<chrome.cookies.Cookie[]> {
  const deadline = Date.now() + ENROLLMENT_TIMEOUT_MS;
  let stableSignature: string | undefined;
  let stableSince = 0;
  let reportedMetadata = "";
  while (Date.now() < deadline) {
    const cookies = await getCookies();
    const metadata = cookies
      .map((cookie) => {
        const selector = selectorForCookie(cookie);
        return selector === undefined ? undefined : {
          selector: selector.id,
          domain: cookie.domain,
          path: cookie.path,
          httpOnly: cookie.httpOnly,
          secure: cookie.secure,
          sameSite: cookie.sameSite,
          session: cookie.session,
        };
      })
      .filter((item) => item !== undefined)
      .sort((left, right) => left.selector.localeCompare(right.selector));
    const serializedMetadata = JSON.stringify(metadata);
    if (serializedMetadata !== reportedMetadata) {
      reportedMetadata = serializedMetadata;
      // Values and usernames are never logged. This is the manual pre-enrollment selector proof.
      console.info("FCP Wikipedia selector diagnostic", metadata);
    }
    if (hasRequiredEnrollmentCookies(cookies)) {
      // Values participate only in this in-memory stabilization signature, never in logs/storage.
      const signature = cookies
        .map((cookie) => `${cookieIdentity(cookie)}\u0000${cookie.value}`)
        .sort()
        .join("\u0001");
      if (signature !== stableSignature) {
        stableSignature = signature;
        stableSince = Date.now();
      } else if (Date.now() - stableSince >= ENROLLMENT_STABLE_MS) {
        return cookies;
      }
    } else {
      stableSignature = undefined;
      stableSince = 0;
    }
    await delay(250);
  }
  console.warn("FCP Wikipedia enrollment skipped: required selector set did not stabilize");
  return [];
}
function relevantTabIds(): Promise<number[]> {
  return callbackPromise<chrome.tabs.Tab[]>((done) =>
    chrome.tabs.query({ url: RELEVANT_TAB_QUERY_PATTERNS }, done),
  ).then((tabs) => tabs.flatMap((tab) =>
    tab.id !== undefined && isRelevant(tab.url) ? [tab.id] : [],
  ));
}
async function healthCheck(tabId: number): Promise<string> {
  const results = await callbackPromise<chrome.scripting.InjectionResult<{ status: number; authenticated: boolean }>[]>((done) =>
    chrome.scripting.executeScript({
      target: { tabId },
      // MAIN is mandatory: an isolated-world fetch has extension request semantics and did not
      // attach the site's session cookie even though chrome.cookies.getAll saw it.
      world: "MAIN",
      func: async () => {
        const response = await fetch("/w/api.php?action=query&meta=userinfo&format=json&formatversion=2", { credentials: "include", cache: "no-store" });
        const body = await response.json() as { query?: { userinfo?: { id?: number; anon?: boolean } } };
        const user = body.query?.userinfo;
        return { status: response.status, authenticated: response.ok && user?.anon !== true && typeof user?.id === "number" && user.id > 0 };
      },
    }, done),
  );
  const result = results[0]?.result;
  if (result === undefined) return "invalid_health_response";
  return result.status === 200 && result.authenticated ? "authenticated" : "logged_out";
}
async function healthCheckWithBackoff(tabId: number): Promise<string> {
  let lastResult = "invalid_health_response";
  const delaysMs = [0, 100, 200, 400, 800];
  for (const delayMs of delaysMs) {
    if (delayMs > 0) await delay(delayMs);
    lastResult = await healthCheck(tabId);
    if (lastResult === "authenticated" || lastResult === "logged_out") return lastResult;
    if ((await getCookies()).length === 0) return "cookie_disappeared_before_health";
  }
  return lastResult;
}
function callbackPromise<T>(invoke: (done: (value: T) => void) => void): Promise<T> { return new Promise((resolve, reject) => invoke((value) => { const error = chrome.runtime.lastError; error === undefined ? resolve(value) : reject(new Error(error.message ?? "Chrome API failed")); })); }
function delay(milliseconds: number): Promise<void> { return new Promise((resolve) => setTimeout(resolve, milliseconds)); }
function enqueue(task: () => Promise<void>): void {
  queue = queue.then(task, task).catch(() => console.error("FCP fail-closed controller error"));
}
function isRelevant(url?: string): boolean {
  try {
    if (url === undefined) return false;
    const parsed = new URL(url);
    return parsed.protocol === "https:" &&
      (parsed.hostname === "wikipedia.org" || parsed.hostname.endsWith(".wikipedia.org"));
  } catch {
    return false;
  }
}
function unique(values: number[]): number[] { return [...new Set(values)]; }
function markExpectedRemoval(cookie: chrome.cookies.Cookie): void {
  expectedRemovals.set(cookieIdentity(cookie), Date.now() + 30_000);
}
function consumeExpectedRemoval(cookie: chrome.cookies.Cookie): boolean {
  const identity = cookieIdentity(cookie);
  const expiry = expectedRemovals.get(identity);
  expectedRemovals.delete(identity);
  return expiry !== undefined && expiry >= Date.now();
}
function randomNonce(): string { const bytes = crypto.getRandomValues(new Uint8Array(32)); return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join(""); }
function isEnvelope(value: unknown): value is Envelope { const item = value as Partial<Envelope>; return typeof value === "object" && value !== null && item.v === 1 && typeof item.conn_nonce === "string" && typeof item.seq === "number" && typeof item.id === "string" && typeof item.type === "string" && typeof item.payload === "object" && item.payload !== null; }
function requiredString(value: Record<string, unknown>, key: string): string { const item = value[key]; if (typeof item !== "string") throw new Error(`${key} must be a string`); return item; }
function optionalString(value: Record<string, unknown>, key: string): string | undefined { const item = value[key]; if (item === null || item === undefined) return undefined; if (typeof item !== "string") throw new Error(`${key} must be a string or null`); return item; }
function requiredBoolean(value: Record<string, unknown>, key: string): boolean { const item = value[key]; if (typeof item !== "boolean") throw new Error(`${key} must be boolean`); return item; }
function requiredNumber(value: Record<string, unknown>, key: string): number { const item = value[key]; if (typeof item !== "number" || !Number.isFinite(item)) throw new Error(`${key} must be a finite number`); return item; }
function optionalNumber(value: Record<string, unknown>, key: string): number | undefined { const item = value[key]; if (item === null || item === undefined) return undefined; if (typeof item !== "number" || !Number.isFinite(item)) throw new Error(`${key} must be a finite number or null`); return item; }
async function loadState(): Promise<RuntimeState> { const stored = await callbackPromise<Record<string, unknown>>((done) => chrome.storage.session.get(STATE_KEY, done)); const value = stored[STATE_KEY] as Partial<RuntimeState> | undefined; return value?.groupState === undefined ? { groupState: "uninitialized", reconciliation: false, relevantTabs: [], lastEvent: "startup" } : value as RuntimeState; }
function saveState(state: RuntimeState): Promise<void> { return callbackPromise<void>((done) => chrome.storage.session.set({ [STATE_KEY]: state }, () => done())); }

// Class declarations have a temporal dead zone. Start the native connection only after the module
// has evaluated NativeClient and every helper used by its synchronous constructor/start path.
chrome.runtime.onStartup.addListener(connect);
connect();
