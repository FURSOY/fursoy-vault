const TEST_ORIGIN = "http://localhost:43118";
const LOOPBACK_IP_ORIGIN = "http://127.0.0.1:43118";
const COOKIE_NAME = "FCP-session-probe";
const DOCWRITE_DIAGNOSTIC_COOKIE_NAME = "FCP-docwrite-diagnostic";
const DUMMY_USERNAME = "probe-user";
const DUMMY_PASSWORD = "probe-password";
const DEFAULT_CYCLES = 10;
const MIN_CYCLES = 1;
const MAX_CYCLES = 100;

interface SessionResponse {
  authenticated: boolean;
  state: "authenticated" | "logged_out";
  reason?: "missing_cookie" | "invalid_session";
}

interface LoginResponse {
  authenticated: boolean;
  state: "authenticated";
}

interface LogoutResponse {
  invalidated: boolean;
  state: "logged_out";
}

interface DiagnosticsResponse {
  activeSessionCount: number;
  securityAlarmCount: number;
  sessionSetCookieHeaderRedacted: string;
  requestCookieDiagnostics: ServerRequestCookieDiagnostic[];
}

interface ServerRequestCookieDiagnostic {
  sequence: number;
  method: string;
  path: string;
  host: string;
  cookieHeaderPresent: boolean;
  cookieNames: string[];
}

interface ResultRow {
  cycle: string;
  check: string;
  expected: string;
  actual: string;
  passed: boolean;
  durationMs: number;
}

interface CycleOutcome {
  restoreSucceeded: boolean;
  wrongLogout: boolean;
  serverSessionInvalidated: boolean;
}

type FirstPartyCommand =
  | "ping"
  | "login"
  | "protected"
  | "logout"
  | "diagnostics"
  | "page-diagnostic"
  | "docwrite-diagnostic"
  | "docwrite-cleanup"
  | "reset";

interface FirstPartyResponse<T> {
  ok: boolean;
  data?: T;
  error?: string;
}

interface RawCookieDiagnostic {
  domain: string;
  name: string;
  storeId: string;
  partitionKey: chrome.cookies.CookiePartitionKey | "<absent>";
  path: string;
}

interface RawCookieStoreDiagnostic {
  id: string;
  tabIds: number[];
}

interface RawTabDiagnostic {
  id: number;
  url: string;
  windowId: number;
  incognito: boolean;
}

interface RawPageDiagnostic {
  origin: string;
  href: string;
  documentCookieNames: string[];
}

interface DocWriteDiagnostic {
  cookieName: string;
  documentCookieNames: string[];
  visibleInDocumentCookie: boolean;
}

interface ExtendedDiagnostic {
  cookies: RawCookieDiagnostic[];
  cookiesAfter250Ms: RawCookieDiagnostic[];
  cookieStores: RawCookieStoreDiagnostic[];
  testTab: RawTabDiagnostic;
  page: RawPageDiagnostic;
  docWrite: DocWriteDiagnostic;
  docWriteVisibleToCookiesApiImmediate: boolean;
  docWriteVisibleToCookiesApiAfter250Ms: boolean;
  sessionSetCookieHeaderRedacted: string;
  serverRequestCookieDiagnostics: ServerRequestCookieDiagnostic[];
}

const cycleInput = requireElement<HTMLInputElement>("cycle-count");
const runButton = requireElement<HTMLButtonElement>("run-probe");
const copyButton = requireElement<HTMLButtonElement>("copy-report");
const statusElement = requireElement<HTMLElement>("status");
const resultBody = requireElement<HTMLTableSectionElement>("result-body");

let lastReport = "";

runButton.addEventListener("click", () => void runSuite());
copyButton.addEventListener("click", () => void copyReport());

