param([switch]$Purge)
# Default is a reversible disable. -Purge is explicit, permanent removal.
$ErrorActionPreference = "Stop"
# Must mirror the browser list in install.ps1: anything left behind is a registry key pointing at
# a host executable that is no longer there.
$registryPaths = @(
  "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Vivaldi\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Opera Software\Opera Stable\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Chromium\NativeMessagingHosts\com.fursoy.vault"
)
foreach ($registryPath in $registryPaths) {
  if (Test-Path -LiteralPath $registryPath) { Remove-Item -LiteralPath $registryPath -Recurse -Force }
}
$updaterPathFile = Join-Path $env:LOCALAPPDATA "FursoyVault\native-host\updater-path.txt"
if (Test-Path -LiteralPath $updaterPathFile) { Remove-Item -LiteralPath $updaterPathFile -Force }
if ($Purge) {
  $dataRoot = Join-Path $env:LOCALAPPDATA "FursoyVault"
  if (Test-Path -LiteralPath $dataRoot) { Remove-Item -LiteralPath $dataRoot -Recurse -Force }
  Write-Host "FURSOY Vault and all local vault data were permanently removed."
} else {
  Write-Host "FURSOY Vault was disabled; vault data was preserved. Run uninstall.ps1 -Purge for permanent removal."
}
