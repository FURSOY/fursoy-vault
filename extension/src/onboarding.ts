import { enhanceSelects } from "./custom-select.js";
import { translate, type Locale } from "./i18n.js";
import { currentLocale } from "./locale.js";
import { isValidProtectionScope, normalizeProtectionScopeInput } from "./protocol.js";

type PolicyLevel = "critical" | "balanced" | "convenient" | "monitor";

// GitHub always redirects this exact URL to the latest release's same-named asset — see the
// matching comment in native-host/install/package-release.ps1, which is what must keep producing
// a zip with this exact filename for the link to never go stale.
const INSTALLER_DOWNLOAD_URL = "https://github.com/FURSOY/fursoy-vault/releases/latest/download/fursoy-vault-windows.zip";

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
const addsiteForm = required<HTMLFormElement>("addsite-form");
const addsiteScope = required<HTMLInputElement>("addsite-scope");
const addsitePolicy = required<HTMLSelectElement>("addsite-policy");
const addsiteError = required<HTMLElement>("addsite-error");
const addsiteSkip = required<HTMLButtonElement>("addsite-skip");
const doneFinish = required<HTMLButtonElement>("done-finish");

function showStep(step: HTMLElement): void {
  const steps = [stepWelcome, stepInstall, stepAddsite, stepDone];
  const names = ["welcome", "install", "addsite", "done"];
  const activeIndex = steps.indexOf(step);
  for (const section of steps) section.hidden = section !== step;
  document.body.dataset.step = names[activeIndex];
  document.querySelectorAll<HTMLElement>(".step-dot").forEach((dot, index) => {
    dot.classList.toggle("active", index === activeIndex);
    dot.classList.toggle("past", index < activeIndex);
  });
  document.querySelectorAll<HTMLElement>(".step-line").forEach((line, index) => {
    line.classList.toggle("past", index < activeIndex);
  });
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
  installCheck.textContent = t(locale, "onboarding.install.checkButton");
  installSkip.textContent = t(locale, "onboarding.install.skip");

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
    setTimeout(() => showStep(stepAddsite), 600);
  } else {
    // state is undefined when the message never got a response at all (e.g. this tab was left
    // open across an extension reload and its page context is stale) — same actionable hint
    // either way: reload this tab and try again.
    installStatus.textContent = t(locale, "onboarding.install.notConnected");
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
  addsiteError.textContent = t(locale, key);
  addsiteError.hidden = false;
}

async function addFirstSite(): Promise<void> {
  addsiteError.hidden = true;
  const scope = normalizeProtectionScopeInput(addsiteScope.value);
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
addsiteSkip.addEventListener("click", () => showStep(stepDone));
doneFinish.addEventListener("click", () => window.close());

void (async () => {
  locale = await currentLocale();
  document.documentElement.lang = locale;
  applyTranslations(locale);
  enhanceSelects();
})();