async function runSuite(): Promise<void> {
  let cycleCount = DEFAULT_CYCLES;
  runButton.disabled = true;
  cycleInput.disabled = true;
  copyButton.disabled = true;
  resultBody.replaceChildren();
  statusElement.textContent = "Preparing one synthetic server-backed session…";

  const startedAt = new Date();
  const rows: ResultRow[] = [];
  const outcomes: CycleOutcome[] = [];
  let storeId = "unknown";
  let testTabId: number | undefined;
  let extendedDiagnostic: ExtendedDiagnostic | undefined;
  let loopbackIpDiagnostic: ExtendedDiagnostic | undefined;
  let finalSnapshot: chrome.cookies.Cookie[] = [];

  try {
    cycleCount = parseCycleCount(cycleInput.value);
    await healthCheck(TEST_ORIGIN);
    await healthCheck(LOOPBACK_IP_ORIGIN);
    await runLegacyExtensionFetchDiagnostic(rows);
    loopbackIpDiagnostic = await runLoopbackIpDiagnostic(rows);
    testTabId = await createFirstPartyTestTab(TEST_ORIGIN);
    storeId = await findStoreIdForTab(testTabId);
    await resetPreviousProbeSession(storeId, testTabId);

    const loginStart = performance.now();
    const login = await firstPartyRequest<LoginResponse>(testTabId, "login");
    rows.push(
      comparison("setup", "single login", true, login.authenticated, elapsed(loginStart)),
    );

    extendedDiagnostic = await collectExtendedDiagnostic(testTabId);
    appendExtendedDiagnosticRows(rows, extendedDiagnostic, "localhost first-party diagnostic");
    await appendUnfilteredCookieDiagnostic(rows, "first-party");

    const initialSession = await protectedRequest(testTabId);
    rows.push(
      comparison(
        "setup",
        "session authenticated after login",
        "authenticated",
        initialSession.state,
        0,
      ),
    );
    if (!initialSession.authenticated) {
      throw new Error(`login did not establish an authenticated session (${initialSession.reason})`);
    }
    const initialServerDiagnostics = await firstPartyRequest<DiagnosticsResponse>(
      testTabId,
      "diagnostics",
    );
    extendedDiagnostic.serverRequestCookieDiagnostics =
      initialServerDiagnostics.requestCookieDiagnostics.map(copyServerRequestDiagnostic);
    extendedDiagnostic.sessionSetCookieHeaderRedacted =
      initialServerDiagnostics.sessionSetCookieHeaderRedacted;
    appendServerRequestDiagnosticRows(
      rows,
      extendedDiagnostic.serverRequestCookieDiagnostics,
      extendedDiagnostic.sessionSetCookieHeaderRedacted,
      "localhost server diagnostic",
    );

    for (let cycle = 1; cycle <= cycleCount; cycle += 1) {
      statusElement.textContent = `Running cycle ${cycle}/${cycleCount} on the same session…`;
      const result = await runCycle(cycle, storeId, testTabId, rows);
      outcomes.push(result.outcome);
      finalSnapshot = result.snapshot;
      renderRows(rows);
    }

    statusElement.textContent = "Running server-side logout invalidation control…";
    await runLogoutInvalidationControl(finalSnapshot, storeId, testTabId, rows);

    const diagnostics = await firstPartyRequest<DiagnosticsResponse>(testTabId, "diagnostics");
    rows.push(
      comparison(
        "summary",
        "server security alarms",
        0,
        diagnostics.securityAlarmCount,
        0,
      ),
    );
    rows.push(
      comparison("summary", "active sessions after cleanup", 0, diagnostics.activeSessionCount, 0),
    );

    renderRows(rows);
    lastReport = formatReport(
      startedAt,
      cycleCount,
      storeId,
      rows,
      outcomes,
      diagnostics,
      extendedDiagnostic,
      loopbackIpDiagnostic,
    );
    copyButton.disabled = false;

    const successfulRestores = outcomes.filter((outcome) => outcome.restoreSucceeded).length;
    statusElement.textContent = `Completed: ${successfulRestores}/${cycleCount} session restores succeeded. Report ready.`;
  } catch (error) {
    const message = errorMessage(error);
    rows.push(failure("suite", "fatal error", "suite completes", message, 0));
    renderRows(rows);
    lastReport = formatReport(
      startedAt,
      cycleCount,
      storeId,
      rows,
      outcomes,
      undefined,
      extendedDiagnostic,
      loopbackIpDiagnostic,
    );
    copyButton.disabled = false;
    statusElement.textContent = `Probe failed: ${message}`;
  } finally {
    await bestEffortCleanup(testTabId);
    if (testTabId !== undefined) {
      await removeTabIgnoringErrors(testTabId);
    }
    runButton.disabled = false;
    cycleInput.disabled = false;
  }
}

