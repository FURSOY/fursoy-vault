import assert from "node:assert/strict";
import { decideStartup, stateAfterHostError } from "../dist/state-machine.js";

const base = { state: "sealed", cookieCount: 0, relevantTabCount: 0, reconciliationRequired: false, pendingInvalidation: false, now: 1_000 };
assert.deepEqual(decideStartup({ ...base, state: "leased", cookieCount: 0, relevantTabCount: 1 }), { action: "evict", reason: "startup_reconciliation" });
assert.deepEqual(decideStartup({ ...base, state: "leased", cookieCount: 2, relevantTabCount: 0 }), { action: "evict", reason: "last_tab_closed" });
assert.deepEqual(decideStartup({ ...base, state: "leased", cookieCount: 2, relevantTabCount: 1, leaseExpiry: 999 }), { action: "evict", reason: "expiry" });
assert.deepEqual(decideStartup({ ...base, state: "leased", cookieCount: 2, relevantTabCount: 1, leaseExpiry: 2_000 }), { action: "schedule_expiry", when: 2_000 });
assert.deepEqual(decideStartup({ ...base, state: "sealed", cookieCount: 1, relevantTabCount: 1 }), { action: "clean_sealed" });
assert.deepEqual(decideStartup({ ...base, state: "evicting", pendingInvalidation: true }), { action: "invalidate" });
assert.deepEqual(stateAfterHostError("unlocking", "inject"), { state: "sealed", reconciliation: false });
assert.deepEqual(stateAfterHostError("evicting"), { state: "degraded", reconciliation: true });

console.log("state-machine tests: PASS");
