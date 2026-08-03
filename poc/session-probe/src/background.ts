export {};

const DUTY_STATE_KEY = "fcp-duty-cycle-state-v1";
const DUTY_ALARM = "fcp-duty-cycle-end";
const COOKIE_NAME = "FCP-session-probe";
const DEFAULT_ORIGIN = "http://localhost:43118";
const MAX_EVENTS = 500;

type DutyEventName =
  | "measurement_started"
  | "measurement_stopped"
  | "login_detected"
  | "inject"
  | "last_tab_closed"
  | "idle_start"
  | "eviction"
  | "reconciliation"
  | "failed_eviction"
  | "site_cookie_recreated";

interface DutyEvent {
  sequence: number;
  at: number;
  event: DutyEventName;
  detail: string;
  durationMs?: number;
}

interface CookieSnapshot {
  setDetails: chrome.cookies.CookieSetDetails;
  capturedAt: number;
}

interface MutationGuard {
  expectedPresent: boolean;
  until: number;
}

interface DutyState {
  version: 1;
  active: boolean;
  origin: string;
  startedAt: number;
  plannedEndAt: number;
  stoppedAt?: number;
  idleThresholdSeconds: number;
  idleState: chrome.idle.IdleState;
  relevantTabIds: number[];
  activeRelevantTab: boolean;
  cookiePresent: boolean;
  snapshot?: CookieSnapshot;
  lastMetricAt: number;
  cookiePresentMs: number;
  activeExposureMs: number;
  unnecessaryExposureMs: number;
  events: DutyEvent[];
  nextSequence: number;
  mutationGuard?: MutationGuard;
}

interface StartMessage {
  target: "fcp-duty-cycle";
  command: "start";
  durationMs: number;
  idleThresholdSeconds: number;
  origin?: string;
}

interface SimpleMessage {
  target: "fcp-duty-cycle";
  command: "get-state" | "stop";
}

interface ObserveLoginMessage {
  target: "fcp-duty-cycle";
  command: "observe-login";
  tabId: number;
}

type DutyMessage = StartMessage | SimpleMessage | ObserveLoginMessage;

let operationQueue: Promise<void> = Promise.resolve();

chrome.action.onClicked.addListener(() => {
  chrome.tabs.create({ url: chrome.runtime.getURL("duty.html") });
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!isDutyMessage(message)) {
    return;
  }

  enqueue(async () => {
    try {
      const result = await handleDutyMessage(message);
      sendResponse({ ok: true, data: result });
    } catch (error: unknown) {
      sendResponse({ ok: false, error: errorMessage(error) });
    }
  });
  return true;
});

chrome.tabs.onRemoved.addListener((tabId) => {
  enqueue(async () => {
    const state = await loadState();
    if (!state?.active || !state.relevantTabIds.includes(tabId)) {
      return;
    }

    advanceMetrics(state, Date.now());
    state.relevantTabIds = state.relevantTabIds.filter((id) => id !== tabId);
    state.activeRelevantTab = await computeActiveRelevantTab(state.origin);
    if (state.relevantTabIds.length === 0) {
      addEvent(state, "last_tab_closed", `tab_id=${tabId}`);
      await saveState(state);
      await evict(state, "last_tab_closed");
    } else {
      await saveState(state);
    }
  });
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (changeInfo.status !== "complete" && changeInfo.url === undefined) {
    return;
  }
  enqueue(async () => {
    const state = await loadState();
    if (!state?.active) {
      return;
    }

    const relevant = isRelevantUrl(tab.url, state.origin);
    const wasRelevant = state.relevantTabIds.includes(tabId);
    if (relevant !== wasRelevant) {
      advanceMetrics(state, Date.now());
      state.relevantTabIds = relevant
        ? uniqueNumbers([...state.relevantTabIds, tabId])
        : state.relevantTabIds.filter((id) => id !== tabId);
      state.activeRelevantTab = await computeActiveRelevantTab(state.origin);
      await saveState(state);
    }

    if (relevant && tab.status === "complete") {
      await handleRelevantTabReady(state, tabId);
    }
  });
});

chrome.tabs.onActivated.addListener(() => {
  enqueue(() => refreshActiveUse("tab_activated"));
});

chrome.windows.onFocusChanged.addListener(() => {
  enqueue(() => refreshActiveUse("window_focus_changed"));
});

