# FURSOY Native Host

Production-oriented Phase 6 native component. It retains the Phase 5 TPM/Hello, FCPV v1 and atomic
vault transaction security properties while routing operations through independent account-group
runtimes.

Implemented:

- validated, bounded account-group config and SHA-256 config digest;
- Native Messaging v2 handshake with the durable state of every configured group;
- per-group vault, lease metadata, capability ledger, pending operation and reconciliation state;
- automatic migration of the Phase 5 Wikipedia lease/capability files to UUID-based paths;
- host-authoritative Critical/Balanced/Convenient/Monitor policy parameters;
- one process-lifetime WinRT apartment with group-scoped Hello handle caches;
- fresh single-use sequence/nonce capability for every inject, including cached-gesture injects;
- cache clearing on Windows lock and process/connection teardown;
- group-scoped redacted audit events that structurally accept no cookie name or value;
- failure isolation: a group-level operation error does not mutate another group's runtime.

The installer copies the exact account-group config to
`%LOCALAPPDATA%\FursoyCookieProtector\config\account-groups.json`. The executable also contains the same
config as a fail-closed fallback.

Still excluded: incognito/multi-profile support, persistent Windows agent and management UI.

## Checks

```text
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
```
