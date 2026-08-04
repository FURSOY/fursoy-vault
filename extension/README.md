# FURSOY Extension — Phase 5 Wikipedia compatibility slice

MV3 extension for the single `tr.wikipedia.org` account group. It builds with `tsc` only. The group
uses exact local `trwiki*` and parent-domain `centralauth_*` authentication-cookie selectors; unrelated
Wikipedia preference, analytics, anti-abuse, and logout-marker cookies are excluded. Host permissions
remain portless because Chrome Cookies API permission matching is cookie-domain based.

The service worker connects to `com.fursoy.cookie_protector`, validates connection nonce and strict
message sequences, sends snapshots only to the native host, and accepts cookie plaintext only in a
`cookies.inject` response. Cookie values are never placed in `storage.session` or logs.

The service-worker console emits a value-free selector diagnostic before first enrollment. Enrollment
is refused until every required local and CentralAuth selector is present and the complete matched set
has remained unchanged for three seconds.

```text
npm install
npm run check
npm run build
```