chrome.idle.onStateChanged.addListener((newState) => {
  enqueue(async () => {
    const state = await loadState();
    if (!state?.active || state.idleState === newState) {
      return;
    }

    advanceMetrics(state, Date.now());
    state.idleState = newState;
    state.activeRelevantTab = await computeActiveRelevantTab(state.origin);
    if (newState === "idle" || newState === "locked") {
      addEvent(state, "idle_start", `idle_state=${newState}`);
      await saveState(state);
      await evict(state, newState === "locked" ? "lock" : "idle");
      return;
    }

    await saveState(state);
    if (state.relevantTabIds.length > 0 && !state.cookiePresent) {
      await restore(state, "idle_end");
    }
  });
});

chrome.cookies.onChanged.addListener((changeInfo) => {
  enqueue(async () => {
    const state = await loadState();
    if (!state?.active || !cookieBelongsToExperiment(changeInfo.cookie, state)) {
      return;
    }

    const now = Date.now();
    const guard = state.mutationGuard;
    const resultingPresence = !changeInfo.removed;
    if (guard !== undefined && guard.until >= now && guard.expectedPresent === resultingPresence) {
      return;
    }

    advanceMetrics(state, now);
    state.cookiePresent = resultingPresence;
    if (resultingPresence) {
      const isInitialLoginCookie = state.snapshot === undefined;
      state.snapshot = snapshotCookie(changeInfo.cookie, state.origin);
      if (isInitialLoginCookie) {
        addEvent(
          state,
          "reconciliation",
          `initial_session_cookie_observed; cause=${changeInfo.cause}`,
        );
      } else {
        addEvent(
          state,
          "site_cookie_recreated",
          `cause=${changeInfo.cause}; source=chrome.cookies.onChanged`,
        );
      }
      await saveState(state);
      if (
        !isInitialLoginCookie &&
        (state.idleState !== "active" || state.relevantTabIds.length === 0)
      ) {
        await evict(state, "site_cookie_recreated_while_inactive");
      }
      return;
    }

    addEvent(state, "reconciliation", `cookie_removed_externally; cause=${changeInfo.cause}`);
    await saveState(state);
  });
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === DUTY_ALARM) {
    enqueue(async () => {
      const state = await loadState();
      if (state?.active) {
        await stopMeasurement(state, "duration_elapsed");
      }
    });
  }
});

chrome.runtime.onStartup.addListener(() => {
  enqueue(() => reconcile("browser_startup"));
});

// MV3 service workers can be suspended between events. storage.session retains this synthetic
// experiment's state (including only the dummy cookie snapshot) in memory for browser lifetime.
enqueue(() => reconcile("service_worker_start"));

function enqueue(task: () => Promise<void>): void {
  operationQueue = operationQueue.then(task, task).catch((error: unknown) => {
    console.error("FCP duty-cycle background task failed", error);
  });
}

async function handleDutyMessage(message: DutyMessage): Promise<unknown> {
  switch (message.command) {
    case "start":
      return startMeasurement(message);
    case "get-state": {
      const state = await loadState();
      if (state === undefined) {
        return null;
      }
      if (state.active) {
        advanceMetrics(state, Date.now());
        await saveState(state);
      }
      return publicState(state);
    }
    case "observe-login": {
      const state = requireActiveState(await loadState());
      await observeLogin(state, message.tabId);
      return publicState(state);
    }
    case "stop": {
      const state = await loadState();
      if (state === undefined) {
        throw new Error("no duty-cycle measurement exists");
      }
      if (state.active) {
        await stopMeasurement(state, "manual_or_harness_stop");
      }
      return publicState(state);
    }
  }
}

