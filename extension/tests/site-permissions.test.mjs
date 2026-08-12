import assert from "node:assert/strict";
import { permissionOrigins, shouldOfferGrantAll } from "../dist/site-permissions.js";

assert.equal(shouldOfferGrantAll(0), false);
assert.equal(shouldOfferGrantAll(1), false);
assert.equal(shouldOfferGrantAll(2), true);
assert.deepEqual(permissionOrigins(["example.com", "youtube.com"]), [
  "*://example.com/*", "*://*.example.com/*",
  "*://youtube.com/*", "*://*.youtube.com/*",
]);
assert.deepEqual(permissionOrigins(["example.com", "example.com"]), [
  "*://example.com/*", "*://*.example.com/*",
]);

console.log("site permission tests: PASS");
