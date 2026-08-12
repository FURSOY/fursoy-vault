import assert from "node:assert/strict";
import { mayAbortWithEmptySnapshot, OperationCoordinator } from "../dist/operation-coordinator.js";

let persisted;
const store = { load: async () => persisted, save: async (value) => { persisted = value; } };
const coordinator = new OperationCoordinator(store);
const groupId = "7a144677-3f5c-4a86-a767-16fd3ca315b8";
const leaseId = "11111111-1111-4111-8111-111111111111";
const operationId = "22222222-2222-4222-8222-222222222222";

const begun = await coordinator.begin(groupId, leaseId, "eviction");
assert.equal(Object.hasOwn(persisted, "cookies"), false);
assert.equal(Object.hasOwn(persisted, "snapshot"), false);
await coordinator.bindIssued(groupId, operationId, 7, leaseId, begun.attemptId);
assert.deepEqual(coordinator.statusQuery(groupId), {
  account_group_id: groupId, operation_id: operationId, operation_sequence: 7, lease_id: leaseId,
});
assert.deepEqual(coordinator.recoveryQuery(groupId), coordinator.statusQuery(groupId));

const restarted = new OperationCoordinator(store);
assert.equal((await restarted.restore())[0]?.operationId, operationId);
assert.throws(() => restarted.assertBinding({ account_group_id: groupId, operation_id: operationId,
  operation_sequence: 8, lease_id: leaseId, attempt_id: begun.attemptId }), /binding mismatch/);
restarted.assertBinding({ account_group_id: groupId, operation_id: operationId,
  operation_sequence: 7, lease_id: leaseId, attempt_id: begun.attemptId });
await restarted.complete(groupId);
assert.equal(persisted, undefined);

const removable = await restarted.begin(groupId, leaseId, "reconciliation");
assert.equal(removable.groupId, groupId);
const duplicateInFlight = await restarted.begin(groupId, leaseId, "reconciliation");
assert.equal(duplicateInFlight.attemptId, removable.attemptId);
const replacementLease = "33333333-3333-4333-8333-333333333333";
const reboundPending = await restarted.begin(groupId, replacementLease, "reconciliation");
assert.notEqual(reboundPending.attemptId, removable.attemptId);
assert.equal(reboundPending.leaseId, replacementLease);
assert.equal(reboundPending.phase, "begin_pending");
await restarted.phase(groupId, "reconciliation_required");
assert.equal(restarted.statusQuery(groupId), undefined);
assert.equal(restarted.recoveryQuery(groupId), undefined);
const retryAfterTerminalFailure = await restarted.begin(groupId, leaseId, "reconciliation");
assert.notEqual(retryAfterTerminalFailure.attemptId, reboundPending.attemptId);
assert.equal(retryAfterTerminalFailure.operationId, undefined);
assert.equal(retryAfterTerminalFailure.phase, "begin_pending");
await restarted.discardGroup(groupId);
assert.equal(restarted.current(groupId), undefined);
assert.equal(persisted, undefined);

const secondGroupId = "c2b71d84-6d3f-45d4-a184-994083ba7659";
const firstConcurrent = await restarted.begin(groupId, leaseId, "enrollment");
const secondConcurrent = await restarted.begin(secondGroupId, undefined, "reconciliation");
assert.equal(restarted.current(groupId)?.attemptId, firstConcurrent.attemptId);
assert.equal(restarted.current(secondGroupId)?.attemptId, secondConcurrent.attemptId);
assert.equal(restarted.currents().length, 2);
await restarted.bindIssued(groupId, operationId, 8, leaseId, firstConcurrent.attemptId);
assert.equal(restarted.current(secondGroupId)?.attemptId, secondConcurrent.attemptId);
assert.equal(restarted.statusQueries().length, 1);
await restarted.discardGroup(groupId);
assert.equal(restarted.current(secondGroupId)?.attemptId, secondConcurrent.attemptId);
await restarted.discardGroup(secondGroupId);
assert.equal(persisted, undefined);
assert.equal(mayAbortWithEmptySnapshot("enrollment"), true);
assert.equal(mayAbortWithEmptySnapshot("eviction"), false);
assert.equal(mayAbortWithEmptySnapshot("reconciliation"), false);

console.log("operation coordinator tests passed");
