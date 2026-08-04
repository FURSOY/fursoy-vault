# FURSOY Native Host

This is the production-oriented Phase 5 native component. It is intentionally limited to one fixed
controlled account group.

Implemented:

- frozen FCPV v1 group-file format (Candidate A: wrapped DEK only in the group file);
- AES-256-GCM seal/open with authenticated header and strict parsing;
- Microsoft Platform Crypto Provider RSA-2048-OAEP-SHA256 KEK primitives, with hardware-only and
  TPM 2.0 checks and no CNG UI policy;
- inject-only Windows Hello capability signing/verification over a canonical five-field challenge;
- one lazily-created WinRT authorizer runtime retained for the complete native connection, while
  every capability opens a fresh KeyCredential handle to prevent same-handle gesture-cache reuse;
- durable inject-capability sequence/nonce reservation and consume-before-unwrap replay ledger;
- verified write-through atomic replacement for vault and ledger files;
- strict Native Messaging v1 DTOs, envelope validation and framing.
- consume-before-unwrap vault transactions with transaction-scoped zeroized DEKs;
- durable lease metadata, startup reconciliation, enrollment, inject and eviction dispatch;
- external-logout/restore-rejection session invalidation with encrypted-vault deletion and
  missing-vault crash recovery to `UNINITIALIZED`;
- four-byte little-endian stdin/stdout framing with connection nonce and sequence validation;
- redacted JSONL audit events and a per-user Chrome registration script.

Deferred beyond the single-group MVP:

- real account support, multiple groups, policies, watcher or product UI.

## Checks

```text
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

Automated tests use synthetic payloads only. They do not create a TPM key or display Windows Hello;
the manual acceptance flow performs those interactive checks.
