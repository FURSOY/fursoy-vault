import assert from "node:assert/strict";
import { GuardedRemovalPlan, snapshotsEqual } from "../dist/guarded-removal.js";

const cookie = (name, value) => ({ domain: ".example.com", host_only: false, http_only: true,
  name, path: "/", same_site: "lax", secure: true, session: true, store_id: "0", value });
const committed = [cookie("a", "1"), cookie("b", "2")];
assert.equal(snapshotsEqual(committed, [cookie("a", "rotated"), cookie("b", "2")]), false);

const plan = new GuardedRemovalPlan(committed);
assert.equal(plan.next(committed).record.name, "a");
assert.deepEqual(plan.next([cookie("b", "rotated")]), { mutation: true });

const partial = new GuardedRemovalPlan(committed);
partial.next(committed);
assert.equal(partial.next([cookie("b", "2")]).record.name, "b");
assert.deepEqual(partial.next([cookie("new", "3")]), { mutation: true });

const siblingDisappeared = new GuardedRemovalPlan(committed);
assert.equal(siblingDisappeared.next(committed).record.name, "a");
assert.deepEqual(siblingDisappeared.next([]), { done: true });

const changedRemaining = new GuardedRemovalPlan(committed);
assert.equal(changedRemaining.next(committed).record.name, "a");
assert.deepEqual(changedRemaining.next([cookie("b", "rotated")]), { mutation: true });

console.log("guarded removal tests passed");
