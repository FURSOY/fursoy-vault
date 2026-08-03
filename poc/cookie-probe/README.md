# Cookie Attribute Probe

This MV3 extension tests `chrome.cookies` attribute round-trip behavior using only synthetic probe
cookies on the fixed local target `http://localhost:43117`. It never reads, overwrites, or removes a
real account/session cookie.

Matching attributes do **not** prove that an actual session survives eviction and restoration. The
probe measures only Chrome API round-trip compatibility.

## Build

```text
npm install
npm run build
```

The build is plain `tsc`; there is no bundler. Load `poc/cookie-probe/` itself as the unpacked
extension because `manifest.json` and the static harness files stay at the project root while
compiled JavaScript is emitted to `dist/`.

## Run

1. Start the local test site with `npm run serve`.
2. Open `chrome://extensions`, enable Developer mode, and choose **Load unpacked**.
3. Select the `poc/cookie-probe/` directory.
4. Click the extension toolbar action. It opens the harness as a full browser tab.
5. Click **Run probe suite**.
6. Click **Copy report as text** and paste the complete report for evaluation.

The manifest contains a pinned public key, so this unpacked extension keeps the same extension ID
when it is reloaded from this directory. The generated private key was intentionally not persisted:
the probe needs only the public SPKI value for deterministic unpacked-extension identity.
