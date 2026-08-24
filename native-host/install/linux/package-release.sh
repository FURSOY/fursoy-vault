#!/usr/bin/env bash
# Builds the Linux companion and packs it with everything needed to install it by hand.
#
# A tarball rather than a .deb or .rpm: the companion is a single binary plus two scripts, it
# installs per-user and touches nothing system-wide, and distribution packages would mean building
# and testing one per family for no gain that a user would notice. Distribution packages are worth
# revisiting when someone is asking for them, not before.
#
# The archive is reproducible for a given source tree: entries are sorted, ownership and timestamps
# are pinned, so two builds of the same tag produce identical bytes and the published checksum means
# something.
set -euo pipefail

cd "$(dirname "$0")/../.."
VERSION=$(cargo metadata --manifest-path Cargo.toml --locked --no-deps --format-version 1 \
  | grep -o '"version":"[^"]*"' | head -1 | cut -d'"' -f4)
[ -n "$VERSION" ] || { echo "could not read the crate version" >&2; exit 1; }

NAME="fursoy-vault-linux-x86_64"
STAGE="target/linux-package/$NAME"
OUT="target/linux-package/$NAME.tar.gz"

echo "Building companion $VERSION"
# --locked so the published binary is built from the committed dependency versions, exactly as the
# Windows job does.
cargo build --release --locked

rm -rf "target/linux-package"
mkdir -p "$STAGE"
cp target/release/fursoy-vault-host "$STAGE/"
cp install/linux/register.sh install/linux/unregister.sh "$STAGE/"
cp ../LICENSE ../SOURCE.txt "$STAGE/"
chmod 755 "$STAGE/fursoy-vault-host" "$STAGE/register.sh" "$STAGE/unregister.sh"
# Set explicitly rather than inherited: a build on a filesystem that does not carry Unix modes
# would otherwise ship world-writable files to everyone who extracts the archive.
chmod 644 "$STAGE/LICENSE" "$STAGE/SOURCE.txt"
printf '%s\n' "$VERSION" > "$STAGE/VERSION"

cat > "$STAGE/README.txt" <<EOF
FURSOY Vault companion for Linux — $VERSION

Install:
  ./register.sh "\$PWD/fursoy-vault-host"

  Registers the companion with every Chromium browser on this machine. Keep this directory where
  it is: the registration points at the binary in place rather than copying it. Then install the
  FURSOY Vault extension from the Chrome Web Store and restart your browser.

Requirements:
  A TPM 2.0 security chip, and membership of the 'tss' group so the TPM device is reachable:
    sudo usermod -aG tss "\$USER"      (log out and back in afterwards)

  You choose a PIN the first time a vault opens. The TPM holds it and rate-limits wrong guesses,
  which is what keeps a short PIN safe — but it also means enough wrong attempts lock the chip for
  a while, and no amount of restarting clears that. Waiting does.

Remove:
  ./unregister.sh              removes the browser registration, keeps your vault
  ./unregister.sh --purge      also deletes every vault permanently

Updates are not automatic. This release is GPL-3.0; see LICENSE and SOURCE.txt.
EOF

# Pinned metadata is what makes the archive reproducible: without it the tar carries this machine's
# user, group and mtimes, and the same source would hash differently on every build.
tar --create --gzip --file "$OUT" \
  --directory "target/linux-package" \
  --owner=0 --group=0 --numeric-owner \
  --mtime="@${SOURCE_DATE_EPOCH:-0}" \
  --sort=name \
  "$NAME"

sha256sum "$OUT" | sed "s|$(dirname "$OUT")/||" > "$OUT.sha256"

echo
echo "Wrote $OUT"
cat "$OUT.sha256"
