# FURSOY Native Host

Production-oriented Phase 7 native component. It retains the Phase 5 TPM/Hello, FCPV v1 and atomic
vault transaction security properties while routing operations through independent account-group
runtimes.

Implemented:

- ownership of the account-group config: validation, atomic persistence, runtime `group.add` /
  `group.remove` and the SHA-256 digest published to the extension;
- Native Messaging v3 handshake with the durable state of every configured group;
- per-group vault, lease metadata, capability ledger, pending operation and reconciliation state;
- automatic migration of the Phase 5 Wikipedia lease/capability files to UUID-based paths;
- host-authoritative Critical/Balanced/Convenient/Monitor policy parameters;
- one process-lifetime WinRT apartment with group-scoped Hello handle caches;
- fresh single-use sequence/nonce capability for every inject, including cached-gesture injects;
- cache clearing on Windows lock and process/connection teardown;
- HMAC-SHA256 chained redacted audit with a DPAPI-protected key and rollback-detecting anchor;
- bounded monitoring transport and one-second `Win32_Process` polling for Chrome remote-debugging
  switches without retaining the command line, port or profile path;
- failure isolation: a group-level operation error does not mutate another group's runtime.

The installer seeds `%LOCALAPPDATA%\FursoyVault\config\account-groups.json` only when it
does not already exist; once sites can be added at runtime (ADR-020) that file is user data and a
reinstall must not overwrite it. The executable still carries a bundled config as a first-run
fallback.

Still excluded: incognito/multi-profile support, persistent Windows agent, profile-directory access
attribution, kernel drivers and management UI. Monitoring is best-effort detection, not blocking.

## Checks

```text
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
```