async function runCycle(
  cycle: number,
  storeId: string,
  testTabId: number,
  rows: ResultRow[],
): Promise<{ outcome: CycleOutcome; snapshot: chrome.cookies.Cookie[] }> {
  const label = String(cycle);

  const snapshotStart = performance.now();
  const snapshot = await getSessionCookies(storeId);
  rows.push(
    comparison(label, "snapshot cookie count", 1, snapshot.length, elapsed(snapshotStart)),
  );
  if (snapshot.length !== 1) {
    throw new Error(`cycle ${cycle}: expected exactly one session cookie, got ${snapshot.length}`);
  }

  const cookie = snapshot[0];
  if (cookie === undefined) {
    throw new Error(`cycle ${cycle}: snapshot unexpectedly empty`);
  }

  const evictStart = performance.now();
  const removed = await removeCookie(cookieDetails(cookie));
  const afterEvictCookies = await getSessionCookies(storeId);
  const evictionPassed = removed !== null && afterEvictCookies.length === 0;
  rows.push(
    comparison(
      label,
      "cookie evicted",
      "removed and absent",
      evictionPassed ? "removed and absent" : "eviction incomplete",
      elapsed(evictStart),
    ),
  );

  const logoutCheckStart = performance.now();
  const loggedOut = await protectedRequest(testTabId);
  rows.push(
    comparison(
      label,
      "protected endpoint after eviction",
      "logged_out/missing_cookie",
      `${loggedOut.state}/${loggedOut.reason ?? "none"}`,
      elapsed(logoutCheckStart),
    ),
  );

  const restoreStart = performance.now();
  const restoredCookie = await setCookie(toSetDetails(cookie));
  const restoreApiSucceeded = restoredCookie.name === COOKIE_NAME;
  rows.push(
    comparison(
      label,
      "cookie restore API",
      "cookie restored",
      restoreApiSucceeded ? "cookie restored" : "unexpected cookie",
      elapsed(restoreStart),
    ),
  );

  const authenticatedStart = performance.now();
  const restoredSession = await protectedRequest(testTabId);
  rows.push(
    comparison(
      label,
      "protected endpoint after restore",
      "authenticated",
      `${restoredSession.state}${restoredSession.reason ? `/${restoredSession.reason}` : ""}`,
      elapsed(authenticatedStart),
    ),
  );

  const restoreSucceeded = restoreApiSucceeded && restoredSession.authenticated;
  const serverSessionInvalidated = restoredSession.reason === "invalid_session";

  rows.push(comparison(label, "cycle restore succeeded", true, restoreSucceeded, 0));
  rows.push(comparison(label, "wrong logout", false, !restoredSession.authenticated, 0));
  rows.push(
    comparison(label, "server session invalidated", false, serverSessionInvalidated, 0),
  );

  return {
    snapshot,
    outcome: {
      restoreSucceeded,
      wrongLogout: !restoredSession.authenticated,
      serverSessionInvalidated,
    },
  };
}

async function runLogoutInvalidationControl(
  snapshot: chrome.cookies.Cookie[],
  storeId: string,
  testTabId: number,
  rows: ResultRow[],
): Promise<void> {
  if (snapshot.length !== 1 || snapshot[0] === undefined) {
    throw new Error("logout control has no final session snapshot");
  }
  const cookie = snapshot[0];

  const logoutStart = performance.now();
  const logout = await firstPartyRequest<LogoutResponse>(testTabId, "logout");
  rows.push(
    comparison(
      "control",
      "server-side logout invalidated session",
      true,
      logout.invalidated,
      elapsed(logoutStart),
    ),
  );

  await setCookie(toSetDetails(cookie));
  const staleRestore = await protectedRequest(testTabId);
  rows.push(
    comparison(
      "control",
      "stale cookie after server logout",
      "logged_out/invalid_session",
      `${staleRestore.state}/${staleRestore.reason ?? "none"}`,
      0,
    ),
  );

  await removeCookie(cookieDetails(cookie));
  const remaining = await getSessionCookies(storeId);
  rows.push(
    comparison(
      "control",
      "final probe cleanup",
      "absent",
      remaining.length === 0 ? "absent" : `${remaining.length} cookie(s) remain`,
      0,
    ),
  );
}

