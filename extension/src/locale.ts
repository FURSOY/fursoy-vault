// The chrome.storage/chrome.i18n-touching half of the i18n mechanism — kept separate from
// i18n.ts so that file stays pure and unit-testable (see its own comment).
import { LOCALE_STORAGE_KEY, isLocale, resolveLocale, setPlatform, type Locale, type Platform } from "./i18n.js";

function getStoredLocale(): Promise<Locale | undefined> {
  return new Promise((resolve) => {
    chrome.storage.local.get(LOCALE_STORAGE_KEY, (items: Record<string, unknown>) => {
      const value = items[LOCALE_STORAGE_KEY];
      resolve(isLocale(value) ? value : undefined);
    });
  });
}

function setStoredLocale(locale: Locale): Promise<void> {
  return new Promise((resolve) => {
    chrome.storage.local.set({ [LOCALE_STORAGE_KEY]: locale }, () => resolve());
  });
}

// First call ever made (no stored preference yet) detects from the browser's UI language and
// persists it, so it only has to be detected once; a settings-page language switcher (not built
// yet) would just call setStoredLocale directly.
export async function currentLocale(): Promise<Locale> {
  const stored = await getStoredLocale();
  if (stored !== undefined) return stored;
  const detected = resolveLocale(chrome.i18n.getUILanguage());
  await setStoredLocale(detected);
  return detected;
}

/// Resolves the platform once and hands it to the message table, so pages can keep calling
/// translate() without threading it through.
///
/// Anything that is not Windows is treated as Linux, because those are the only two the host
/// supports; a build running anywhere else has already failed to connect, and guessing "Linux"
/// there produces better wording than asserting "Windows" would.
export async function applyPlatform(): Promise<Platform> {
  const platform = await new Promise<Platform>((resolve) => {
    chrome.runtime.getPlatformInfo((info) => {
      resolve(chrome.runtime.lastError === undefined && info.os === "win" ? "win" : "linux");
    });
  });
  setPlatform(platform);
  return platform;
}
