param([switch]$Purge)
$ErrorActionPreference = "Stop"
$registryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.vault"
if (Test-Path -LiteralPath $registryPath) { Remove-Item -LiteralPath $registryPath -Recurse -Force }
if ($Purge) {
  $dataRoot = Join-Path $env:LOCALAPPDATA "FursoyVault"
  if (Test-Path -LiteralPath $dataRoot) { Remove-Item -LiteralPath $dataRoot -Recurse -Force }
  Write-Host "Native Messaging registration and all local vault data were permanently removed."
} else {
  Write-Host "Native Messaging disabled; vault data was preserved. Run with -Purge for permanent removal."
}