function toSetDetails(cookie: chrome.cookies.Cookie): chrome.cookies.CookieSetDetails {
  const details: chrome.cookies.CookieSetDetails = {
    url: cookieUrl(cookie),
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
  return details;
}

function cookieUrl(cookie: chrome.cookies.Cookie): string {
  const scheme = cookie.secure ? "https" : "http";
  const host = cookie.domain.replace(/^\./, "");
  return `${scheme}://${host}:43118${cookie.path}`;
}

function cookieDetails(cookie: chrome.cookies.Cookie): chrome.cookies.CookieDetails {
  return {
    url: cookieUrl(cookie),
    name: cookie.name,
    storeId: cookie.storeId,
    partitionKey: cookie.partitionKey,
  };
}

async function resetPreviousProbeSession(storeId: string, testTabId: number): Promise<void> {
  await firstPartyRequest<{ clearedSessionCount: number }>(testTabId, "reset");
  const cookies = (await getCookiesByUrlOnly()).filter((cookie) => cookie.name === COOKIE_NAME);
  for (const cookie of cookies) {
    await removeCookie(cookieDetails(cookie));
  }
  const remaining = await getSessionCookies(storeId);
  if (remaining.length !== 0) {
    throw new Error(`probe cleanup left ${remaining.length} filtered cookie(s)`);
  }
}

async function bestEffortCleanup(testTabId: number | undefined): Promise<void> {
  try {
    if (testTabId === undefined) {
      await apiRequest<{ clearedSessionCount: number }>("/api/reset", { method: "POST" });
    } else {
      await firstPartyRequest<{ clearedSessionCount: number }>(testTabId, "reset");
    }
  } catch {
    // The local server may have stopped; browser-side cleanup is still attempted below.
  }
  try {
    const cookies = (await getCookiesByUrlOnly()).filter((cookie) => cookie.name === COOKIE_NAME);
    for (const cookie of cookies) {
      await removeCookie(cookieDetails(cookie));
    }
  } catch {
    // Cleanup is best-effort only after the primary result has already been recorded.
  }
}

async function runLoopbackIpDiagnostic(rows: ResultRow[]): Promise<ExtendedDiagnostic> {
  let tabId: number | undefined;
  try {
    tabId = await createFirstPartyTestTab(LOOPBACK_IP_ORIGIN);
    const storeId = await findStoreIdForTab(tabId);
    await firstPartyRequest<{ clearedSessionCount: number }>(tabId, "reset");
    await removeAllProbeCookies(LOOPBACK_IP_ORIGIN);

    const loginStart = performance.now();
    const login = await firstPartyRequest<LoginResponse>(tabId, "login");
    rows.push(
      comparison(
        "127.0.0.1 diagnostic",
        "single first-party login",
        true,
        login.authenticated,
        elapsed(loginStart),
      ),
    );

    const diagnostic = await collectExtendedDiagnostic(tabId);
    appendExtendedDiagnosticRows(rows, diagnostic, "127.0.0.1 first-party diagnostic");
    rows.push(
      informational("127.0.0.1 diagnostic", "test tab cookie store ID", storeId),
    );

    const protectedStart = performance.now();
    const session = await protectedRequest(tabId);
    rows.push(
      comparison(
        "127.0.0.1 diagnostic",
        "protected state after login",
        "authenticated",
        session.state,
        elapsed(protectedStart),
      ),
    );

    const serverDiagnostics = await firstPartyRequest<DiagnosticsResponse>(tabId, "diagnostics");
    diagnostic.serverRequestCookieDiagnostics =
      serverDiagnostics.requestCookieDiagnostics.map(copyServerRequestDiagnostic);
    diagnostic.sessionSetCookieHeaderRedacted = serverDiagnostics.sessionSetCookieHeaderRedacted;
    appendServerRequestDiagnosticRows(
      rows,
      diagnostic.serverRequestCookieDiagnostics,
      diagnostic.sessionSetCookieHeaderRedacted,
      "127.0.0.1 server diagnostic",
    );
    return diagnostic;
  } finally {
    if (tabId !== undefined) {
      try {
        await firstPartyRequest<{ clearedSessionCount: number }>(tabId, "reset");
      } catch {
        // The comparison result is retained even if best-effort server cleanup cannot run.
      }
      try {
        await removeAllProbeCookies(LOOPBACK_IP_ORIGIN);
      } catch {
        // The comparison result is retained even if best-effort browser cleanup cannot run.
      }
      await removeTabIgnoringErrors(tabId);
    }
  }
}

async function collectExtendedDiagnostic(testTabId: number): Promise<ExtendedDiagnostic> {
  const docWrite = await firstPartyRequest<DocWriteDiagnostic>(testTabId, "docwrite-diagnostic");
  try {
    const cookiesAfter250MsPromise = (async () => {
      await delay(250);
      return getAllCookiesWithoutFilters();
    })();
    const [cookies, cookiesAfter250Ms, cookieStores, tab, page] = await Promise.all([
      getAllCookiesWithoutFilters(),
      cookiesAfter250MsPromise,
      getAllCookieStores(),
      getTab(testTabId),
      firstPartyRequest<RawPageDiagnostic>(testTabId, "page-diagnostic"),
    ]);

    if (tab.id === undefined) {
      throw new Error("tabs.get returned the test tab without an ID");
    }

    return {
      cookies: cookies.map(toRawCookieDiagnostic),
      cookiesAfter250Ms: cookiesAfter250Ms.map(toRawCookieDiagnostic),
      cookieStores: cookieStores.map((store) => ({ id: store.id, tabIds: [...store.tabIds] })),
      testTab: {
        id: tab.id,
        url: tab.url ?? "<absent>",
        windowId: tab.windowId,
        incognito: tab.incognito,
      },
      page,
      docWrite,
      docWriteVisibleToCookiesApiImmediate: cookies.some(
        (cookie) => cookie.name === DOCWRITE_DIAGNOSTIC_COOKIE_NAME,
      ),
      docWriteVisibleToCookiesApiAfter250Ms: cookiesAfter250Ms.some(
        (cookie) => cookie.name === DOCWRITE_DIAGNOSTIC_COOKIE_NAME,
      ),
      sessionSetCookieHeaderRedacted: "<not_captured>",
      serverRequestCookieDiagnostics: [],
    };
  } finally {
    await firstPartyRequest<{ cleaned: boolean }>(testTabId, "docwrite-cleanup");
  }
}

function toRawCookieDiagnostic(cookie: chrome.cookies.Cookie): RawCookieDiagnostic {
  return {
    domain: cookie.domain,
    name: cookie.name,
    storeId: cookie.storeId,
    partitionKey: cookie.partitionKey ?? "<absent>",
    path: cookie.path,
  };
}

function copyServerRequestDiagnostic(
  diagnostic: ServerRequestCookieDiagnostic,
): ServerRequestCookieDiagnostic {
  return { ...diagnostic, cookieNames: [...diagnostic.cookieNames] };
}

function appendExtendedDiagnosticRows(
  rows: ResultRow[],
  diagnostic: ExtendedDiagnostic,
  section: string,
): void {
  rows.push(
    informational(
      section,
      "getAll({}) cookie count",
      String(diagnostic.cookies.length),
    ),
  );
  diagnostic.cookies.forEach((cookie, index) => {
    rows.push(
      informational(
        section,
        `getAll({}) cookie[${index}]`,
        JSON.stringify(cookie),
      ),
    );
  });
  rows.push(
    informational(
      section,
      "getAll({}) after 250ms delay cookie count",
      String(diagnostic.cookiesAfter250Ms.length),
    ),
  );
  diagnostic.cookiesAfter250Ms.forEach((cookie, index) => {
    rows.push(
      informational(
        section,
        `getAll({}) after 250ms cookie[${index}]`,
        JSON.stringify(cookie),
      ),
    );
  });
  rows.push(
    comparison(
      section,
      "document.cookie write/read sanity",
      true,
      diagnostic.docWrite.visibleInDocumentCookie,
      0,
    ),
  );
  rows.push(
    comparison(
      section,
      "document-written cookie visible in immediate getAll({})",
      true,
      diagnostic.docWriteVisibleToCookiesApiImmediate,
      0,
    ),
  );
  rows.push(
    comparison(
      section,
      "document-written cookie visible in getAll({}) after 250ms",
      true,
      diagnostic.docWriteVisibleToCookiesApiAfter250Ms,
      0,
    ),
  );
  rows.push(
    informational(
      section,
      "getAllCookieStores() store count",
      String(diagnostic.cookieStores.length),
    ),
  );
  diagnostic.cookieStores.forEach((store, index) => {
    rows.push(
      informational(
        section,
        `cookie store[${index}]`,
        JSON.stringify(store),
      ),
    );
  });
  rows.push(
    informational(
      section,
      "tabs.get(testTabId)",
      JSON.stringify(diagnostic.testTab),
    ),
  );
  rows.push(
    informational(
      section,
      "content page/document.cookie names",
      JSON.stringify(diagnostic.page),
    ),
  );
  renderRows(rows);
}

function appendServerRequestDiagnosticRows(
  rows: ResultRow[],
  diagnostics: readonly ServerRequestCookieDiagnostic[],
  sessionSetCookieHeaderRedacted: string,
  section: string,
): void {
  rows.push(
    informational(
      section,
      "session Set-Cookie header (value redacted)",
      sessionSetCookieHeaderRedacted,
    ),
  );
  rows.push(
    informational(
      section,
      "recent /api/login and /api/protected request count",
      String(diagnostics.length),
    ),
  );
  diagnostics.forEach((diagnostic, index) => {
    rows.push(
      informational(
        section,
        `request[${index}] Cookie header evidence`,
        JSON.stringify(diagnostic),
      ),
    );
  });
  renderRows(rows);
}

async function runLegacyExtensionFetchDiagnostic(rows: ResultRow[]): Promise<void> {
  await apiRequest<{ clearedSessionCount: number }>("/api/reset", { method: "POST" });
  await removeAllProbeCookies();

  const login = await apiRequest<LoginResponse>("/api/login", {
    method: "POST",
    body: JSON.stringify({ username: DUMMY_USERNAME, password: DUMMY_PASSWORD }),
  });
  rows.push(
    comparison(
      "legacy diagnostic",
      "extension-fetch login",
      true,
      login.authenticated,
      0,
    ),
  );
  const session = await apiRequest<SessionResponse>("/api/protected");
  rows.push(
    comparison(
      "legacy diagnostic",
      "extension-fetch protected state",
      "authenticated",
      session.state,
      0,
    ),
  );
  await appendUnfilteredCookieDiagnostic(rows, "legacy extension-fetch");

  await apiRequest<{ clearedSessionCount: number }>("/api/reset", { method: "POST" });
  await removeAllProbeCookies();
}

async function appendUnfilteredCookieDiagnostic(
  rows: ResultRow[],
  context: string,
): Promise<void> {
  const cookies = await getCookiesByUrlOnly();
  rows.push(
    predicateResult(
      "diagnostic",
      `${context} unfiltered getAll({url}) cookie count`,
      ">=1",
      cookies.length,
      cookies.length >= 1,
      0,
    ),
  );
  cookies.forEach((cookie, index) => {
    rows.push(
      informational(
        "diagnostic",
        `${context} cookie[${index}] metadata`,
        JSON.stringify(describeCookie(cookie)),
      ),
    );
  });
  renderRows(rows);
}

async function removeAllProbeCookies(origin = TEST_ORIGIN): Promise<void> {
  const cookies = (await getCookiesByUrlOnly(origin)).filter(
    (cookie) => cookie.name === COOKIE_NAME,
  );
  for (const cookie of cookies) {
    await removeCookie(cookieDetails(cookie));
  }
}

function describeCookie(cookie: chrome.cookies.Cookie): Record<string, unknown> {
  return {
    name: cookie.name,
    value: "<redacted>",
    domain: cookie.domain,
    hostOnly: cookie.hostOnly,
    path: cookie.path,
    secure: cookie.secure,
    httpOnly: cookie.httpOnly,
    sameSite: cookie.sameSite,
    session: cookie.session,
    expirationDate: cookie.expirationDate ?? "<absent>",
    storeId: cookie.storeId,
    partitionKey: cookie.partitionKey ?? "<absent>",
  };
}

async function healthCheck(origin: string): Promise<void> {
  const response = await fetch(`${origin}/api/health`, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`${origin} health check returned HTTP ${response.status}`);
  }
}

