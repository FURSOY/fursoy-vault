# FURSOY Native Host

Production-oriented Phase 7 native component. It retains the Phase 5 TPM/Hello, FCPV v1 and atomic
vault transaction security properties while routing operations through independent account-group
runtimes.

Implemented:

- ownership of the account-group config: validation, atomic persistence, runtime `group.add` /
  `group.remove` and the SHA-256 digest published to the extension;
- Native Messaging v5 handshake with explicit product-version and capability compatibility;
- per-group vault, lease metadata, capability ledger, pending operation and reconciliation state;
- automatic migration of the Phase 5 Wikipedia lease/capability files to UUID-based paths;
- host-authoritative Critical/Balanced/Convenient/Monitor policy parameters;
- one process-lifetime WinRT apartment with group-scoped Hello handle caches;
- fresh single-use sequence/nonce capability for every inject, including cached-gesture injects;
- cache clearing on Windows lock and process/connection teardown;
- HMAC-SHA256 chained redacted audit with a DPAPI-protected key, rollback-detecting anchor,
  90-day retention checkpoint and verified JSONL export;
- bounded monitoring transport driven by WMI process-start events; only Authenticode-valid Chrome
  executables under known Google Chrome install roots are inspected, without retaining command
  lines, ports or profile paths;
- failure isolation: a group-level operation error does not mutate another group's runtime.

The installer seeds `%LOCALAPPDATA%\FursoyVault\config\account-groups.json` only when it
does not already exist; once sites can be added at runtime (ADR-020) that file is user data and a
reinstall must not overwrite it. The executable still carries a bundled config as a first-run
fallback.

Close Chrome before exporting the retained, redacted audit chain:

```text
fursoy-vault-host.exe --export-audit C:\path\audit.jsonl
```

Installation uses immutable versioned deployments and switches the Native Messaging manifest only
after the new executable is present. The default uninstaller only disables integration; pass
`-Purge` to explicitly erase all local vault, configuration, audit and companion data.

Still excluded: incognito/multi-profile support, persistent Windows agent, profile-directory access
attribution, kernel drivers and management UI. Monitoring is best-effort detection, not blocking.

## Checks

```text
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
```