async function startMeasurement(message: StartMessage): Promise<unknown> {
  if (!Number.isFinite(message.durationMs) || message.durationMs < 60_000) {
    throw new Error("durationMs must be at least 60000");
  }
  if (!Number.isFinite(message.idleThresholdSeconds) || message.idleThresholdSeconds < 15) {
    throw new Error("idleThresholdSeconds must be at least 15");
  }

  const existing = await loadState();
  if (existing?.active) {
    throw new Error("a duty-cycle measurement is already active");
  }

  const origin = normalizeOrigin(message.origin ?? DEFAULT_ORIGIN);
  const now = Date.now();
  chrome.idle.setDetectionInterval(Math.floor(message.idleThresholdSeconds));
  const [idleState, relevantTabs, activeRelevantTab, cookies] = await Promise.all([
    queryIdleState(Math.floor(message.idleThresholdSeconds)),
    queryRelevantTabs(origin),
    computeActiveRelevantTab(origin),
    getSessionCookies(origin),
  ]);
  const initialCookie = cookies[0];
  const state: DutyState = {
    version: 1,
    active: true,
    origin,
    startedAt: now,
    plannedEndAt: now + message.durationMs,
    idleThresholdSeconds: Math.floor(message.idleThresholdSeconds),
    idleState,
    relevantTabIds: relevantTabs.flatMap((tab) => (tab.id === undefined ? [] : [tab.id])),
    activeRelevantTab,
    cookiePresent: initialCookie !== undefined,
    snapshot: initialCookie === undefined ? undefined : snapshotCookie(initialCookie, origin),
    lastMetricAt: now,
    cookiePresentMs: 0,
    activeExposureMs: 0,
    unnecessaryExposureMs: 0,
    events: [],
    nextSequence: 1,
  };
  addEvent(
    state,
    "measurement_started",
    `duration_ms=${message.durationMs}; idle_threshold_s=${state.idleThresholdSeconds}; origin=${origin}`,
  );
  addEvent(
    state,
    "reconciliation",
    `source=measurement_start; tabs=${state.relevantTabIds.length}; cookie_present=${state.cookiePresent}; idle_state=${state.idleState}`,
  );
  await saveState(state);
  chrome.alarms.create(DUTY_ALARM, { when: state.plannedEndAt });
  return publicState(state);
}

async function observeLogin(state: DutyState, tabId: number): Promise<void> {
  const protectedResult = await firstPartyCommand<{ state?: string }>(tabId, "protected");
  if (protectedResult.state !== "authenticated") {
    throw new Error(`login observation expected authenticated, got ${String(protectedResult.state)}`);
  }
  const cookies = await getSessionCookies(state.origin);
  if (cookies.length !== 1 || cookies[0] === undefined) {
    throw new Error(`login observation expected exactly one session cookie, got ${cookies.length}`);
  }

  advanceMetrics(state, Date.now());
  state.cookiePresent = true;
  state.snapshot = snapshotCookie(cookies[0], state.origin);
  state.relevantTabIds = uniqueNumbers([...state.relevantTabIds, tabId]);
  state.activeRelevantTab = await computeActiveRelevantTab(state.origin);
  addEvent(state, "login_detected", `tab_id=${tabId}; store_id=${cookies[0].storeId}`);
  await saveState(state);
}

async function handleRelevantTabReady(state: DutyState, tabId: number): Promise<void> {
  if (!state.active || state.idleState !== "active") {
    return;
  }
  const cookies = await getSessionCookies(state.origin);
  if (cookies.length > 0) {
    if (!state.cookiePresent) {
      advanceMetrics(state, Date.now());
      state.cookiePresent = true;
      const cookie = cookies[0];
      if (cookie !== undefined) {
        state.snapshot = snapshotCookie(cookie, state.origin);
      }
      addEvent(state, "reconciliation", "tab_ready_found_cookie_while_state_was_absent");
      await saveState(state);
    }
    return;
  }
  if (state.snapshot !== undefined) {
    await restore(state, `relevant_tab_opened; tab_id=${tabId}`, tabId);
    return;
  }

  try {
    const protectedResult = await firstPartyCommand<{ state?: string }>(tabId, "protected");
    if (protectedResult.state === "authenticated") {
      await observeLogin(state, tabId);
    }
  } catch {
    // The content script may not be ready on the first onUpdated signal. The harness explicitly
    // calls observe-login after login, so this best-effort detection is not a correctness gate.
  }
}

async function evict(state: DutyState, reason: string): Promise<void> {
  if (!state.active) {
    return;
  }
  const startedAt = performance.now();
  try {
    const cookies = await getSessionCookies(state.origin);
    const firstCookie = cookies[0];
    if (firstCookie !== undefined) {
      state.snapshot = snapshotCookie(firstCookie, state.origin);
    }
    state.mutationGuard = { expectedPresent: false, until: Date.now() + 2_000 };
    await saveState(state);
    for (const cookie of cookies) {
      await removeCookie(cookieRemovalDetails(cookie, state.origin));
    }
    const remaining = await getSessionCookies(state.origin);
    if (remaining.length !== 0) {
      throw new Error(`${remaining.length} session cookie(s) remain after removal`);
    }

    advanceMetrics(state, Date.now());
    state.cookiePresent = false;
    addEvent(
      state,
      "eviction",
      `reason=${reason}; removed_count=${cookies.length}`,
      performance.now() - startedAt,
    );
    await saveState(state);
  } catch (error: unknown) {
    addEvent(
      state,
      "failed_eviction",
      `reason=${reason}; error=${errorMessage(error)}`,
      performance.now() - startedAt,
    );
    await saveState(state);
  }
}

