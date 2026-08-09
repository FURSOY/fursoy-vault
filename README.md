# FURSOY Vault

FURSOY Vault is a Windows + Chrome session-cookie vault. It removes cookies for sites the user
explicitly protects, stores them encrypted by TPM/Windows Hello, and restores them after a fresh
Hello approval. It is deliberately narrower than a password manager or a general browser-security
suite: it protects Chrome cookie sessions, not localStorage, IndexedDB, service-worker state,
downloaded files, passwords, other browsers or a compromised Windows account.

The product has four policies: Critical, Balanced and Convenient actively vault cookies; Monitor
only records best-effort signals and never mutates browser state. Monitoring is detection, not an
EDR or process blocker. Incognito and multi-profile attribution are currently unsupported.

## Build and verify

```text
cd extension
npm ci
npm run package

cd ..\native-host
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --release --locked
powershell -File install/package-release.ps1
```

Windows acceptance coverage and the hardware/manual rows are documented in
`tests/acceptance/MATRIX.md`.

## License and source

Copyright contributors. Licensed under GPL-3.0-only; see `LICENSE`. Release assets include both the
license and `SOURCE.txt`, which identifies the matching complete source tag and build procedure.
