import assert from "node:assert/strict";
import {
  COOKIE_CHUNK_TARGET_BYTES,
  MAX_COOKIE_CHUNKS,
  MAX_COOKIE_RECORDS,
  OrderedChunkAssembler,
  chunkRecords,
} from "../dist/cookie-chunks.js";

// Synthetic values only: no browser/session material is written to test output or artifacts.
const records = [0, 1, 2, 3].map((id) => ({ id, value: "x".repeat(300 * 1024) }));
const chunks = chunkRecords(records);
assert.ok(Buffer.byteLength(JSON.stringify(records)) > 1024 * 1024, "fixture must exercise >1 MiB aggregate transfer");
assert.ok(chunks.length > 1);
assert.deepEqual(chunks.flat().map((record) => record.id), [0, 1, 2, 3]);
for (const chunk of chunks) {
  assert.ok(Buffer.byteLength(JSON.stringify(chunk)) <= COOKIE_CHUNK_TARGET_BYTES, "each chunk stays bounded");
}

assert.throws(
  () => chunkRecords([{ value: "x".repeat(COOKIE_CHUNK_TARGET_BYTES + 1) }]),
  /one cookie exceeds/,
);

const assembler = new OrderedChunkAssembler();
assert.equal(assembler.receive("group-a", "lease-a", 0, 2, 3, [{ id: 1 }, { id: 2 }]), undefined);
assert.equal(assembler.pendingTransferCount(), 1);
assert.deepEqual(assembler.receive("group-a", "lease-a", 1, 2, 3, [{ id: 3 }]), [{ id: 1 }, { id: 2 }, { id: 3 }]);
assert.equal(assembler.pendingTransferCount(), 0);

assert.throws(() => assembler.receive("group-b", "lease-b", 1, 2, 1, [{ id: 1 }]), /first inject chunk is missing/);
assert.throws(() => assembler.receive("group-b", "lease-b", 0, MAX_COOKIE_CHUNKS + 1, 1, [{ id: 1 }]), /metadata is out of range/);
assert.throws(() => assembler.receive("group-b", "lease-b", 0, 1, MAX_COOKIE_RECORDS + 1, [{ id: 1 }]), /metadata is out of range/);

// A mismatch exposes the current reconnect weakness without treating it as desired behavior.
assert.equal(assembler.receive("group-c", "lease-old", 0, 2, 2, [{ id: 1 }]), undefined);
assert.throws(() => assembler.receive("group-c", "lease-new", 0, 1, 1, [{ id: 2 }]), /binding or order mismatch/);
assert.equal(assembler.pendingTransferCount(), 0);
assert.deepEqual(assembler.receive("group-c", "lease-new", 0, 1, 1, [{ id: 2 }]), [{ id: 2 }]);

console.log("cookie chunk tests: PASS");