async function restore(state: DutyState, reason: string, preferredTabId?: number): Promise<void> {
  if (!state.active || state.snapshot === undefined) {
    return;
  }
  const existing = await getSessionCookies(state.origin);
  if (existing.length > 0) {
    advanceMetrics(state, Date.now());
    state.cookiePresent = true;
    await saveState(state);
    return;
  }

  const startedAt = performance.now();
  state.mutationGuard = { expectedPresent: true, until: Date.now() + 2_000 };
  await saveState(state);
  const restored = await setCookie(state.snapshot.setDetails);
  const tabId = preferredTabId ?? state.relevantTabIds[0];
  if (tabId === undefined) {
    throw new Error("restore completed without a relevant tab for health verification");
  }
  const protectedResult = await retryFirstPartyProtected(tabId);
  if (protectedResult.state !== "authenticated") {
    throw new Error(`restored cookie failed health check: ${String(protectedResult.state)}`);
  }

  advanceMetrics(state, Date.now());
  state.cookiePresent = true;
  state.snapshot = snapshotCookie(restored, state.origin);
  addEvent(
    state,
    "inject",
    `reason=${reason}; tab_id=${tabId}`,
    performance.now() - startedAt,
  );
  await saveState(state);
}

async function refreshActiveUse(_source: string): Promise<void> {
  const state = await loadState();
  if (!state?.active) {
    return;
  }
  const activeRelevantTab = await computeActiveRelevantTab(state.origin);
  if (activeRelevantTab === state.activeRelevantTab) {
    return;
  }
  advanceMetrics(state, Date.now());
  state.activeRelevantTab = activeRelevantTab;
  await saveState(state);
}

async function reconcile(source: string): Promise<void> {
  const state = await loadState();
  if (!state?.active) {
    return;
  }
  const [tabs, idleState, activeRelevantTab, cookies] = await Promise.all([
    queryRelevantTabs(state.origin),
    queryIdleState(state.idleThresholdSeconds),
    computeActiveRelevantTab(state.origin),
    getSessionCookies(state.origin),
  ]);
  const now = Date.now();
  advanceMetrics(state, now);
  const previousPresence = state.cookiePresent;
  state.relevantTabIds = tabs.flatMap((tab) => (tab.id === undefined ? [] : [tab.id]));
  state.idleState = idleState;
  state.activeRelevantTab = activeRelevantTab;
  state.cookiePresent = cookies.length > 0;
  const firstCookie = cookies[0];
  if (firstCookie !== undefined) {
    state.snapshot = snapshotCookie(firstCookie, state.origin);
  }
  addEvent(
    state,
    "reconciliation",
    `source=${source}; tabs=${state.relevantTabIds.length}; cookie_present=${state.cookiePresent}; previous_cookie_present=${previousPresence}; idle_state=${idleState}`,
  );
  if (!previousPresence && state.cookiePresent && !guardMatches(state, true, now)) {
    addEvent(state, "site_cookie_recreated", `source=${source}`);
  }
  await saveState(state);
  if (state.cookiePresent && (idleState !== "active" || state.relevantTabIds.length === 0)) {
    await evict(state, `reconciliation_${source}`);
  }
}

async function stopMeasurement(state: DutyState, reason: string): Promise<void> {
  const now = Date.now();
  advanceMetrics(state, now);
  state.active = false;
  state.stoppedAt = now;
  state.mutationGuard = undefined;
  state.snapshot = undefined;
  addEvent(state, "measurement_stopped", `reason=${reason}`);
  await saveState(state);
  chrome.alarms.clear(DUTY_ALARM);
}

function advanceMetrics(state: DutyState, now: number): void {
  const end = Math.min(now, state.plannedEndAt);
  const elapsed = Math.max(0, end - state.lastMetricAt);
  if (state.cookiePresent) {
    state.cookiePresentMs += elapsed;
    if (state.activeRelevantTab && state.idleState === "active") {
      state.activeExposureMs += elapsed;
    } else {
      state.unnecessaryExposureMs += elapsed;
    }
  }
  state.lastMetricAt = Math.max(state.lastMetricAt, end);
}

