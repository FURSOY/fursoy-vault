const TEST_HOST = "localhost";
const TEST_PORT = 43117;
const TEST_ORIGIN = `http://${TEST_HOST}:${TEST_PORT}`;
const PARTITION_TOP_LEVEL_SITE = TEST_ORIGIN;
const EXPIRATION_TOLERANCE_SECONDS = 2;

type SameSiteStatus = chrome.cookies.SameSiteStatus;

interface SourceCookie {
  domain: string;
  expirationDate?: number;
  hostOnly: boolean;
  httpOnly: boolean;
  path: string;
  sameSite: SameSiteStatus;
  secure: boolean;
  storeId: string;
  partitionKey?: chrome.cookies.CookiePartitionKey;
}

interface ProbeCase {
  label: string;
  name: string;
  source: SourceCookie;
  attributes: readonly AttributeName[];
}

type AttributeName =
  | "hostOnly"
  | "domain"
  | "path"
  | "secure"
  | "httpOnly"
  | "sameSite"
  | "session"
  | "expirationDate"
  | "partitionKey"
  | "storeId"
  | "url"
  | "prefix";

interface ResultRow {
  testCase: string;
  attribute: string;
  expected: string;
  actual: string;
  passed: boolean;
}

interface RunContext {
  runId: string;
  storeId: string;
}

const runButton = requireElement<HTMLButtonElement>("run-probe");
const copyButton = requireElement<HTMLButtonElement>("copy-report");
const statusElement = requireElement<HTMLElement>("status");
const resultBody = requireElement<HTMLTableSectionElement>("result-body");

let lastReport = "";

runButton.addEventListener("click", () => void runSuite());
copyButton.addEventListener("click", () => void copyReport());

async function runSuite(): Promise<void> {
  runButton.disabled = true;
  copyButton.disabled = true;
  statusElement.textContent = "Running probe suite…";
  resultBody.replaceChildren();

  const startedAt = new Date();
  const rows: ResultRow[] = [];

  try {
    await assertTestSiteAvailable();
    const storeId = await findCurrentStoreId();
    const context: RunContext = {
      runId: startedAt.getTime().toString(36),
      storeId,
    };

    for (const probeCase of createCases(context)) {
      rows.push(...(await runCase(probeCase)));
    }

    renderRows(rows);
    lastReport = formatReport(startedAt, storeId, rows);
    copyButton.disabled = false;

    const passed = rows.filter((row) => row.passed).length;
    statusElement.textContent = `Completed: ${passed}/${rows.length} checks passed.`;
  } catch (error) {
    const message = errorMessage(error);
    rows.push({
      testCase: "suite",
      attribute: "fatal error",
      expected: "probe suite completes",
      actual: message,
      passed: false,
    });
    renderRows(rows);
    lastReport = formatReport(startedAt, "unknown", rows);
    copyButton.disabled = false;
    statusElement.textContent = `Probe failed: ${message}`;
  } finally {
    runButton.disabled = false;
  }
}

function createCases(context: RunContext): ProbeCase[] {
  const base: Omit<SourceCookie, "sameSite"> = {
    domain: TEST_HOST,
    hostOnly: true,
    httpOnly: false,
    path: "/",
    secure: false,
    storeId: context.storeId,
  };
  const name = (suffix: string): string => `FCP-probe-${suffix}-${context.runId}`;

  return [
    {
      label: "host-only session cookie",
      name: name("host-only"),
      source: { ...base, sameSite: "unspecified" },
      attributes: ["hostOnly", "domain", "session", "storeId", "url"],
    },
    {
      label: "domain persistent cookie",
      name: name("domain"),
      source: {
        ...base,
        hostOnly: false,
        sameSite: "lax",
        expirationDate: Math.floor(Date.now() / 1000) + 3600,
      },
      attributes: ["hostOnly", "domain", "session", "expirationDate", "url"],
    },
    {
      label: "path and HttpOnly cookie",
      name: name("path-http-only"),
      source: { ...base, httpOnly: true, path: "/probe/deep", sameSite: "strict" },
      attributes: ["path", "httpOnly", "sameSite", "url"],
    },
    {
      label: "Secure cookie",
      name: name("secure"),
      source: { ...base, secure: true, sameSite: "lax" },
      attributes: ["secure", "url"],
    },
    ...(["unspecified", "lax", "strict"] as const).map((sameSite) => ({
      label: `SameSite=${sameSite}`,
      name: name(`same-site-${sameSite}`),
      source: { ...base, sameSite },
      attributes: ["sameSite"] as const,
    })),
    {
      label: "SameSite=no_restriction with Secure",
      name: name("same-site-none"),
      source: { ...base, sameSite: "no_restriction", secure: true },
      attributes: ["sameSite", "secure", "url"],
    },
    {
      label: "CHIPS partition key",
      name: name("partitioned"),
      source: {
        ...base,
        sameSite: "no_restriction",
        secure: true,
        partitionKey: { topLevelSite: PARTITION_TOP_LEVEL_SITE },
      },
      attributes: ["partitionKey", "secure", "sameSite", "storeId", "url"],
    },
    {
      label: "__Host- prefix",
      name: "__Host-FCP-probe",
      source: { ...base, hostOnly: true, path: "/", secure: true, sameSite: "lax" },
      attributes: ["prefix", "hostOnly", "domain", "path", "secure", "url"],
    },
    {
      label: "__Secure- prefix",
      name: "__Secure-FCP-probe",
      source: { ...base, secure: true, sameSite: "lax" },
      attributes: ["prefix", "secure", "url"],
    },
  ];
}

