# Windows release acceptance matrix

`run-windows.ps1` is the repeatable software gate. `Automated` means the row is backed by the
exact test IDs in `TEST_COVERAGE.md`; `Partial automated` names a narrower component claim and
does not imply an end-to-end browser/process failure test. Hardware/Chrome rows are recorded by
the release owner against a clean disposable profile.

| Scenario | Gate | Test IDs | Expected result |
|---|---|---|---|
| Protocol/capability handshake | Automated | `ACC-NMH-001` | v6 ACK echoes request ID and required capabilities |
| Protocol v7 activation and downgrade floor | Automated | `ACC-NMH-005` | v7 ACK persists floor 7; later v6 handshake cannot activate operation semantics |
| Unauthorized caller origin | Automated | `ACC-NMH-002` | Host exits before reading application messages |
| Old protocol | Automated | `ACC-NMH-003` | Host fails closed |
| >1 MiB cookie transfer | Automated (component contract) | `CHUNK-EXT-001`, `CHUNK-HOST-001`, `CHUNK-HOST-002` | Both production chunkers preserve order with bounded frames; host rejects declared aggregate count above its limit |
| Host crash during config/vault mutation | Partial automated (durability seams) | `CRASH-VAULT-001`, `FAIL-ATOMIC-001`, `FAIL-ATOMIC-002` | Staged-delete recovery and both sides of atomic replacement are deterministic; a killed-process cross-component test remains missing |
| Browser/service-worker restart | Partial automated (decision state) | `RESTART-DECISION-001` | A new connection requires a fresh ACK; real service-worker termination and reconnect reconciliation remain missing |
| Native transaction journal recovery | Automated (native component) | `TXN-VAULT-001`, `TXN-RECOVERY-001`, `TXN-RECOVERY-002`, `TXN-LEASE-001`, `TXN-GATE-001`, `TXN-KEY-001`, `V6-COMPAT-001` | Prepared vault bytes, ambiguous durability, journal/lease lag, startup gating, key failure and unchanged v6 handshake contract are deterministic |
| Windows Hello cancel | Hardware release gate | — | Lease remains sealed; no cookie injection |
| Windows Hello credential reset | Hardware release gate | — | Registry is quarantined and one controlled re-enrollment occurs |
| CHIPS round-trip | Chrome 130+ release gate | — | topLevelSite and hasCrossSiteAncestor round-trip exactly |
| Permission removal/regrant | Chrome release gate | — | Removed scope permission disappears; regrant repairs access |
| Two-profile isolation | Partial automated plus Chrome release gate | `ACC-NMH-004` | Disk config/recovery claims are isolated automatically; Chrome cookie-store isolation remains a UI gate |

For a public tag, attach the completed matrix to the release notes with Windows build, Chrome
version, extension ZIP hash, companion ZIP hash, and tester identity.