function addEvent(
  state: DutyState,
  event: DutyEventName,
  detail: string,
  durationMs?: number,
): void {
  const entry: DutyEvent = {
    sequence: state.nextSequence++,
    at: Date.now(),
    event,
    detail,
  };
  if (durationMs !== undefined) {
    entry.durationMs = Math.round(durationMs * 10) / 10;
  }
  state.events.push(entry);
  if (state.events.length > MAX_EVENTS) {
    state.events.splice(0, state.events.length - MAX_EVENTS);
  }
}

function publicState(state: DutyState): Record<string, unknown> {
  const endedAt = state.stoppedAt ?? Math.min(Date.now(), state.plannedEndAt);
  const browserOpenTimeMs = Math.max(0, endedAt - state.startedAt);
  const exposureDutyCycle = safeRatio(state.cookiePresentMs, browserOpenTimeMs);
  const unnecessaryExposureRatio = safeRatio(state.unnecessaryExposureMs, browserOpenTimeMs);
  return {
    active: state.active,
    origin: state.origin,
    startedAt: state.startedAt,
    plannedEndAt: state.plannedEndAt,
    stoppedAt: state.stoppedAt,
    idleThresholdSeconds: state.idleThresholdSeconds,
    idleState: state.idleState,
    relevantTabIds: [...state.relevantTabIds],
    activeRelevantTab: state.activeRelevantTab,
    cookiePresent: state.cookiePresent,
    snapshotAvailable: state.snapshot !== undefined,
    browserOpenTimeMs,
    cookiePresentMs: Math.round(state.cookiePresentMs),
    activeExposureMs: Math.round(state.activeExposureMs),
    unnecessaryExposureMs: Math.round(state.unnecessaryExposureMs),
    exposureDutyCycle,
    unnecessaryExposureRatio,
    events: state.events.map((event) => ({ ...event })),
  };
}

function safeRatio(numerator: number, denominator: number): number {
  return denominator === 0 ? 0 : numerator / denominator;
}

function requireActiveState(state: DutyState | undefined): DutyState {
  if (state === undefined || !state.active) {
    throw new Error("no active duty-cycle measurement");
  }
  return state;
}

function snapshotCookie(cookie: chrome.cookies.Cookie, origin: string): CookieSnapshot {
  const details: chrome.cookies.CookieSetDetails = {
    url: `${origin}${cookie.path}`,
    name: cookie.name,
    value: cookie.value,
    path: cookie.path,
    secure: cookie.secure,
    httpOnly: cookie.httpOnly,
    sameSite: cookie.sameSite,
    storeId: cookie.storeId,
  };
  if (!cookie.hostOnly) {
    details.domain = cookie.domain;
  }
  if (!cookie.session && cookie.expirationDate !== undefined) {
    details.expirationDate = cookie.expirationDate;
  }
  if (cookie.partitionKey !== undefined) {
    details.partitionKey = cookie.partitionKey;
  }
  return { setDetails: details, capturedAt: Date.now() };
}

function cookieRemovalDetails(
  cookie: chrome.cookies.Cookie,
  origin: string,
): chrome.cookies.CookieDetails {
  return {
    url: `${origin}${cookie.path}`,
    name: cookie.name,
    storeId: cookie.storeId,
    partitionKey: cookie.partitionKey,
  };
}

function cookieBelongsToExperiment(cookie: chrome.cookies.Cookie, state: DutyState): boolean {
  const host = new URL(state.origin).hostname;
  return cookie.name === COOKIE_NAME && cookie.domain.replace(/^\./, "") === host;
}

function guardMatches(state: DutyState, present: boolean, now: number): boolean {
  return (
    state.mutationGuard !== undefined &&
    state.mutationGuard.until >= now &&
    state.mutationGuard.expectedPresent === present
  );
}

function normalizeOrigin(origin: string): string {
  const url = new URL(origin);
  if (url.protocol !== "http:" || url.port !== "43118") {
    throw new Error(`unsupported duty-cycle origin: ${origin}`);
  }
  if (url.hostname !== "localhost" && url.hostname !== "127.0.0.1") {
    throw new Error(`unsupported duty-cycle host: ${url.hostname}`);
  }
  return url.origin;
}

function isRelevantUrl(url: string | undefined, origin: string): boolean {
  if (url === undefined) {
    return false;
  }
  try {
    return new URL(url).origin === origin;
  } catch {
    return false;
  }
}

function uniqueNumbers(values: number[]): number[] {
  return [...new Set(values)];
}

