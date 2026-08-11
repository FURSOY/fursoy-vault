# Release process

Public releases are produced only by `.github/workflows/release.yml`; maintainers do not upload
locally built binaries. A pushed `vX.Y.Z` annotated tag runs all quality gates, calculates SHA-256
checksums, and publishes the GitHub Release using the annotated tag message as its release notes.
For signed releases, the workflow submits the Windows companion to SignPath and verifies the
returned Authenticode signature before publication.

Version `v0.4.1` is the sole unsigned bootstrap exception. SignPath Foundation requires a project
to be publicly released in the form it intends to sign before applying, so this tag may publish the
GitHub Actions-built companion without SignPath configuration. The release notes identify it as
unsigned and warn that Windows may show **Unknown publisher**. No other tag can use this exception.

## One-time SignPath setup

1. Apply to SignPath Foundation and install the SignPath GitHub App for this repository.
2. Configure a SignPath artifact whose root is the uploaded ZIP and whose signing rule signs
   `fursoy-vault-host.exe` inside that ZIP.
3. Add the repository secret `SIGNPATH_API_TOKEN`.
4. Add these repository variables:
   - `SIGNPATH_ORGANIZATION_ID`
   - `SIGNPATH_PROJECT_SLUG`
   - `SIGNPATH_SIGNING_POLICY_SLUG`

Except for the explicit `v0.4.1` bootstrap path above, the workflow fails before building or
publishing when any required SignPath setting is absent. It never silently falls back to an
unsigned public release.

## Cut a release

1. Update every project version and release-facing document, then merge the tested commit to
   `main`.
2. Confirm the normal `release-quality` workflow passes on that commit.
3. Create an annotated tag. Its message becomes the GitHub Release description:

   ```text
   git tag -a v0.4.1 -m "FURSOY Vault v0.4.1" -m "Describe user-visible changes here."
   git push origin v0.4.1
   ```

4. Approve the signing request in SignPath. GitHub publishes the release only after signing and
   local signature verification succeed.

The companion asset must retain the exact name `fursoy-vault-windows.zip`, because onboarding uses
GitHub's stable `releases/latest/download/fursoy-vault-windows.zip` URL.
