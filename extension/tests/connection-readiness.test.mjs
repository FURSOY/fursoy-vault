import assert from "node:assert/strict";
import { ConnectionReadiness } from "../dist/connection-readiness.js";

const connection = new ConnectionReadiness();
assert.equal(connection.connected, false);
assert.equal(connection.ready, false);

// Opening the native port must not expose it to config mutations before handshake validation.
connection.opened();
assert.equal(connection.connected, true);
assert.equal(connection.ready, false);

connection.accepted();
assert.equal(connection.ready, true);

// A disconnect and reconnect always require a fresh ACK; readiness cannot leak across ports.
connection.closed();
assert.equal(connection.connected, false);
assert.equal(connection.ready, false);
connection.opened();
assert.equal(connection.ready, false);
assert.throws(() => { connection.closed(); connection.accepted(); }, /disconnected during handshake/);

console.log("connection readiness tests: PASS");
