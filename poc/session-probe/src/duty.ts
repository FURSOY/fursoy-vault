export {};

const TEST_ORIGIN = "http://localhost:43118";
const TEST_URL = `${TEST_ORIGIN}/`;

interface DutyEvent {
  sequence: number;
  at: number;
  event: string;
  detail: string;
  durationMs?: number;
}

interface DutyPublicState {
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
  snapshotAvailable: boolean;
  browserOpenTimeMs: number;
  cookiePresentMs: number;
  activeExposureMs: number;
  unnecessaryExposureMs: number;
  exposureDutyCycle: number;
  unnecessaryExposureRatio: number;
  events: DutyEvent[];
}

interface RuntimeResponse<T> {
  ok: boolean;
  data?: T;
  error?: string;
}

const durationInput = requiredElement<HTMLInputElement>("duration-minutes");
const activeInput = requiredElement<HTMLInputElement>("active-seconds");
const closedInput = requiredElement<HTMLInputElement>("closed-seconds");
const idleInput = requiredElement<HTMLInputElement>("idle-seconds");
const runButton = requiredElement<HTMLButtonElement>("run-duty");
const stopButton = requiredElement<HTMLButtonElement>("stop-duty");
const copyButton = requiredElement<HTMLButtonElement>("copy-duty");
const statusElement = requiredElement<HTMLElement>("status");
const eventBody = requiredElement<HTMLTableSectionElement>("event-body");
const browserOpenElement = requiredElement<HTMLElement>("browser-open");
const cookiePresentElement = requiredElement<HTMLElement>("cookie-present");
const dutyCycleElement = requiredElement<HTMLElement>("duty-cycle");
const activeExposureElement = requiredElement<HTMLElement>("active-exposure");
const unnecessaryExposureElement = requiredElement<HTMLElement>("unnecessary-exposure");
const unnecessaryRatioElement = requiredElement<HTMLElement>("unnecessary-ratio");

let running = false;
let stopRequested = false;
let testTabId: number | undefined;
let latestState: DutyPublicState | null = null;
let latestReport = "";

runButton.addEventListener("click", () => {
  void runSimulation();
});

stopButton.addEventListener("click", () => {
  stopRequested = true;
  statusElement.textContent = "Stopping measurement…";
  void stopAndRender("user requested stop");
});

copyButton.addEventListener("click", () => {
  void navigator.clipboard.writeText(latestReport).then(() => {
    statusElement.textContent = "Duty-cycle report copied as plain text.";
  });
});

void restoreExistingView();

