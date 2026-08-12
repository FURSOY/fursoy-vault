# Release process

Public releases are produced only by `.github/workflows/release.yml`; maintainers do not upload
locally built binaries. A pushed `vX.Y.Z` annotated tag runs all quality gates, calculates SHA-256
checksums, and publishes the GitHub Release using the annotated tag message as its release notes.
The Windows companion is currently unsigned. Every release description automatically warns that
Windows may show **Unknown publisher** and directs users to verify the published SHA-256 checksum.

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

The companion asset must retain the exact name `fursoy-vault-windows.zip`, because onboarding uses
GitHub's stable `releases/latest/download/fursoy-vault-windows.zip` URL.
