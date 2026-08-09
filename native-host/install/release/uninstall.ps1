param([switch]$Purge)
# Default is a reversible disable. -Purge is explicit, permanent removal.
$ErrorActionPreference = "Stop"
$registryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.vault"
if (Test-Path -LiteralPath $registryPath) { Remove-Item -LiteralPath $registryPath -Recurse -Force }
if ($Purge) {
  $dataRoot = Join-Path $env:LOCALAPPDATA "FursoyVault"
  if (Test-Path -LiteralPath $dataRoot) { Remove-Item -LiteralPath $dataRoot -Recurse -Force }
  Write-Host "FURSOY Vault and all local vault data were permanently removed."
} else {
  Write-Host "FURSOY Vault was disabled; vault data was preserved. Run uninstall.ps1 -Purge for permanent removal."
}
