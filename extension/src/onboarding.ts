import { enhanceSelects } from "./custom-select.js";
import { translate, type Locale } from "./i18n.js";
import { currentLocale } from "./locale.js";
import { isValidProtectionScope, normalizeProtectionScopeInput } from "./protocol.js";

type PolicyLevel = "critical" | "balanced" | "convenient" | "monitor";
type OnboardingStepName = "welcome" | "install" | "addsite" | "done";
interface RecoveryCandidate {
  profileId: string;
  displayName: string;
  browser: string;
  lastUsedUnixMs: number;
  siteCount: number;
}

const ONBOARDING_STEP_KEY = "fursoy.onboarding.step";

// GitHub always redirects this exact URL to the latest release's same-named asset — see the
// matching comment in native-host/install/package-release.ps1. The friendly Setup filename stays
// fixed while Velopack's versioned package/feed assets power automatic companion updates.
const INSTALLER_DOWNLOAD_URL = "https://github.com/FURSOY/fursoy-vault/releases/latest/download/FURSOY-Vault-Setup.exe";

function required<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) throw new Error(`missing required element ${id}`);
  return element as T;
}

const stepWelcome = required<HTMLElement>("step-welcome");
const stepInstall = required<HTMLElement>("step-install");
const stepAddsite = required<HTMLElement>("step-addsite");
const stepDone = required<HTMLElement>("step-done");

const welcomeNext = required<HTMLButtonElement>("welcome-next");
const installDownload = required<HTMLAnchorElement>("install-download");
const installCheck = required<HTMLButtonElement>("install-check");
const installStatus = required<HTMLElement>("install-status");
const installSkip = required<HTMLButtonElement>("install-skip");
const recoveryPanel = required<HTMLElement>("recovery-panel");
const recoveryList = required<HTMLElement>("recovery-list");
const recoveryStatus = required<HTMLElement>("recovery-status");
const addsiteForm = required<HTMLFormElement>("addsite-form");
const addsiteScope = required<HTMLInputElement>("addsite-scope");
const addsiteDomainInput = required<HTMLElement>("addsite-domain-input");
const addsitePolicy = required<HTMLSelectElement>("addsite-policy");
const addsiteError = required<HTMLElement>("addsite-error");
const addsiteSkip = required<HTMLButtonElement>("addsite-skip");
const doneFinish = required<HTMLButtonElement>("done-finish");

function showStep(step: HTMLElement): void {
  const steps = [stepWelcome, stepInstall, stepAddsite, stepDone];
  const names: OnboardingStepName[] = ["welcome", "install", "addsite", "done"];
  const activeIndex = steps.indexOf(step);
  const activeName = names[activeIndex];
  if (activeIndex < 0 || activeName === undefined) throw new Error("unknown onboarding step");
  for (const section of steps) section.hidden = section !== step;
  document.body.dataset.step = activeName;
  sessionStorage.setItem(ONBOARDING_STEP_KEY, activeName);
  document.querySelectorAll<HTMLElement>(".step-dot").forEach((dot, index) => {
    dot.classList.toggle("active", index === activeIndex);
    dot.classList.toggle("past", index < activeIndex);
  });
  document.querySelectorAll<HTMLElement>(".step-line").forEach((line, index) => {
    line.classList.toggle("past", index < activeIndex);
  });
}

function restoreStep(): void {
  const savedStep = sessionStorage.getItem(ONBOARDING_STEP_KEY) as OnboardingStepName | null;
  const steps: Record<OnboardingStepName, HTMLElement> = {
    welcome: stepWelcome,
    install: stepInstall,
    addsite: stepAddsite,
    done: stepDone,
  };
  showStep(savedStep === null ? stepWelcome : steps[savedStep] ?? stepWelcome);
}

function t(locale: Locale, key: string): string {
  return translate(locale, key, undefined);
}

