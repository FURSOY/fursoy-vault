import assert from "node:assert/strict";
import {
  MONITOR_OUTBOX_LIMIT,
  MONITOR_RATE_LIMIT_MS,
  addToBoundedOutbox,
  notificationDecision,
  signalSeverity,
  validateMonitorEvent,
} from "../dist/monitor.js";

function event(index, signal = "lease_outside_cookie_created") {
  return {
    event_id: `00000000-0000-4000-8000-${String(index).padStart(12, "0")}`,
    observed_at_unix_ms: 1_000 + index,
    source: "extension",
    signal,
    severity: signalSeverity(signal),
    account_group_id: `00000000-0000-4000-8001-${String(index).padStart(12, "0")}`,
    occurrence_count: 1,
  };
}

assert.equal(signalSeverity("remote_debugging_port"), "high");
assert.equal(signalSeverity("permission_missing"), "high");
assert.equal(signalSeverity("lease_outside_cookie_created"), "medium");
assert.equal(signalSeverity("selector_changed"), "info");

let outbox = [];
for (let index = 0; index < MONITOR_OUTBOX_LIMIT + 5; index += 1) {
  outbox = addToBoundedOutbox(outbox, event(index));
}
assert.equal(outbox.length, MONITOR_OUTBOX_LIMIT);
assert.equal(outbox[0].event_id, event(5).event_id);

const coalesced = addToBoundedOutbox([event(1)], { ...event(2), account_group_id: event(1).account_group_id });
assert.equal(coalesced.length, 1);
assert.equal(coalesced[0].occurrence_count, 2);

const warning = event(20);
const first = notificationDecision(warning, {}, 100_000);
assert.equal(first.show, true);
assert.equal(notificationDecision(warning, first.next, 100_000 + MONITOR_RATE_LIMIT_MS - 1).show, false);
assert.equal(notificationDecision(warning, first.next, 100_000 + MONITOR_RATE_LIMIT_MS).show, true);
assert.equal(notificationDecision(event(21, "selector_changed"), {}, 100_000).show, false);

assert.equal(validateMonitorEvent(warning), true);
assert.equal(validateMonitorEvent({ ...warning, severity: "info" }), false);
console.log("monitor tests: PASS");
