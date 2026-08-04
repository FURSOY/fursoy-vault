$ErrorActionPreference = "Stop"
$registryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.cookie_protector"
if (Test-Path -LiteralPath $registryPath) { Remove-Item -LiteralPath $registryPath -Recurse -Force }
Write-Host "Native Messaging registration removed; vault data was preserved."
