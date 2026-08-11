# Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

## Roles

- Committers and reviewers: [FURSOY](https://github.com/FURSOY)
- Approvers: [FURSOY](https://github.com/FURSOY)

## Privacy

This program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it. FURSOY Vault does not collect
telemetry or transfer vault contents, cookie values, browsing history, Windows Hello data, or
monitoring results to the project maintainer. See the full [privacy policy](PRIVACY.md).

## Signing and release process

The Windows companion is built from a version tag by a GitHub-hosted Windows runner. The complete
test and acceptance suite must pass before the unsigned release artifact is submitted to SignPath.
Every signing request requires manual approval. The workflow verifies the returned executable's
Authenticode signature before GitHub publishes the release and its SHA-256 checksums.

Only binaries built from this repository are submitted under this project. The matching Git tag is
the complete corresponding GPL-3.0-only source for every published binary.
