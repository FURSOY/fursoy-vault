# Disposable Profile Session Probe

Experiment 3 tests whether one synthetic, server-backed session survives repeated cookie eviction
and restoration in a disposable Chrome profile. The local app stores session IDs only in an
in-memory map. It uses the fixed dummy credentials `probe-user` / `probe-password`; these are not
real credentials or secrets.

The harness logs in exactly once, then runs N cookie snapshot → eviction → logged-out check →
restore → authenticated check cycles on that same session. After the cycles it calls the real
logout endpoint, restores the stale cookie once, and verifies that server-side invalidation prevents
the old session from returning.

Immediately after the first-party login, the harness captures a raw extended diagnostic: completely
unfiltered `chrome.cookies.getAll({})` metadata, every cookie store and its tab IDs, the test tab's
URL/window/incognito metadata, and content-page `document.cookie` names. The copied report places
this data in a separate section without assigning a root cause.

The same diagnostic now compares the immediate unfiltered cookie view with a second
`chrome.cookies.getAll({})` read after 250 ms. The localhost server also retains the ten most
recent `/api/login` and `/api/protected` request observations and reports only whether a Cookie
header was present and which cookie names it contained. Cookie values are not retained or reported.

Before the existing `localhost` session run, the harness performs the same first-party login,
protected request, immediate `getAll({})`, and 250 ms delayed `getAll({})` diagnostic at
`http://127.0.0.1:43118`. Both origins remain in the copied report as separate raw sections. The
harness does not automatically accept or reject the localhost-special-case hypothesis.

Both first-party diagnostics also write `FCP-docwrite-diagnostic=1; path=/` from the content page,
confirm its name through `document.cookie`, and then look for that fixed name in the immediate and
250 ms Cookies API snapshots. The content script deletes this non-HttpOnly diagnostic cookie before
the session cycle begins. The report also includes the server's login `Set-Cookie` template with the
session value replaced by `<redacted>`.

Cookie values are never placed in the UI table or copied text report. The fixed synthetic cookie
name may appear in the temporary metadata diagnostic so store and partition behavior can be traced.

## Build

```text
npm install
npm run build
```

The extension is compiled with plain `tsc`; no bundler is used. Load `poc/session-probe/` itself as
the unpacked extension because the manifest/static harness stay at the root and JavaScript is emitted
to `dist/`.

## Manual disposable-profile run

1. Start the local app with `npm run serve`; it listens only on IPv6 and IPv4 loopback and serves
   both `http://localhost:43118` and `http://127.0.0.1:43118`.
2. Start Chrome with a new disposable profile directory; do not use a real-account profile.
3. Open `chrome://extensions`, enable Developer mode, and load `poc/session-probe/` unpacked.
4. Click the extension toolbar action to open the full-page harness.
5. Leave **Cycles** at `10`, then click **Run session probe**.
6. Click **Copy report as text** and preserve the complete report for evaluation.
7. Close Chrome and remove only the disposable profile directory you created for this experiment.

The test server is intentionally in-memory. Restarting it invalidates every test session. This probe
does not establish compatibility with real sites, server-side rotation, device binding,
`localStorage`, or `IndexedDB`.

The harness also calls a probe-only reset endpoint before and after the run so an interrupted test
does not leave a dummy in-memory session behind. The separate logout invalidation control still uses
the normal logout endpoint and verifies that restoring its stale cookie cannot revive the session.
