import assert from "node:assert/strict";
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

console.log("i18n tests: PASS");
