#!/usr/bin/env bash
# Registers the FURSOY Vault Native Messaging host with every Chromium browser on this machine.
#
# The Windows counterpart writes a registry value per browser; here each browser reads a JSON
# manifest from its own directory under the user's config, so the same manifest is written to all
# of them. Writing one for a browser that is not installed is inert, and it means a browser
# installed later works without re-running this.
#
# Per-user, never system-wide: the vault lives in the user's data directory and there is nothing
# here that another account should be able to reach.
set -euo pipefail

HOST_NAME="com.fursoy.vault"
EXTENSION_ID="ibjddphkjppgkdbegjibddbjkagdlaea"

usage() {
  echo "usage: $0 /path/to/fursoy-vault-host" >&2
  exit 2
}

[ $# -eq 1 ] || usage
HOST_BINARY=$(readlink -f -- "$1") || usage
if [ ! -x "$HOST_BINARY" ]; then
  echo "error: $HOST_BINARY is not an executable file" >&2
  exit 1
fi

CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"

# Chromium forks each keep their own manifest directory. Chrome Beta/Dev/Canary and the
# distribution-packaged Chromium builds are included because a developer or an early adopter is
# exactly the sort of user likely to be running one.
BROWSER_DIRS=(
  "$CONFIG_HOME/google-chrome"
  "$CONFIG_HOME/google-chrome-beta"
  "$CONFIG_HOME/google-chrome-unstable"
  "$CONFIG_HOME/chromium"
  "$CONFIG_HOME/microsoft-edge"
  "$CONFIG_HOME/BraveSoftware/Brave-Browser"
  "$CONFIG_HOME/vivaldi"
  "$CONFIG_HOME/opera"
)

manifest() {
  cat <<JSON
{
  "name": "$HOST_NAME",
  "description": "FURSOY Vault native host",
  "path": "$HOST_BINARY",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://$EXTENSION_ID/"
  ]
}
JSON
}

written=0
for browser_dir in "${BROWSER_DIRS[@]}"; do
  target_dir="$browser_dir/NativeMessagingHosts"
  mkdir -p "$target_dir"
  target="$target_dir/$HOST_NAME.json"
  # Written to a temporary file in the same directory and renamed, so a browser reading the
  # manifest concurrently sees either the old one or the new one, never a half-written file.
  temporary="$target.new"
  manifest > "$temporary"
  chmod 600 "$temporary"
  mv -f "$temporary" "$target"
  written=$((written + 1))
done

echo "Registered $HOST_NAME for $written Chromium browsers."
echo "Host binary: $HOST_BINARY"
echo
echo "Install the FURSOY Vault extension from the Chrome Web Store to continue."