async function runCase(probeCase: ProbeCase): Promise<ResultRow[]> {
  const details = buildShadowCookie(probeCase.source, probeCase.name);
  const cleanupDetails = cookieDetails(details);
  const rows: ResultRow[] = [];

  // This is only an API round-trip probe. Matching attributes do not prove that a real
  // server-side session survives eviction and restoration.
  await removeIgnoringMissing(cleanupDetails);

  try {
    await setCookie(details);
    const actual = await getCookie(cleanupDetails);
    if (actual === null) {
      rows.push(failure(probeCase.label, "read-back", "cookie exists", "cookie not found"));
    } else {
      rows.push(
        ...probeCase.attributes.map((attribute) =>
          compareAttribute(probeCase, attribute, details, actual),
        ),
      );
    }
  } catch (error) {
    rows.push(
      failure(probeCase.label, "write/read", "round-trip succeeds", errorMessage(error)),
    );
  } finally {
    try {
      await removeCookie(cleanupDetails);
      const remaining = await getCookie(cleanupDetails);
      rows.push(
        comparison(
          probeCase.label,
          "cleanup",
          "absent",
          remaining === null ? "absent" : "cookie still present",
        ),
      );
    } catch (error) {
      rows.push(failure(probeCase.label, "cleanup", "cookie removed", errorMessage(error)));
    }
  }

  return rows;
}

function buildShadowCookie(
  source: SourceCookie,
  probeName: string,
): chrome.cookies.CookieSetDetails {
  if (source.domain.length === 0) {
    throw new Error("source cookie domain is empty");
  }

  const details: chrome.cookies.CookieSetDetails = {
    url: deriveCookieUrl(source),
    name: probeName,
    value: "synthetic-probe-value",
    path: source.path,
    secure: source.secure,
    httpOnly: source.httpOnly,
    sameSite: source.sameSite,
    storeId: source.storeId,
  };

  if (!source.hostOnly) {
    details.domain = source.domain;
  }
  if (source.expirationDate !== undefined) {
    details.expirationDate = source.expirationDate;
  }
  if (source.partitionKey !== undefined) {
    details.partitionKey = source.partitionKey;
  }

  return details;
}

function deriveCookieUrl(source: SourceCookie): string {
  const scheme = source.secure ? "https" : "http";
  const host = source.domain.replace(/^\./, "");
  const path = source.path.startsWith("/") ? source.path : `/${source.path}`;
  return `${scheme}://${host}:${TEST_PORT}${path}`;
}

function compareAttribute(
  probeCase: ProbeCase,
  attribute: AttributeName,
  details: chrome.cookies.CookieSetDetails,
  actual: chrome.cookies.Cookie,
): ResultRow {
  const source = probeCase.source;

  switch (attribute) {
    case "hostOnly":
      return comparison(probeCase.label, attribute, source.hostOnly, actual.hostOnly);
    case "domain": {
      const expected = source.hostOnly ? source.domain : `.${source.domain.replace(/^\./, "")}`;
      return comparison(probeCase.label, attribute, expected, actual.domain);
    }
    case "path":
      return comparison(probeCase.label, attribute, source.path, actual.path);
    case "secure":
      return comparison(probeCase.label, attribute, source.secure, actual.secure);
    case "httpOnly":
      return comparison(probeCase.label, attribute, source.httpOnly, actual.httpOnly);
    case "sameSite":
      return comparison(probeCase.label, attribute, source.sameSite, actual.sameSite);
    case "session":
      return comparison(
        probeCase.label,
        attribute,
        source.expirationDate === undefined,
        actual.session,
      );
    case "expirationDate": {
      const expected = source.expirationDate;
      const observed = actual.expirationDate;
      const passed =
        expected !== undefined &&
        observed !== undefined &&
        Math.abs(expected - observed) <= EXPIRATION_TOLERANCE_SECONDS;
      return result(
        probeCase.label,
        attribute,
        `${formatValue(expected)} (±${EXPIRATION_TOLERANCE_SECONDS}s)`,
        formatValue(observed),
        passed,
      );
    }
    case "partitionKey":
      return comparison(
        probeCase.label,
        attribute,
        normalizePartitionKey(source.partitionKey),
        normalizePartitionKey(actual.partitionKey),
      );
    case "storeId":
      return comparison(probeCase.label, attribute, source.storeId, actual.storeId);
    case "url":
      return comparison(probeCase.label, attribute, deriveCookieUrl(source), details.url);
    case "prefix":
      return comparison(probeCase.label, attribute, probeCase.name, actual.name);
  }
}