function applyTranslations(locale: Locale): void {
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((element) => {
    const key = element.dataset.i18n;
    if (key !== undefined) element.textContent = t(locale, key);
  });
  required<HTMLElement>("welcome-title").textContent = t(locale, "onboarding.welcome.title");
  required<HTMLElement>("welcome-body").textContent = t(locale, "onboarding.welcome.body");
  welcomeNext.textContent = t(locale, "onboarding.welcome.next");

  required<HTMLElement>("install-title").textContent = t(locale, "onboarding.install.title");
  required<HTMLElement>("install-body").textContent = t(locale, "onboarding.install.body");
  installDownload.textContent = t(locale, "onboarding.install.downloadButton");
  installDownload.href = INSTALLER_DOWNLOAD_URL;
  required<HTMLElement>("install-hello-notice").textContent = t(locale, "onboarding.install.helloNotice");
  required<HTMLElement>("install-signing-notice").textContent = t(locale, "onboarding.install.signingNotice");
  installCheck.textContent = t(locale, "onboarding.install.checkButton");
  installSkip.textContent = t(locale, "onboarding.install.skip");
  required<HTMLElement>("recovery-title").textContent = t(locale, "onboarding.recovery.title");
  required<HTMLElement>("recovery-body").textContent = t(locale, "onboarding.recovery.body");

  required<HTMLElement>("addsite-title").textContent = t(locale, "onboarding.addsite.title");
  required<HTMLElement>("addsite-body").textContent = t(locale, "onboarding.addsite.body");
  required<HTMLElement>("addsite-scope-label").textContent = t(locale, "onboarding.addsite.scopeLabel");
  addsiteScope.placeholder = t(locale, "onboarding.addsite.scopePlaceholder");
  required<HTMLElement>("addsite-policy-label").textContent = t(locale, "onboarding.addsite.policyLabel");
  required<HTMLButtonElement>("addsite-submit").textContent = t(locale, "onboarding.addsite.submit");
  addsiteSkip.textContent = t(locale, "onboarding.addsite.skip");
  addsitePolicy.querySelectorAll<HTMLOptionElement>("option[data-key]").forEach((option) => {
    const key = option.dataset.key;
    if (key !== undefined) option.textContent = t(locale, key);
  });

  required<HTMLElement>("done-title").textContent = t(locale, "onboarding.done.title");
  required<HTMLElement>("done-body").textContent = t(locale, "onboarding.done.body");
  doneFinish.textContent = t(locale, "onboarding.done.finish");
}

function send(message: Record<string, unknown>): Promise<Record<string, unknown> | undefined> {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(message, (response) => {
      resolve(chrome.runtime.lastError === undefined ? response as Record<string, unknown> | undefined : undefined);
    });
  });
}

async function awaitConfigAck(response: Record<string, unknown> | undefined): Promise<Record<string, unknown> | undefined> {
  if (response?.pending !== true || typeof response.requestId !== "string") return response;
  for (let attempt = 0; attempt < 50; attempt += 1) {
    await new Promise((resolve) => { setTimeout(resolve, 100); });
    const status = await send({ type: "popup.operation", requestId: response.requestId });
    if (status?.pending !== true) return status;
  }
  return { ok: false, error: "operation_timeout" };
}

function requestScopePermission(scope: string): Promise<boolean> {
  return new Promise((resolve) => {
    chrome.permissions.request({ origins: [`*://${scope}/*`, `*://*.${scope}/*`] }, (granted) => {
      resolve(chrome.runtime.lastError === undefined && granted);
    });
  });
}

let locale: Locale = "tr";

async function checkConnection(): Promise<void> {
  installCheck.disabled = true;
  installStatus.hidden = false;
  installStatus.textContent = t(locale, "onboarding.install.waiting");
  const state = await send({ type: "popup.state" });
  installCheck.disabled = false;
  if (state?.connected === true) {
    installStatus.textContent = t(locale, "onboarding.install.connected");
    const candidates = parseRecoveryCandidates(state.recoveryCandidates);
    if (candidates.length > 0) {
      renderRecoveryCandidates(candidates);
      recoveryPanel.hidden = false;
      installSkip.textContent = t(locale, "onboarding.recovery.startFresh");
    } else {
      setTimeout(() => showStep(stepAddsite), 600);
    }
  } else {
    // state is undefined when the message never got a response at all (e.g. this tab was left
    // open across an extension reload and its page context is stale) — same actionable hint
    // either way: reload this tab and try again.
    installStatus.textContent = t(locale, "onboarding.install.notConnected");
  }
}

function parseRecoveryCandidates(value: unknown): RecoveryCandidate[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw): RecoveryCandidate[] => {
    if (typeof raw !== "object" || raw === null) return [];
    const candidate = raw as Partial<RecoveryCandidate>;
    return typeof candidate.profileId === "string" && typeof candidate.displayName === "string" &&
      typeof candidate.browser === "string" && typeof candidate.lastUsedUnixMs === "number" &&
      typeof candidate.siteCount === "number"
      ? [candidate as RecoveryCandidate] : [];
  });
}

