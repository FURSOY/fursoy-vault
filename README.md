# FURSOY Vault

FURSOY Vault is a Windows + Chrome session-cookie vault. It removes cookies for sites the user
explicitly protects, stores them encrypted by TPM/Windows Hello, and restores them after a fresh
Hello approval. It is deliberately narrower than a password manager or a general browser-security
suite: it protects Chrome cookie sessions, not localStorage, IndexedDB, service-worker state,
downloaded files, passwords, other browsers or a compromised Windows account.

The product has four policies: Critical, Balanced and Convenient actively vault cookies; Monitor
only records best-effort signals and never mutates browser state. Monitoring is detection, not an
EDR or process blocker. Each regular Chrome profile receives an isolated vault namespace;
incognito and browsers other than Chrome are currently unsupported.

Chrome profiles are intentionally isolated and a vault can never be listed, copied or claimed by
another profile. Clearing extension storage or uninstalling the extension therefore makes that
profile's existing vault unavailable; no automatic cross-profile recovery or escrow path exists.

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

## Privacy

FURSOY Vault operates locally and does not collect telemetry or transmit vault contents to the
project maintainer. See the full [privacy policy](PRIVACY.md).

## Code signing policy

Windows release binaries follow the [project code signing policy](CODE_SIGNING_POLICY.md). Public
releases are built, tested, signed, and published from version tags by GitHub Actions; see the
[release process](docs/RELEASING.md).