function normalizePartitionKey(
  partitionKey: chrome.cookies.CookiePartitionKey | undefined,
): string {
  if (partitionKey?.topLevelSite === undefined) {
    return "absent";
  }
  const topLevelSite = new URL(partitionKey.topLevelSite).origin;
  const ancestor = partitionKey.hasCrossSiteAncestor;
  return ancestor === undefined
    ? `topLevelSite=${topLevelSite}`
    : `topLevelSite=${topLevelSite}, hasCrossSiteAncestor=${ancestor}`;
}

async function assertTestSiteAvailable(): Promise<void> {
  let response: Response;
  try {
    response = await fetch(`${TEST_ORIGIN}/health.json`, { cache: "no-store" });
  } catch (error) {
    throw new Error(`test site is unavailable at ${TEST_ORIGIN}: ${errorMessage(error)}`);
  }
  if (!response.ok) {
    throw new Error(`test site health check returned HTTP ${response.status}`);
  }
}

async function findCurrentStoreId(): Promise<string> {
  const [stores, tab] = await Promise.all([getAllCookieStores(), getCurrentTab()]);
  const tabId = tab.id;
  const store =
    tabId === undefined
      ? stores[0]
      : stores.find((candidate) => candidate.tabIds.includes(tabId)) ?? stores[0];
  if (store === undefined) {
    throw new Error("Chrome returned no cookie stores");
  }
  return store.id;
}

function cookieDetails(details: chrome.cookies.CookieSetDetails): chrome.cookies.CookieDetails {
  if (details.name === undefined) {
    throw new Error("probe cookie name is missing");
  }
  return {
    url: details.url,
    name: details.name,
    storeId: details.storeId,
    partitionKey: details.partitionKey,
  };
}

function getCookie(details: chrome.cookies.CookieDetails): Promise<chrome.cookies.Cookie | null> {
  return chromeCall((resolve) => chrome.cookies.get(details, resolve));
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

function getAllCookieStores(): Promise<chrome.cookies.CookieStore[]> {
  return chromeCall((resolve) => chrome.cookies.getAllCookieStores(resolve));
}

function getCurrentTab(): Promise<chrome.tabs.Tab> {
  return chromeCall<chrome.tabs.Tab | undefined>((resolve) => chrome.tabs.getCurrent(resolve)).then(
    (tab) => tab ?? {},
  );
}

async function removeIgnoringMissing(details: chrome.cookies.CookieDetails): Promise<void> {
  try {
    await chromeCall((resolve) => chrome.cookies.remove(details, resolve));
  } catch {
    // Cleanup must not hide the primary probe result. A later write uses the same exact name.
  }
}

function removeCookie(
  details: chrome.cookies.CookieDetails,
): Promise<{ name: string; storeId: string; url: string } | null> {
  return chromeCall((resolve) => chrome.cookies.remove(details, resolve));
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
      row.testCase,
      row.attribute,
      row.expected,
      row.actual,
      row.passed ? "PASS" : "FAIL",
    ]) {
      const cell = document.createElement("td");
      cell.textContent = value;
      tableRow.append(cell);
    }
    fragment.append(tableRow);
  }
  resultBody.replaceChildren(fragment);
}

function formatReport(startedAt: Date, storeId: string, rows: readonly ResultRow[]): string {
  const passed = rows.filter((row) => row.passed).length;
  const lines = [
    "FURSOY Cookie Attribute Probe",
    `started_at=${startedAt.toISOString()}`,
    `test_origin=${TEST_ORIGIN}`,
    `store_id=${storeId}`,
    `user_agent=${navigator.userAgent}`,
    `summary=${passed}/${rows.length} passed`,
    "limitation=This proves only chrome.cookies API attribute round-trip compatibility; it does not prove that a real session survives eviction and restore.",
    "",
    "case | attribute | expected | actual | result",
  ];
  for (const row of rows) {
    lines.push(
      [row.testCase, row.attribute, row.expected, row.actual, row.passed ? "PASS" : "FAIL"]
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

function comparison(
  testCase: string,
  attribute: string,
  expected: unknown,
  actual: unknown,
): ResultRow {
  const expectedText = formatValue(expected);
  const actualText = formatValue(actual);
  return result(testCase, attribute, expectedText, actualText, expectedText === actualText);
}

function failure(
  testCase: string,
  attribute: string,
  expected: string,
  actual: string,
): ResultRow {
  return result(testCase, attribute, expected, actual, false);
}

function result(
  testCase: string,
  attribute: string,
  expected: string,
  actual: string,
  passed: boolean,
): ResultRow {
  return { testCase, attribute, expected, actual, passed };
}

function formatValue(value: unknown): string {
  return value === undefined ? "absent" : String(value);
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
