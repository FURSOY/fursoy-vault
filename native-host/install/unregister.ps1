param([switch]$Purge)
$ErrorActionPreference = "Stop"
# Must mirror the browser list in register.ps1 / install.ps1.
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
if ($Purge) {
  $dataRoot = Join-Path $env:LOCALAPPDATA "FursoyVault"
  if (Test-Path -LiteralPath $dataRoot) { Remove-Item -LiteralPath $dataRoot -Recurse -Force }
  Write-Host "Native Messaging registration and all local vault data were permanently removed."
} else {
  Write-Host "Native Messaging disabled; vault data was preserved. Run with -Purge for permanent removal."
}
