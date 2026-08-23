# linux-authorizer (Faz L0 prototype)

Answers one question: can a TPM 2.0 key gated by a PIN stand in for the Windows Hello authorizer,
and what shape does its proof have to take? Standalone on purpose — it does not depend on the
native host and nothing in the host depends on it.

## Result

It satisfies the contract. `cargo run` against a TPM prints:

```
PASS  enrolls a PIN-gated key
PASS  signs with the correct PIN
PASS  signature is 64 raw bytes
PASS  signature verifies
PASS  tampered payload is rejected
PASS  wrong PIN is refused
```

## What this settles about the abstraction

`SignedCapability` has three fields. Comparing the two backends:

| Field | Windows Hello | TPM + PIN |
|---|---|---|
| `payload` | same | same |
| `signature` | 64 raw bytes (r‖s), DER-decoded from WebAuthn | 64 raw bytes (r‖s), assembled from the TPM's separate r and s buffers |
| `authenticator_data` | required — carries rpIdHash, the UP/UV flags and signCount | **has no equivalent, and needs none** |

So the signature field is already identical and only `authenticator_data` is backend-specific.

The reason it is not needed here is worth stating precisely, because it is the security argument
for this backend. WebAuthn carries a user-verified *flag* because an authenticator may or may not
have verified the user, so the fact has to be asserted and then checked. A TPM key created with an
`authValue` cannot produce a signature at all unless the correct PIN was supplied: verification is
structural rather than asserted, and the existence of the signature is the proof. There is no flag
to forge — but equally, nothing would stop a key from being created *without* an auth requirement,
so `assert_requires_auth` refuses to sign with such a key at load time. That check is what makes
the argument hold.

The TPM also applies its own dictionary-attack lockout to wrong PINs. That is what makes a
six-digit PIN defensible; without it the PIN would be trivially brute-forceable offline.

Since `SignedCapability` is created, verified and dropped inside the host process — it is never
persisted (`CapabilityLedger` stores the unsigned `CapabilityPayload`) and never crosses the
protocol to the extension — making `authenticator_data` backend-specific costs no protocol version
and no vault migration.

## Two implementation notes for the real backend

**Transient object slots are scarce.** Every operation that loads a context consumes one, and the
simulator ran out after three leaked handles during bring-up. Every handle opened here is flushed
on the way out, including on error paths. No persistent slot is consumed either: TPM primary keys
are deterministic for a given seed and template, so the parent is recreated per operation rather
than being evicted to a persistent handle.

**No DER parsing.** The TPM returns r and s as separate buffers, so unlike the WebAuthn path this
backend needs none of `webauthn_codec`'s DER handling — only left-padding each to the curve size.

## Running it

Against the swtpm simulator:

```bash
rm -rf /tmp/swtpm-state && mkdir -p /tmp/swtpm-state
swtpm socket --tpm2 --tpmstate dir=/tmp/swtpm-state \
  --ctrl type=tcp,port=2322 --server type=tcp,port=2321 \
  --flags not-need-init,startup-clear --daemon
export TPM2TOOLS_TCTI="swtpm:host=127.0.0.1,port=2321"
cargo run
```

Against a real TPM, drop `TPM2TOOLS_TCTI` — the default is `device:/dev/tpmrm0` — and make sure
the user can reach the device (usually membership of the `tss` group).

`swtpm` is a simulator: lockout thresholds, slot counts and vendor behaviour differ on real
hardware, so a real-TPM run is what actually validates this.

Individual commands: `enroll <pin>`, `sign <pin> <payload>`, `verify <payload> <signature-hex>`.
`LINUX_AUTHORIZER_REGISTRY` overrides where the credential registry is written.

The expected-failure case logs a `tss2` error to stderr on its way to passing; that is the library
reporting `TPM_RC_AUTH_FAIL`, not a fault. `export TSS2_LOG=all+NONE` silences it.
