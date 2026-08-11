# Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

## Roles

- Committers and reviewers: [FURSOY](https://github.com/FURSOY)
- Approvers: [FURSOY](https://github.com/FURSOY)

## Privacy

FURSOY Vault does not collect telemetry or transfer vault contents, cookie values, browsing
history, Windows Hello data, or monitoring results to any networked system. Network access occurs
only when the user explicitly opens a download or source link. Chrome and GitHub apply their own
privacy policies when their services are used.

## Signing and release process

The Windows companion is built from a version tag by a GitHub-hosted Windows runner. The complete
test and acceptance suite must pass before the unsigned release artifact is submitted to SignPath.
Every signing request requires manual approval. The workflow verifies the returned executable's
Authenticode signature before GitHub publishes the release and its SHA-256 checksums.

Only binaries built from this repository are submitted under this project. The matching Git tag is
the complete corresponding GPL-3.0-only source for every published binary.