async function runSimulation(): Promise<void> {
  if (running) {
    return;
  }
  let durationMs: number;
  let activeSeconds: number;
  let closedSeconds: number;
  let idleThresholdSeconds: number;
  try {
    durationMs = readNumber(durationInput, 1, 30, "duration") * 60_000;
    activeSeconds = readNumber(activeInput, 10, 300, "active phase");
    closedSeconds = readNumber(closedInput, 5, 300, "closed-tab phase");
    idleThresholdSeconds = readNumber(idleInput, 15, 300, "idle threshold");
  } catch (error: unknown) {
    statusElement.textContent = errorMessage(error);
    return;
  }
  if ((activeSeconds + closedSeconds + idleThresholdSeconds + 10) * 1_000 >= durationMs) {
    statusElement.textContent =
      "Duration is too short: leave at least 10 seconds after active + closed + idle phases.";
    return;
  }

  setControlsRunning(true);
  stopRequested = false;
  latestReport = "";
  copyButton.disabled = true;
  clearView();

  try {
    statusElement.textContent = "Opening the controlled first-party test tab…";
    testTabId = await createTab(TEST_URL, true);
    await waitForContent(testTabId);
    await firstPartyCommand(testTabId, "reset");

    latestState = await dutyCommand<DutyPublicState>({
      target: "fcp-duty-cycle",
      command: "start",
      durationMs,
      idleThresholdSeconds,
      origin: TEST_ORIGIN,
    });
    renderState(latestState);

    statusElement.textContent = "Logging in once from the first-party test tab…";
    await firstPartyCommand(testTabId, "login");
    const loginState = await firstPartyCommand<{ state?: string }>(testTabId, "protected");
    if (loginState.state !== "authenticated") {
      throw new Error(`login health check returned ${String(loginState.state)}`);
    }
    latestState = await dutyCommand<DutyPublicState>({
      target: "fcp-duty-cycle",
      command: "observe-login",
      tabId: testTabId,
    });
    renderState(latestState);

    statusElement.textContent = `Active-use phase: keep the test tab focused for ${activeSeconds} seconds.`;
    await waitPhase(activeSeconds * 1_000);
    assertNotStopped();

    statusElement.textContent = "Closing the last relevant tab; onRemoved must evict automatically…";
    await removeTab(testTabId);
    testTabId = undefined;
    await waitForState(
      (state) => !state.cookiePresent && eventCount(state, "last_tab_closed") >= 1,
      10_000,
      "last-tab eviction",
    );

    statusElement.textContent = `Closed-tab phase: cookie should remain absent for ${closedSeconds} seconds.`;
    await waitPhase(closedSeconds * 1_000);
    assertNotStopped();

    statusElement.textContent = "Reopening the relevant tab; background listener must inject automatically…";
    testTabId = await createTab(TEST_URL, true);
    await waitForContent(testTabId);
    await waitForAuthenticated(testTabId, 10_000);
    await waitForState(
      (state) => state.cookiePresent && eventCount(state, "inject") >= 1,
      10_000,
      "automatic inject",
    );

    statusElement.textContent =
      `Idle phase armed. Do not use mouse or keyboard; chrome.idle should fire after ${idleThresholdSeconds} seconds.`;
    await waitUntilMeasurementStops();
    await finishAndCleanup();
    statusElement.textContent = "Measurement complete. Review the metrics and copy the text report.";
  } catch (error: unknown) {
    if (!stopRequested) {
      statusElement.textContent = `Measurement failed: ${errorMessage(error)}`;
      await stopAndRender(`fatal harness error: ${errorMessage(error)}`);
    }
    await cleanupTestTab();
  } finally {
    running = false;
    setControlsRunning(false);
  }
}

async function waitUntilMeasurementStops(): Promise<void> {
  while (!stopRequested) {
    const state = await getDutyState();
    if (state === null) {
      throw new Error("background measurement state disappeared");
    }
    latestState = state;
    renderState(state);
    if (!state.active) {
      return;
    }
    await delay(1_000);
  }
  throw new Error("measurement stopped by user");
}

async function waitPhase(milliseconds: number): Promise<void> {
  const end = Date.now() + milliseconds;
  while (Date.now() < end) {
    assertNotStopped();
    const state = await getDutyState();
    if (state !== null) {
      latestState = state;
      renderState(state);
      if (!state.active) {
        throw new Error("measurement duration elapsed before the simulation phase completed");
      }
    }
    await delay(Math.min(1_000, Math.max(0, end - Date.now())));
  }
}

async function waitForState(
  predicate: (state: DutyPublicState) => boolean,
  timeoutMs: number,
  label: string,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const state = await getDutyState();
    if (state !== null) {
      latestState = state;
      renderState(state);
      if (predicate(state)) {
        return;
      }
    }
    await delay(200);
  }
  throw new Error(`${label} did not complete within ${timeoutMs} ms`);
}

async function finishAndCleanup(): Promise<void> {
  const state = await getDutyState();
  if (state !== null) {
    latestState = state;
    renderState(state);
    latestReport = buildReport(state);
    copyButton.disabled = false;
  }
  if (testTabId !== undefined) {
    try {
      await firstPartyCommand(testTabId, "reset");
    } catch {
      // Cleanup is best-effort after metrics have been finalized.
    }
  }
  await cleanupTestTab();
}

async function stopAndRender(_reason: string): Promise<void> {
  try {
    const state = await dutyCommand<DutyPublicState>({ target: "fcp-duty-cycle", command: "stop" });
    latestState = state;
    renderState(state);
    latestReport = buildReport(state);
    copyButton.disabled = false;
  } catch (error: unknown) {
    statusElement.textContent = `Stop failed: ${errorMessage(error)}`;
  }
  await cleanupTestTab();
}