function protectedRequest(testTabId: number): Promise<SessionResponse> {
  return firstPartyRequest<SessionResponse>(testTabId, "protected");
}

async function apiRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`${TEST_ORIGIN}${path}`, {
    ...init,
    cache: "no-store",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...init.headers,
    },
  });
  const body = (await response.json()) as T;
  if (!response.ok) {
    throw new Error(`${path} returned HTTP ${response.status}: ${JSON.stringify(body)}`);
  }
  return body;
}

async function findStoreIdForTab(tabId: number): Promise<string> {
  const stores = await getAllCookieStores();
  const store = stores.find((candidate) => candidate.tabIds.includes(tabId));
  if (store === undefined) {
    throw new Error(`Chrome returned no cookie store containing test tab ${tabId}`);
  }
  return store.id;
}

function getSessionCookies(storeId: string): Promise<chrome.cookies.Cookie[]> {
  return chromeCall((resolve) =>
    chrome.cookies.getAll({ url: `${TEST_ORIGIN}/`, name: COOKIE_NAME, storeId }, resolve),
  );
}

function getCookiesByUrlOnly(origin = TEST_ORIGIN): Promise<chrome.cookies.Cookie[]> {
  return chromeCall((resolve) =>
    chrome.cookies.getAll({ url: `${origin}/` }, resolve),
  );
}

