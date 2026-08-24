// i18n mechanism only — no message keys are wired into the UI yet. Kept dependency-free (no
// chrome.* calls) so it is unit-testable the same way monitor.ts is; each page resolves and
// persists its own current locale via chrome.storage/chrome.i18n and calls translate() with it.
import { en } from "./locales/en.js";
import { tr } from "./locales/tr.js";

export type Locale = "tr" | "en";

export const SUPPORTED_LOCALES: readonly Locale[] = ["tr", "en"];
export const DEFAULT_LOCALE: Locale = "en";
export const LOCALE_STORAGE_KEY = "fcp-locale-v1";

const MESSAGES: Record<Locale, Record<string, string>> = { tr, en };

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && (SUPPORTED_LOCALES as readonly string[]).includes(value);
}

// Maps a BCP-47 language tag (e.g. chrome.i18n.getUILanguage()'s "en-US", "tr") to the nearest
// supported locale, falling back to the default when nothing matches.
export function resolveLocale(tag: string): Locale {
  const base = tag.split("-")[0]?.toLowerCase();
  return isLocale(base) ? base : DEFAULT_LOCALE;
}

export type Platform = "win" | "linux";

/// Set once per page from chrome.runtime.getPlatformInfo. Left unset the base key is used, which
/// is why base text is written platform-neutrally: an unresolved platform must still read
/// correctly rather than assert something about the machine.
let activePlatform: Platform | undefined;

export function setPlatform(platform: Platform): void {
  activePlatform = platform;
}

/// Whether a key has anything to say on this platform at all. Lets a page hide an element whose
/// advice only applies elsewhere, rather than translating it into something vague.
export function hasMessage(locale: Locale, key: string): boolean {
  const suffixed = activePlatform === undefined ? undefined : `${key}.${activePlatform}`;
  const table = MESSAGES[locale] ?? MESSAGES[DEFAULT_LOCALE];
  return [suffixed, key].some(
    (candidate) => candidate !== undefined && (table[candidate] ?? MESSAGES[DEFAULT_LOCALE][candidate]) !== undefined,
  );
}

// A platform-specific variant wins when one exists: "key.linux" before "key". Most text needs no
// variant, because only instructions genuinely differ between platforms — what a thing is called
// should not.
//
// Missing keys fall back to the default locale's text, then to the raw key itself, so a partially
// translated locale (or a key not yet migrated into either table) never renders blank.
export function translate(locale: Locale, key: string, params?: Readonly<Record<string, string | number>>): string {
  const table = MESSAGES[locale] ?? MESSAGES[DEFAULT_LOCALE];
  const lookup = (candidate: string): string | undefined => table[candidate] ?? MESSAGES[DEFAULT_LOCALE][candidate];
  const suffixed = activePlatform === undefined ? undefined : lookup(`${key}.${activePlatform}`);
  let text = suffixed ?? lookup(key) ?? key;
  if (params !== undefined) {
    for (const [name, value] of Object.entries(params)) text = text.replaceAll(`{${name}}`, String(value));
  }
  return text;
}
