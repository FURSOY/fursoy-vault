# Code signing policy

## Current status

The Windows companion is currently distributed without an Authenticode signature. Windows may
therefore display **Unknown publisher** during installation. This status is disclosed on the
download page and in every GitHub Release. The Chrome extension package is also published with a
SHA-256 checksum.

## Privacy

This program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it. FURSOY Vault does not collect
telemetry or transfer vault contents, cookie values, browsing history, Windows Hello data, or
monitoring results to the project maintainer. See the full [privacy policy](PRIVACY.md).

## Signing and release process

The Windows companion is built from a version tag by a GitHub-hosted Windows runner. The complete
test and acceptance suite must pass before GitHub publishes the unsigned release and its SHA-256
checksums. Maintainers do not replace release binaries with locally built files.

The matching Git tag is the complete corresponding GPL-3.0-only source for every published binary.
Users should download only from the project's GitHub Releases page and compare each ZIP against the
adjacent `.sha256` file. If trusted code signing is introduced later, this policy and the release
workflow will be updated before the first signed release.
