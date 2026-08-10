import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DEFAULT_LOCALE, SUPPORTED_LOCALES, isLocale, resolveLocale, translate } from "../dist/i18n.js";

assert.deepEqual(SUPPORTED_LOCALES, ["tr", "en"]);
assert.equal(DEFAULT_LOCALE, "tr");

assert.equal(isLocale("tr"), true);
assert.equal(isLocale("en"), true);
assert.equal(isLocale("fr"), false);
assert.equal(isLocale(undefined), false);
assert.equal(isLocale(42), false);

assert.equal(resolveLocale("tr"), "tr");
assert.equal(resolveLocale("en-US"), "en");
assert.equal(resolveLocale("EN-GB"), "en");
assert.equal(resolveLocale("fr-FR"), DEFAULT_LOCALE);
assert.equal(resolveLocale(""), DEFAULT_LOCALE);

// No message tables are populated yet, so translate() must fall back to the raw key rather than
// rendering blank — this is the contract callers can already build against before any key exists.
assert.equal(translate("tr", "some.unmigrated.key"), "some.unmigrated.key");
assert.equal(translate("en", "some.unmigrated.key"), "some.unmigrated.key");

// Param substitution still applies to the fallback text (the key itself) so the mechanism is
// fully exercised even with empty tables.
assert.equal(translate("tr", "hello {name}", { name: "world" }), "hello world");

// The two message tables are hand-maintained in parallel; a key added to one and forgotten in the
// other would silently fall back to the default-locale (or raw-key) text instead of failing, so
// key-set parity is checked directly against the source rather than left to be noticed visually.
function keysOf(path) {
  const source = readFileSync(new URL(path, import.meta.url), "utf8");
  return new Set([...source.matchAll(/^\s*"([^"]+)":/gm)].map((match) => match[1]));
}
const trKeys = keysOf("../src/locales/tr.ts");
const enKeys = keysOf("../src/locales/en.ts");
assert.ok(trKeys.size > 0, "tr.ts key extraction found nothing; the regex or file path is stale");
const missingFromEn = [...trKeys].filter((key) => !enKeys.has(key));
const missingFromTr = [...enKeys].filter((key) => !trKeys.has(key));
assert.deepEqual(missingFromEn, [], `keys present in tr.ts but missing from en.ts: ${missingFromEn.join(", ")}`);
assert.deepEqual(missingFromTr, [], `keys present in en.ts but missing from tr.ts: ${missingFromTr.join(", ")}`);

console.log("i18n tests: PASS");
