# Release process

Public releases are produced only by `.github/workflows/release.yml`; maintainers do not upload
locally built binaries. A pushed `vX.Y.Z` annotated tag runs all quality gates, calculates SHA-256
checksums, and publishes the GitHub Release using the annotated tag message as its release notes.
The Windows companion is currently unsigned. Every release description automatically warns that
Windows may show **Unknown publisher** and directs users to verify the published SHA-256 checksum.
The companion is packaged by Velopack: `FURSOY-Vault-Setup.exe` is the user-facing installer and
the remaining `releases.win.json`/versioned package assets are the automatic-update feed.

## Cut a release

1. Update every project version and release-facing document, then merge the tested commit to
   `main`.
2. Confirm the normal `release-quality` workflow passes on that commit.
3. Create an annotated tag. Its message becomes the GitHub Release description:

   ```text
   git tag -a v0.4.1 -m "FURSOY Vault v0.4.1" -m "Describe user-visible changes here."
   git push origin v0.4.1
   ```

4. Watch the `publish-release` workflow. GitHub publishes the release only after every automated
   quality gate, package build, and checksum step succeeds.

The installer alias must retain the exact name `FURSOY-Vault-Setup.exe`, because onboarding uses
GitHub's stable `releases/latest/download/FURSOY-Vault-Setup.exe` URL. Publish every file produced
under `native-host/target/velopack`; omitting the feed or full package breaks automatic updates.

## Rollout order

Publish the GitHub Release first so existing companions can download it. Submit the matching
extension package to the Chrome Web Store only after the companion release is available. Protocol
changes must retain the documented minimum-version checks and, where possible, one previous host
version of overlap. A failed update check never changes vault, lease, journal or browser state.
