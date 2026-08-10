import { translate, type Locale } from "./i18n.js";
import { currentLocale } from "./locale.js";

type UnlockStatus = "ready" | "unlocking" | "recovering" | "redirecting" | "error";

interface UnlockResponse {
  ok?: boolean;
  status?: UnlockStatus;
  error?: string;
}

const button = requiredElement<HTMLButtonElement>("unlock");
const statusText = requiredElement<HTMLElement>("status");
const buttonLabelCandidate = button.querySelector<HTMLElement>(".unlock-label");
if (buttonLabelCandidate === null) throw new Error("missing unlock button label");
const buttonLabel: HTMLElement = buttonLabelCandidate;
const trustLabelBox = requiredElement<HTMLElement>("trust-strip");
let requestActive = false;
let pollInFlight = false;
let locale: Locale = "tr";
let currentStatus: UnlockStatus = "ready";

function t(key: string): string {
  return translate(locale, key, undefined);
}

function applyTranslations(): void {
  document.documentElement.lang = locale;
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((element) => {
    const key = element.dataset.i18n;
    if (key !== undefined) element.textContent = t(key);
  });
  trustLabelBox.setAttribute("aria-label", t("unlock.trustLabel"));
  render(currentStatus);
}

button.addEventListener("click", () => {
  requestActive = true;
  render("unlocking");
  sendUnlockMessage("unlock.start").then(handleResponse, () => {
    requestActive = false;
    render("error");
  });
});

setInterval(() => {
  if (pollInFlight) return;
  pollInFlight = true;
  sendUnlockMessage("unlock.status")
    .then(handleResponse, () => {
      requestActive = false;
      render("error");
    })
    .finally(() => { pollInFlight = false; });
}, 300);

void sendUnlockMessage("unlock.status").then(handleResponse, () => render("error"));

void (async () => {
  locale = await currentLocale();
  applyTranslations();
})();

function handleResponse(response: unknown): void {
  const result = response as UnlockResponse | undefined;
  const status = result?.status ?? "error";
  if (status === "ready" || status === "error") requestActive = false;
  render(status);
}

function render(status: UnlockStatus): void {
  currentStatus = status;
  document.body.dataset.status = status;
  switch (status) {
    case "ready":
      statusText.textContent = t("unlock.status.ready");
      buttonLabel.textContent = t("unlock.button.ready");
      button.disabled = false;
      break;
    case "unlocking":
      statusText.textContent = t("unlock.status.unlocking");
      buttonLabel.textContent = t("unlock.button.unlocking");
      button.disabled = true;
      break;
    case "recovering":
      statusText.textContent = t("unlock.status.recovering");
      buttonLabel.textContent = t("unlock.button.recovering");
      button.disabled = true;
      break;
    case "redirecting":
      statusText.textContent = t("unlock.status.redirecting");
      buttonLabel.textContent = t("unlock.button.redirecting");
      button.disabled = true;
      break;
    case "error":
      statusText.textContent = t("unlock.status.error");
      buttonLabel.textContent = t("unlock.button.error");
      button.disabled = requestActive;
      break;
  }
}

function sendUnlockMessage(type: "unlock.status" | "unlock.start"): Promise<unknown> {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendMessage({ type }, (response) => {
      const error = chrome.runtime.lastError;
      error === undefined ? resolve(response) : reject(new Error("extension message failed"));
    });
  });
}

function requiredElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) throw new Error(`missing required element ${id}`);
  return element as T;
}

export {};
