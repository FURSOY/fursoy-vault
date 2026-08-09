FURSOY Vault — Companion App Setup
==================================

1. Double-click install.bat.
   Windows may show a security warning ("Unknown publisher" / "Windows protected your PC") —
   this is because the app doesn't have a code-signing certificate yet, not a sign of malware.
   Click "More info" / "Run anyway" to continue.

2. You'll see a confirmation message once setup finishes.

3. Install the FURSOY Vault extension from the Chrome Web Store (if you haven't already).

To disable: double-click uninstall.bat. Your protected sites and vault data stay under
%LOCALAPPDATA%\FursoyVault and are not lost on reinstall.

To permanently erase the companion, configuration, audit history and every encrypted vault, close
Chrome and run: powershell -File uninstall.ps1 -Purge

Audit export (close Chrome first):
  "%LOCALAPPDATA%\FursoyVault\native-host\versions\<current>\fursoy-vault-host.exe" --export-audit audit.jsonl

This release is GPL-3.0. See LICENSE and SOURCE.txt for license, source and reproducible build info.
