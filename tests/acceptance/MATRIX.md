# Windows release acceptance matrix

`run-windows.ps1` is the repeatable software gate. The final six rows require a real Windows
Hello/TPM and Chrome UI, so the release owner records them against a clean disposable profile.

| Scenario | Gate | Expected result |
|---|---|---|
| Protocol/capability handshake | Automated | v6 ACK echoes request ID and required capabilities |
| Unauthorized caller origin | Automated | Host exits before reading application messages |
| Old protocol | Automated | Host fails closed |
| >1 MiB cookie transfer | Rust/TS automated tests | Ordered bounded chunks; aggregate limits enforced |
| Host crash during config/vault mutation | Rust automated tests | Tombstone is restored or committed from authoritative config |
| Browser/service-worker restart | State-machine automated tests | Durable metadata is reconciled from host state |
| Windows Hello cancel | Hardware release gate | Lease remains sealed; no cookie injection |
| Windows Hello credential reset | Hardware release gate | Registry is quarantined and one controlled re-enrollment occurs |
| CHIPS round-trip | Chrome 130+ release gate | topLevelSite and hasCrossSiteAncestor round-trip exactly |
| Permission removal/regrant | Chrome release gate | Removed scope permission disappears; regrant repairs access |
| Two-profile isolation | Chrome release gate | Two Chrome profiles can protect the same domain without groups, vault records, leases or restored cookies crossing profiles |
| Profile recovery transfer | Automated + Chrome release gate | An empty profile can claim an inactive previous vault; ownership rotates to a fresh ID and the old namespace is no longer reusable |

For a public tag, attach the completed matrix to the release notes with Windows build, Chrome
version, extension ZIP hash, companion ZIP hash, and tester identity.
