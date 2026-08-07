import assert from "node:assert/strict";
import {
  cookieUrl,
  guessScope,
  hostInScope,
  navigationPatterns,
  validateConfig,
} from "../dist/protocol.js";

function group(overrides = {}) {
  return {
    id: "7a144677-3f5c-4a86-a767-16fd3ca315b8",
    display_name: "Test",
    scope: "example.com",
    policy_level: "balanced",
    eviction_triggers: ["last_tab_closed", "idle", "lock", "expiry", "manual"],
    store_policy: "normal_profile",
    ...overrides,
  };
}

function config(groups) {
  return { version: 2, compatibility_version: 2, groups };
}

// A registrable domain must own its subdomains, and nothing outside it.
assert.equal(hostInScope("example.com", "example.com"), true);
assert.equal(hostInScope("example.com", "mail.example.com"), true);
assert.equal(hostInScope("example.com", ".example.com"), true);
assert.equal(hostInScope("example.com", "notexample.com"), false);
assert.equal(hostInScope("example.com", "example.com.evil.test"), false);

// ADR-015: the URL authorizing a cookie operation is rebuilt without a port.
assert.equal(cookieUrl({ domain: ".wikipedia.org", path: "/", secure: true }), "https://wikipedia.org/");
assert.equal(cookieUrl({ domain: "localhost", path: "/", secure: false }), "http://localhost/");

assert.deepEqual(navigationPatterns(group()), ["*://example.com/*", "*://*.example.com/*"]);

// Scope guessing: the popup shows this, so it only has to be right for the common cases.
assert.equal(guessScope("tr.wikipedia.org"), "wikipedia.org");
assert.equal(guessScope("example.com"), "example.com");
assert.equal(guessScope("www.example.co.uk"), "example.co.uk");
assert.equal(guessScope("shop.example.com.tr"), "example.com.tr");
assert.equal(guessScope("localhost"), "localhost");
assert.equal(guessScope("127.0.0.1"), "127.0.0.1");

validateConfig(config([group()]));

// Nested scopes would make cookie ownership ambiguous between two groups.
assert.throws(() => validateConfig(config([
  group(),
  group({ id: "c2b71d84-6d3f-45d4-a184-994083ba7659", scope: "mail.example.com" }),
])));

// Sibling scopes are fine.
validateConfig(config([
  group(),
  group({ id: "c2b71d84-6d3f-45d4-a184-994083ba7659", scope: "example.org" }),
]));

// A bare label is an internal page hostname (chrome://newtab/), not a registrable domain.
for (const scope of ["", "example..com", "example.com.", "example.com/path", "ex ample.com", "newtab", "extensions"]) {
  assert.throws(() => validateConfig(config([group({ scope })])), undefined, `scope ${JSON.stringify(scope)} must be rejected`);
}

assert.throws(() => validateConfig({ version: 1, compatibility_version: 1, groups: [group()] }));

console.log("protocol tests: PASS");