function isDutyMessage(value: unknown): value is DutyMessage {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Partial<DutyMessage>;
  return (
    candidate.target === "fcp-duty-cycle" &&
    (candidate.command === "start" ||
      candidate.command === "get-state" ||
      candidate.command === "stop" ||
      candidate.command === "observe-login")
  );
}

async function loadState(): Promise<DutyState | undefined> {
  const items = await storageGet(DUTY_STATE_KEY);
  const value = items[DUTY_STATE_KEY];
  return isDutyState(value) ? value : undefined;
}

function isDutyState(value: unknown): value is DutyState {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Partial<DutyState>;
  return candidate.version === 1 && typeof candidate.active === "boolean";
}

function saveState(state: DutyState): Promise<void> {
  return storageSet({ [DUTY_STATE_KEY]: state });
}

function storageGet(key: string): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    chrome.storage.session.get(key, (items) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "storage.session.get failed"));
        return;
      }
      resolve(items);
    });
  });
}

function storageSet(items: Record<string, unknown>): Promise<void> {
  return new Promise((resolve, reject) => {
    chrome.storage.session.set(items, () => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "storage.session.set failed"));
        return;
      }
      resolve();
    });
  });
}

function queryRelevantTabs(origin: string): Promise<chrome.tabs.Tab[]> {
  return new Promise((resolve, reject) => {
    chrome.tabs.query({ url: [`${origin}/*`] }, (tabs) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "tabs.query failed"));
        return;
      }
      resolve(tabs);
    });
  });
}

function computeActiveRelevantTab(origin: string): Promise<boolean> {
  return new Promise((resolve, reject) => {
    chrome.windows.getLastFocused({ populate: true }, (window) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "windows.getLastFocused failed"));
        return;
      }
      const activeTab = window.tabs?.find((tab) => tab.active === true);
      resolve(window.focused && isRelevantUrl(activeTab?.url, origin));
    });
  });
}

function queryIdleState(seconds: number): Promise<chrome.idle.IdleState> {
  return new Promise((resolve) => {
    chrome.idle.queryState(seconds, resolve);
  });
}

function getSessionCookies(origin: string): Promise<chrome.cookies.Cookie[]> {
  return new Promise((resolve, reject) => {
    chrome.cookies.getAll({ url: `${origin}/`, name: COOKIE_NAME }, (cookies) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "cookies.getAll failed"));
        return;
      }
      resolve(cookies);
    });
  });
}

function setCookie(details: chrome.cookies.CookieSetDetails): Promise<chrome.cookies.Cookie> {
  return new Promise((resolve, reject) => {
    chrome.cookies.set(details, (cookie) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined || cookie === undefined) {
        reject(new Error(lastError?.message ?? "cookies.set returned no cookie"));
        return;
      }
      resolve(cookie);
    });
  });
}

function removeCookie(details: chrome.cookies.CookieDetails): Promise<void> {
  return new Promise((resolve, reject) => {
    chrome.cookies.remove(details, (removed) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined || removed === null) {
        reject(new Error(lastError?.message ?? `cookies.remove did not remove ${details.name}`));
        return;
      }
      resolve();
    });
  });
}

async function retryFirstPartyProtected(tabId: number): Promise<{ state?: string }> {
  let lastError: unknown;
  for (let attempt = 0; attempt < 10; attempt += 1) {
    try {
      return await firstPartyCommand<{ state?: string }>(tabId, "protected");
    } catch (error: unknown) {
      lastError = error;
      await delay(100);
    }
  }
  throw new Error(`protected health check did not become ready: ${errorMessage(lastError)}`);
}

function firstPartyCommand<T>(tabId: number, command: string): Promise<T> {
  return new Promise((resolve, reject) => {
    chrome.tabs.sendMessage(
      tabId,
      { target: "fcp-session-probe", command },
      (response: unknown) => {
        const lastError = chrome.runtime.lastError;
        if (lastError !== undefined) {
          reject(new Error(lastError.message ?? `content command ${command} failed`));
          return;
        }
        if (!isContentResponse(response)) {
          reject(new Error(`content command ${command} returned an invalid response`));
          return;
        }
        if (!response.ok) {
          reject(new Error(response.error ?? `content command ${command} failed`));
          return;
        }
        resolve(response.data as T);
      },
    );
  });
}

function isContentResponse(
  value: unknown,
): value is { ok: boolean; data?: unknown; error?: string } {
  return typeof value === "object" && value !== null && typeof (value as { ok?: unknown }).ok === "boolean";
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
