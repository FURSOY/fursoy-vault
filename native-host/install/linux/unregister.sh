#!/usr/bin/env bash
# Removes the Native Messaging registration. Vault data is preserved unless --purge is given.
#
# The default is deliberately reversible, matching the Windows uninstaller: removing the browser
# registration stops the host from being reachable, which is what someone uninstalling usually
# wants, while a vault they may still need is not destroyed by a command they ran to tidy up.
set -euo pipefail

HOST_NAME="com.fursoy.vault"
PURGE=0

for argument in "$@"; do
  case "$argument" in
    --purge) PURGE=1 ;;
    *) echo "usage: $0 [--purge]" >&2; exit 2 ;;
  esac
done

CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

# Must mirror the list in register.sh: a manifest left behind points at a host binary that is no
# longer there, and the browser reports a confusing failure rather than a clean absence.
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

removed=0
for browser_dir in "${BROWSER_DIRS[@]}"; do
  target="$browser_dir/NativeMessagingHosts/$HOST_NAME.json"
  if [ -f "$target" ]; then
    rm -f "$target"
    removed=$((removed + 1))
  fi
done

if [ "$PURGE" -eq 1 ]; then
  # The TPM-held keys are not deleted here, and cannot be: the KEK's private half lives in the TPM
  # and this removes the wrapped blobs that address it. Without them the key is unreachable and
  # the vault is unrecoverable, which is what a purge is for.
  rm -rf "$DATA_HOME/fursoy-vault"
  echo "Removed $removed browser registrations and all local vault data."
else
  echo "Removed $removed browser registrations; vault data was preserved."
  echo "Run with --purge to delete it permanently."
fi
