# FURSOY Extension — Phase 7 monitoring slice

MV3 extension built only with `tsc`. Account groups are loaded from `account-groups.json`; the same
bytes are bundled by the native host and their SHA-256 digest is bound to the Native Messaging v3
handshake. A mismatch fails closed before any lease is granted.

The initial Phase 6 configuration contains two isolated groups:

- Wikipedia (`balanced`): exact `trwiki*` and `centralauth_*` authentication selectors.
- Controlled Session App (`critical`): `FCP-mvp-session` on `http://localhost:43119`.

Runtime state, alarms, navigation unlock contexts, enrollment, invalidation and reconciliation locks
are keyed by account-group UUID. Chrome cookie mutations remain globally serialized so two groups
cannot race the browser store. Cookie values are never written to extension storage or logs.

Policy idle thresholds are 1 minute (`critical`), 5 minutes (`balanced`) and 15 minutes
(`convenient`). The extension uses the one-minute Chrome idle event as a base and schedules a separate
alarm for each longer group threshold. `monitor` groups do not request leases or mutate cookies.

Monitoring events use a 128-entry bounded `chrome.storage.local` outbox and are acknowledged by
event ID after native-host audit. High/medium signals use fixed, redacted Chrome notifications with
a ten-minute signal+group rate limit; info signals remain audit-only. This is user-mode best-effort
detection and does not claim to block process or profile access.

```text
npm install
npm run check
npm run build
npm test
```