function renderRecoveryCandidates(candidates: RecoveryCandidate[]): void {
  recoveryList.replaceChildren();
  candidates.forEach((candidate, index) => {
    const card = document.createElement("div");
    card.className = "recovery-card";
    const copy = document.createElement("div");
    const name = document.createElement("strong");
    name.textContent = candidate.displayName;
    if (index === 0) {
      const badge = document.createElement("span");
      badge.className = "recovery-badge";
      badge.textContent = t(locale, "onboarding.recovery.recommended");
      name.append(badge);
    }
    const details = document.createElement("small");
    const date = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" })
      .format(new Date(candidate.lastUsedUnixMs));
    details.textContent = t(locale, "onboarding.recovery.details")
      .replace("{browser}", candidate.browser)
      .replace("{count}", String(candidate.siteCount))
      .replace("{date}", date);
    copy.append(name, details);
    const button = document.createElement("button");
    button.type = "button";
    button.className = "primary";
    button.textContent = t(locale, "onboarding.recovery.button");
    button.addEventListener("click", () => { void recoverProfile(candidate, button); });
    card.append(copy, button);
    recoveryList.append(card);
  });
}

async function recoverProfile(candidate: RecoveryCandidate, button: HTMLButtonElement): Promise<void> {
  recoveryList.querySelectorAll<HTMLButtonElement>("button").forEach((item) => { item.disabled = true; });
  recoveryStatus.hidden = false;
  recoveryStatus.textContent = t(locale, "onboarding.recovery.waiting");
  try {
    const response = await awaitConfigAck(await send({ type: "popup.recover", profileId: candidate.profileId }));
    if (response?.ok !== true) throw new Error("recovery_failed");
    recoveryStatus.textContent = t(locale, "onboarding.recovery.success");
    setTimeout(() => showStep(stepDone), 700);
  } catch {
    recoveryStatus.textContent = t(locale, "onboarding.recovery.error");
    recoveryList.querySelectorAll<HTMLButtonElement>("button").forEach((item) => { item.disabled = false; });
    button.focus();
  }
}

async function scopeOverlapsExisting(scope: string): Promise<boolean> {
  const state = await send({ type: "popup.state" });
  const groups = Array.isArray(state?.groups) ? state.groups as Array<{ scope: string }> : [];
  return groups.some((group) => {
    const existing = group.scope.toLowerCase();
    return scope === existing || scope.endsWith(`.${existing}`) || existing.endsWith(`.${scope}`);
  });
}

function showAddsiteError(key: string): void {
  addsiteScope.setAttribute("aria-invalid", "true");
  addsiteDomainInput.classList.add("has-error");
  addsiteError.textContent = t(locale, key);
  addsiteError.hidden = false;
  addsiteScope.focus();
}

function clearAddsiteError(): void {
  addsiteScope.removeAttribute("aria-invalid");
  addsiteDomainInput.classList.remove("has-error");
  addsiteError.hidden = true;
}

function normalizeAddsiteScope(): string {
  const scope = normalizeProtectionScopeInput(addsiteScope.value);
  if (scope !== "") addsiteScope.value = scope;
  return scope;
}

async function addFirstSite(): Promise<void> {
  clearAddsiteError();
  const scope = normalizeAddsiteScope();
  if (scope === "") return showAddsiteError("onboarding.addsite.error.empty");
  if (!isValidProtectionScope(scope)) return showAddsiteError("onboarding.addsite.error.invalid");
  if (await scopeOverlapsExisting(scope)) return showAddsiteError("onboarding.addsite.error.overlap");
  const submitButton = required<HTMLButtonElement>("addsite-submit");
  submitButton.disabled = true;
  try {
    const policyLevel = addsitePolicy.value as PolicyLevel;
    await send({ type: "popup.stageProtect", scope, displayName: scope, policyLevel });
    const granted = await requestScopePermission(scope);
    if (!granted) {
      await send({ type: "popup.cancelProtect" });
      return showAddsiteError("onboarding.addsite.error.permission");
    }
    const response = await awaitConfigAck(await send({ type: "popup.protect" }));
    if (response?.ok !== true) return showAddsiteError("onboarding.addsite.error.generic");
    showStep(stepDone);
  } catch { showAddsiteError("onboarding.addsite.error.generic"); }
  finally { submitButton.disabled = false; }
}

welcomeNext.addEventListener("click", () => showStep(stepInstall));
installCheck.addEventListener("click", () => { void checkConnection(); });
installSkip.addEventListener("click", () => showStep(stepAddsite));
addsiteForm.addEventListener("submit", (event) => { event.preventDefault(); void addFirstSite(); });
addsiteScope.addEventListener("input", clearAddsiteError);
addsiteScope.addEventListener("blur", () => { normalizeAddsiteScope(); });
addsiteSkip.addEventListener("click", () => showStep(stepDone));
doneFinish.addEventListener("click", () => {
  sessionStorage.removeItem(ONBOARDING_STEP_KEY);
  window.close();
});

void (async () => {
  locale = await currentLocale();
  document.documentElement.lang = locale;
  applyTranslations(locale);
  enhanceSelects();
  restoreStep();
})();