function getAllCookiesWithoutFilters(): Promise<chrome.cookies.Cookie[]> {
  return chromeCall((resolve) => chrome.cookies.getAll({}, resolve));
}

function setCookie(details: chrome.cookies.CookieSetDetails): Promise<chrome.cookies.Cookie> {
  return chromeCall<chrome.cookies.Cookie | undefined>((resolve) =>
    chrome.cookies.set(details, resolve),
  ).then((cookie) => {
    if (cookie === undefined) {
      throw new Error("chrome.cookies.set returned no cookie");
    }
    return cookie;
  });
}

function removeCookie(
  details: chrome.cookies.CookieDetails,
): Promise<{ name: string; storeId: string; url: string } | null> {
  return chromeCall((resolve) => chrome.cookies.remove(details, resolve));
}

function getAllCookieStores(): Promise<chrome.cookies.CookieStore[]> {
  return chromeCall((resolve) => chrome.cookies.getAllCookieStores(resolve));
}

function getTab(tabId: number): Promise<chrome.tabs.Tab> {
  return chromeCall((resolve) => chrome.tabs.get(tabId, resolve));
}

async function createFirstPartyTestTab(origin: string): Promise<number> {
  const tab = await chromeCall<chrome.tabs.Tab>((resolve) =>
    chrome.tabs.create({ url: `${origin}/`, active: false }, resolve),
  );
  if (tab.id === undefined) {
    throw new Error("Chrome created the first-party test tab without a tab ID");
  }

  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const pong = await firstPartyRequest<{ ready: boolean; origin: string }>(tab.id, "ping");
      if (pong.ready && pong.origin === origin) {
        return tab.id;
      }
    } catch {
      // The content script is not ready until the loopback document reaches document_idle.
    }
    await delay(100);
  }
  await removeTabIgnoringErrors(tab.id);
  throw new Error(`${origin} first-party test tab content script did not become ready`);
}

async function firstPartyRequest<T>(tabId: number, command: FirstPartyCommand): Promise<T> {
  const response = await chromeCall<unknown>((resolve) =>
    chrome.tabs.sendMessage(
      tabId,
      { target: "fcp-session-probe", command },
      resolve,
    ),
  );
  if (typeof response !== "object" || response === null) {
    throw new Error(`first-party ${command} returned an invalid response`);
  }
  const envelope = response as FirstPartyResponse<T>;
  if (!envelope.ok || envelope.data === undefined) {
    throw new Error(`first-party ${command} failed: ${envelope.error ?? "unknown error"}`);
  }
  return envelope.data;
}

async function removeTabIgnoringErrors(tabId: number): Promise<void> {
  try {
    await chromeCall<void>((resolve) => chrome.tabs.remove(tabId, resolve));
  } catch {
    // The user may have closed the test tab; no further cleanup is required for the tab itself.
  }
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

function chromeCall<T>(register: (resolve: (value: T) => void) => void): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    register((value) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "unknown Chrome API error"));
        return;
      }
      resolve(value);
    });
  });
}

