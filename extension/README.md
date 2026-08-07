# FURSOY Extension — Phase 7 monitoring slice

MV3 extension built only with `tsc`. It ships **no account-group config of its own**: the native
host owns the config and sends it in the Native Messaging v3 handshake (ADR-020). The extension
validates whatever it receives and caches it in `chrome.storage.local` only so it can still evict
fail-closed while the host is unreachable. An invalid config stops the extension.

A group is a registrable domain (`scope`). Every cookie under that domain and its subdomains is
vaulted — there are no per-site cookie selectors, no `required_for_enrollment` markers and no
site-specific health checks. Cookie removals are never interpreted, so nothing tries to tell a real
logout from a session rotation; a stale vault self-heals at the next capture.

The popup adds and removes protected sites. Adding one requests the matching host permission
through Chrome's own prompt (`optional_host_permissions`); refusing it aborts the addition.

Runtime state, alarms, navigation unlock contexts, enrollment and reconciliation locks
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
