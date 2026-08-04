# FURSOY Native Messaging Protocol v1

Phase 5 uses Chrome's fixed native-messaging framing: a 4-byte little-endian JSON byte length,
followed by one UTF-8 JSON object. Both directions apply a conservative 1 MiB frame ceiling.

Every object uses the envelope in `envelope.schema.json`. `conn_nonce` is fresh per connection,
`seq` starts at 1 and increases by exactly one, and `id` is a non-nil UUID. The Phase 5 message
set is deliberately restricted to handshake, lease, inject, and verified eviction transactions.

`session.invalidate` / `session.invalidated` is the fail-closed terminal path for an external site
logout or a restore that the site's authenticated health check rejects. It deletes the stale encrypted
group payload, resets the lease to `UNINITIALIZED`, and never displays Hello. This prevents an invalid
server-side session from being offered repeatedly on later tab navigations.

`evict.confirmed.cookie_disposition` separates durable snapshot confirmation from physical cookie
removal. Initial enrollment returns `retain_leased`: the extension verifies the cookie remains and
the host finalizes `LEASED`. Real eviction/reconciliation returns `remove`: the extension removes
and verifies zero remaining cookies before the host finalizes `SEALED`.

`handshake.ack` reports the host's durable group state together with the current lease id/expiry; it
does not mutate that state or start reconciliation by itself. The extension first refreshes the
actual Chrome tab/cookie observations, then selects exactly one action. A healthy `LEASED` state
with a live relevant tab and present cookie resumes. No relevant tab selects `last_tab_closed`;
missing/inconsistent browser state or a durable transitional/degraded state selects
`startup_reconciliation`. This ordering prevents a cold MV3 worker handshake from shadowing the
`tabs.onRemoved` eviction trigger.

Windows Hello capabilities are host-internal signed authorization objects rather than extension
assertions. Their canonical binary challenge is:

```text
"FCPHCAP1"
|| account_group_id[16]
|| operation[1]                 # 1=inject; no evict capability exists
|| expiry_unix_ms[u64 LE]
|| monotonic_sequence[u64 LE]
|| nonce[32]
```

The lease state machine persists the reserved sequence/nonce, verifies every bound field and the
Hello signature, then durably consumes sequence/nonce before an inject TPM DEK unwrap. Reuse is
rejected. Enrollment, eviction, and reconciliation do not create capabilities or display Hello:
they move cookie material toward the sealed state and use the TPM-backed KEK silently within one
zeroized vault transaction.

`lease.grant.capability_sequence` is a positive integer for an authorized inject and `null` for
non-interactive enrollment. Eviction and reconciliation proceed through their snapshot/result
messages without a lease capability grant.