function renderRows(rows: readonly ResultRow[]): void {
  const fragment = document.createDocumentFragment();
  for (const row of rows) {
    const tableRow = document.createElement("tr");
    tableRow.className = row.passed ? "pass" : "fail";
    for (const value of [
      row.cycle,
      row.check,
      row.expected,
      row.actual,
      row.passed ? "PASS" : "FAIL",
      row.durationMs.toFixed(1),
    ]) {
      const cell = document.createElement("td");
      cell.textContent = value;
      tableRow.append(cell);
    }
    fragment.append(tableRow);
  }
  resultBody.replaceChildren(fragment);
}

function formatReport(
  startedAt: Date,
  requestedCycles: number,
  storeId: string,
  rows: readonly ResultRow[],
  outcomes: readonly CycleOutcome[],
  diagnostics: DiagnosticsResponse | undefined,
  extendedDiagnostic: ExtendedDiagnostic | undefined,
  loopbackIpDiagnostic: ExtendedDiagnostic | undefined,
): string {
  const successfulRestores = outcomes.filter((outcome) => outcome.restoreSucceeded).length;
  const wrongLogouts = outcomes.filter((outcome) => outcome.wrongLogout).length;
  const invalidatedSessions = outcomes.filter((outcome) => outcome.serverSessionInvalidated).length;
  const completedCycles = outcomes.length;
  const restoreRate = percentage(successfulRestores, completedCycles);
  const wrongLogoutRate = percentage(wrongLogouts, completedCycles);
  const passedChecks = rows.filter((row) => row.passed).length;

  const lines = [
    "FURSOY Disposable Profile Session Probe",
    `started_at=${startedAt.toISOString()}`,
    `test_origin=${TEST_ORIGIN}`,
    `comparison_origin=${LOOPBACK_IP_ORIGIN}`,
    `store_id=${storeId}`,
    `user_agent=${navigator.userAgent}`,
    `requested_cycles=${requestedCycles}`,
    `completed_cycles=${completedCycles}`,
    `restore_successes=${successfulRestores}`,
    `restore_success_rate=${restoreRate}`,
    `wrong_logouts=${wrongLogouts}`,
    `wrong_logout_rate=${wrongLogoutRate}`,
    `server_session_invalidations_during_cycles=${invalidatedSessions}`,
    `security_alarm_count=${diagnostics?.securityAlarmCount ?? "unknown"}`,
    "persistent_profile_corruption=manual_observation_required",
    `check_summary=${passedChecks}/${rows.length} passed`,
    "secret_redaction=session cookie values are always redacted; only the fixed synthetic probe name may appear in diagnostic metadata",
    "limitation=These controlled loopback results do not establish compatibility with real sites, rotation, device binding, or non-cookie storage.",
  ];

  lines.push("", "EXTENDED DIAGNOSTIC (RAW - NO AUTOMATIC INTERPRETATION)");
  if (extendedDiagnostic === undefined) {
    lines.push("extended_diagnostic=not_captured");
  } else {
    lines.push(`all_cookies_count=${extendedDiagnostic.cookies.length}`);
    extendedDiagnostic.cookies.forEach((cookie, index) => {
      lines.push(`all_cookie[${index}]=${JSON.stringify(cookie)}`);
    });
    lines.push(`all_cookies_after_250ms_count=${extendedDiagnostic.cookiesAfter250Ms.length}`);
    extendedDiagnostic.cookiesAfter250Ms.forEach((cookie, index) => {
      lines.push(`all_cookie_after_250ms[${index}]=${JSON.stringify(cookie)}`);
    });
    lines.push(`cookie_stores_count=${extendedDiagnostic.cookieStores.length}`);
    extendedDiagnostic.cookieStores.forEach((store, index) => {
      lines.push(`cookie_store[${index}]=${JSON.stringify(store)}`);
    });
    lines.push(`test_tab=${JSON.stringify(extendedDiagnostic.testTab)}`);
    lines.push(`content_page=${JSON.stringify(extendedDiagnostic.page)}`);
    lines.push(`docwrite_diagnostic=${JSON.stringify(extendedDiagnostic.docWrite)}`);
    lines.push(
      `docwrite_visible_to_cookies_api_immediate=${extendedDiagnostic.docWriteVisibleToCookiesApiImmediate}`,
    );
    lines.push(
      `docwrite_visible_to_cookies_api_after_250ms=${extendedDiagnostic.docWriteVisibleToCookiesApiAfter250Ms}`,
    );
    lines.push(
      `session_set_cookie_header_redacted=${extendedDiagnostic.sessionSetCookieHeaderRedacted}`,
    );
    lines.push(
      `server_request_cookie_diagnostics_count=${extendedDiagnostic.serverRequestCookieDiagnostics.length}`,
    );
    extendedDiagnostic.serverRequestCookieDiagnostics.forEach((diagnostic, index) => {
      lines.push(`server_request_cookie_diagnostic[${index}]=${JSON.stringify(diagnostic)}`);
    });
  }
  lines.push("END EXTENDED DIAGNOSTIC");

  lines.push("", "LOOPBACK IP DIAGNOSTIC (RAW - NO AUTOMATIC INTERPRETATION)");
  if (loopbackIpDiagnostic === undefined) {
    lines.push("loopback_ip_diagnostic=not_captured");
  } else {
    lines.push(`loopback_ip_origin=${LOOPBACK_IP_ORIGIN}`);
    lines.push(`loopback_ip_all_cookies_count=${loopbackIpDiagnostic.cookies.length}`);
    loopbackIpDiagnostic.cookies.forEach((cookie, index) => {
      lines.push(`loopback_ip_all_cookie[${index}]=${JSON.stringify(cookie)}`);
    });
    lines.push(
      `loopback_ip_all_cookies_after_250ms_count=${loopbackIpDiagnostic.cookiesAfter250Ms.length}`,
    );
    loopbackIpDiagnostic.cookiesAfter250Ms.forEach((cookie, index) => {
      lines.push(`loopback_ip_all_cookie_after_250ms[${index}]=${JSON.stringify(cookie)}`);
    });
    lines.push(`loopback_ip_test_tab=${JSON.stringify(loopbackIpDiagnostic.testTab)}`);
    lines.push(`loopback_ip_content_page=${JSON.stringify(loopbackIpDiagnostic.page)}`);
    lines.push(`loopback_ip_docwrite_diagnostic=${JSON.stringify(loopbackIpDiagnostic.docWrite)}`);
    lines.push(
      `loopback_ip_docwrite_visible_to_cookies_api_immediate=${loopbackIpDiagnostic.docWriteVisibleToCookiesApiImmediate}`,
    );
    lines.push(
      `loopback_ip_docwrite_visible_to_cookies_api_after_250ms=${loopbackIpDiagnostic.docWriteVisibleToCookiesApiAfter250Ms}`,
    );
    lines.push(
      `loopback_ip_session_set_cookie_header_redacted=${loopbackIpDiagnostic.sessionSetCookieHeaderRedacted}`,
    );
    lines.push(
      `loopback_ip_server_request_cookie_diagnostics_count=${loopbackIpDiagnostic.serverRequestCookieDiagnostics.length}`,
    );
    loopbackIpDiagnostic.serverRequestCookieDiagnostics.forEach((diagnostic, index) => {
      lines.push(
        `loopback_ip_server_request_cookie_diagnostic[${index}]=${JSON.stringify(diagnostic)}`,
      );
    });
  }
  lines.push(
    "END LOOPBACK IP DIAGNOSTIC",
    "",
    "cycle | check | expected | actual | result | duration_ms",
  );

  for (const row of rows) {
    lines.push(
      [
        row.cycle,
        row.check,
        row.expected,
        row.actual,
        row.passed ? "PASS" : "FAIL",
        row.durationMs.toFixed(1),
      ]
        .map(sanitizeReportValue)
        .join(" | "),
    );
  }
  return lines.join("\n");
}