async function cleanupTestTab(): Promise<void> {
  const tabId = testTabId;
  testTabId = undefined;
  if (tabId === undefined) {
    return;
  }
  try {
    await removeTab(tabId);
  } catch {
    // The user may already have closed the tab.
  }
}

async function restoreExistingView(): Promise<void> {
  try {
    const state = await getDutyState();
    if (state === null) {
      return;
    }
    latestState = state;
    renderState(state);
    if (!state.active) {
      latestReport = buildReport(state);
      copyButton.disabled = false;
    }
  } catch {
    // A freshly loaded extension legitimately has no session state yet.
  }
}

function renderState(state: DutyPublicState): void {
  browserOpenElement.textContent = formatDuration(state.browserOpenTimeMs);
  cookiePresentElement.textContent = formatDuration(state.cookiePresentMs);
  dutyCycleElement.textContent = formatPercent(state.exposureDutyCycle);
  activeExposureElement.textContent = formatDuration(state.activeExposureMs);
  unnecessaryExposureElement.textContent = formatDuration(state.unnecessaryExposureMs);
  unnecessaryRatioElement.textContent = formatPercent(state.unnecessaryExposureRatio);

  eventBody.replaceChildren();
  for (const event of state.events) {
    const row = document.createElement("tr");
    appendCell(row, String(event.sequence));
    appendCell(row, new Date(event.at).toISOString());
    appendCell(row, event.event);
    appendCell(row, event.detail);
    appendCell(row, event.durationMs === undefined ? "—" : event.durationMs.toFixed(1));
    eventBody.append(row);
  }
  if (state.events.length === 0) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = 5;
    cell.textContent = "No events yet.";
    row.append(cell);
    eventBody.append(row);
  }
}

function buildReport(state: DutyPublicState): string {
  const counts = new Map<string, number>();
  for (const event of state.events) {
    counts.set(event.event, (counts.get(event.event) ?? 0) + 1);
  }
  const lines = [
    "FURSOY Duty-cycle Probe",
    `started_at=${new Date(state.startedAt).toISOString()}`,
    `stopped_at=${state.stoppedAt === undefined ? "not_stopped" : new Date(state.stoppedAt).toISOString()}`,
    `test_origin=${state.origin}`,
    `idle_threshold_seconds=${state.idleThresholdSeconds}`,
    "active_definition=focused Chrome window + active relevant tab + chrome.idle state active",
    `browser_open_time_ms=${state.browserOpenTimeMs}`,
    `cookie_present_ms=${state.cookiePresentMs}`,
    `active_exposure_ms=${state.activeExposureMs}`,
    `unnecessary_exposure_ms=${state.unnecessaryExposureMs}`,
    `exposure_duty_cycle=${formatPercent(state.exposureDutyCycle)}`,
    `unnecessary_exposure_over_browser_open_time=${formatPercent(state.unnecessaryExposureRatio)}`,
    `inject_count=${counts.get("inject") ?? 0}`,
    `last_tab_closed_count=${counts.get("last_tab_closed") ?? 0}`,
    `idle_start_count=${counts.get("idle_start") ?? 0}`,
    `eviction_count=${counts.get("eviction") ?? 0}`,
    `failed_eviction_count=${counts.get("failed_eviction") ?? 0}`,
    `reconciliation_count=${counts.get("reconciliation") ?? 0}`,
    `site_cookie_recreated_count=${counts.get("site_cookie_recreated") ?? 0}`,
    "scope=synthetic localhost session only; this does not prove production-site compatibility",
    "",
    "sequence | timestamp | event | detail | duration_ms",
  ];
  for (const event of state.events) {
    lines.push(
      `${event.sequence} | ${new Date(event.at).toISOString()} | ${event.event} | ${event.detail} | ${event.durationMs?.toFixed(1) ?? "-"}`,
    );
  }
  return lines.join("\n");
}