async function copyReport(): Promise<void> {
  try {
    await navigator.clipboard.writeText(lastReport);
    statusElement.textContent = "Report copied as plain text.";
  } catch (error) {
    statusElement.textContent = `Copy failed: ${errorMessage(error)}`;
  }
}

function parseCycleCount(raw: string): number {
  const value = Number(raw);
  if (!Number.isInteger(value) || value < MIN_CYCLES || value > MAX_CYCLES) {
    throw new Error(`cycle count must be an integer from ${MIN_CYCLES} to ${MAX_CYCLES}`);
  }
  return value;
}

function comparison(
  cycle: string,
  check: string,
  expected: unknown,
  actual: unknown,
  durationMs: number,
): ResultRow {
  const expectedText = String(expected);
  const actualText = String(actual);
  return {
    cycle,
    check,
    expected: expectedText,
    actual: actualText,
    passed: expectedText === actualText,
    durationMs,
  };
}

function predicateResult(
  cycle: string,
  check: string,
  expected: unknown,
  actual: unknown,
  passed: boolean,
  durationMs: number,
): ResultRow {
  return {
    cycle,
    check,
    expected: String(expected),
    actual: String(actual),
    passed,
    durationMs,
  };
}

function informational(cycle: string, check: string, actual: string): ResultRow {
  return {
    cycle,
    check,
    expected: "metadata captured (value redacted)",
    actual,
    passed: true,
    durationMs: 0,
  };
}

function failure(
  cycle: string,
  check: string,
  expected: string,
  actual: string,
  durationMs: number,
): ResultRow {
  return { cycle, check, expected, actual, passed: false, durationMs };
}

function elapsed(startedAt: number): number {
  return performance.now() - startedAt;
}

function percentage(numerator: number, denominator: number): string {
  return denominator === 0 ? "n/a" : `${((numerator / denominator) * 100).toFixed(3)}%`;
}

function sanitizeReportValue(value: string): string {
  return value.replaceAll("|", "/").replaceAll("\r", " ").replaceAll("\n", " ");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function requireElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing required element #${id}`);
  }
  return element as T;
}

cycleInput.value = String(DEFAULT_CYCLES);