function clearView(): void {
  browserOpenElement.textContent = "—";
  cookiePresentElement.textContent = "—";
  dutyCycleElement.textContent = "—";
  activeExposureElement.textContent = "—";
  unnecessaryExposureElement.textContent = "—";
  unnecessaryRatioElement.textContent = "—";
  eventBody.replaceChildren();
}

function setControlsRunning(value: boolean): void {
  running = value;
  runButton.disabled = value;
  stopButton.disabled = !value;
  durationInput.disabled = value;
  activeInput.disabled = value;
  closedInput.disabled = value;
  idleInput.disabled = value;
}

function assertNotStopped(): void {
  if (stopRequested) {
    throw new Error("measurement stopped by user");
  }
}

function eventCount(state: DutyPublicState, name: string): number {
  return state.events.filter((event) => event.event === name).length;
}

async function waitForAuthenticated(tabId: number, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastState = "unknown";
  while (Date.now() < deadline) {
    try {
      const result = await firstPartyCommand<{ state?: string }>(tabId, "protected");
      lastState = String(result.state);
      if (result.state === "authenticated") {
        return;
      }
    } catch {
      lastState = "content_not_ready";
    }
    await delay(200);
  }
  throw new Error(`restored session did not authenticate; last_state=${lastState}`);
}

async function waitForContent(tabId: number): Promise<void> {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      await firstPartyCommand(tabId, "ping");
      return;
    } catch {
      await delay(100);
    }
  }
  throw new Error("test tab content script did not become ready");
}

function getDutyState(): Promise<DutyPublicState | null> {
  return dutyCommand<DutyPublicState | null>({ target: "fcp-duty-cycle", command: "get-state" });
}

function dutyCommand<T>(message: unknown): Promise<T> {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendMessage(message, (rawResponse: unknown) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "background message failed"));
        return;
      }
      const response = rawResponse as RuntimeResponse<T> | undefined;
      if (response === undefined || typeof response.ok !== "boolean") {
        reject(new Error("background returned an invalid response"));
        return;
      }
      if (!response.ok) {
        reject(new Error(response.error ?? "background command failed"));
        return;
      }
      resolve(response.data as T);
    });
  });
}

function firstPartyCommand<T = unknown>(tabId: number, command: string): Promise<T> {
  return new Promise((resolve, reject) => {
    chrome.tabs.sendMessage(
      tabId,
      { target: "fcp-session-probe", command },
      (rawResponse: unknown) => {
        const lastError = chrome.runtime.lastError;
        if (lastError !== undefined) {
          reject(new Error(lastError.message ?? `content command ${command} failed`));
          return;
        }
        const response = rawResponse as RuntimeResponse<T> | undefined;
        if (response === undefined || typeof response.ok !== "boolean") {
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

function createTab(url: string, active: boolean): Promise<number> {
  return new Promise((resolve, reject) => {
    chrome.tabs.create({ url, active }, (tab) => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined || tab.id === undefined) {
        reject(new Error(lastError?.message ?? "tabs.create returned no tab id"));
        return;
      }
      resolve(tab.id);
    });
  });
}

function removeTab(tabId: number): Promise<void> {
  return new Promise((resolve, reject) => {
    chrome.tabs.remove(tabId, () => {
      const lastError = chrome.runtime.lastError;
      if (lastError !== undefined) {
        reject(new Error(lastError.message ?? "tabs.remove failed"));
        return;
      }
      resolve();
    });
  });
}

function appendCell(row: HTMLTableRowElement, value: string): void {
  const cell = document.createElement("td");
  cell.textContent = value;
  row.append(cell);
}

function formatDuration(milliseconds: number): string {
  return `${(milliseconds / 1_000).toFixed(1)} s`;
}

function formatPercent(ratio: number): string {
  return `${(ratio * 100).toFixed(3)}%`;
}

function readNumber(input: HTMLInputElement, minimum: number, maximum: number, label: string): number {
  const value = Number(input.value);
  if (!Number.isFinite(value) || value < minimum || value > maximum) {
    throw new Error(`${label} must be between ${minimum} and ${maximum}`);
  }
  return value;
}

function requiredElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing required element #${id}`);
  }
  return element as T;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
