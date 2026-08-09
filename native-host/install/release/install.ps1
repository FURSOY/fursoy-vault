# FURSOY Vault companion app installer.
# Ships as a release asset alongside fursoy-vault-host.exe (in the same folder as this script) —
# unlike register.ps1, this does NOT build from source and does not require Rust/cargo. It only
# copies the already-built exe into place and registers it with Chrome.
$ErrorActionPreference = "Stop"
$scriptRoot = $PSScriptRoot

# Migrate an old FursoyCookieProtector-era install if one is still present.
$legacyDataRoot = Join-Path $env:LOCALAPPDATA "FursoyCookieProtector"
$dataRoot = Join-Path $env:LOCALAPPDATA "FursoyVault"
if ((Test-Path -LiteralPath $legacyDataRoot) -and (-not (Test-Path -LiteralPath $dataRoot))) {
  Move-Item -LiteralPath $legacyDataRoot -Destination $dataRoot
  Write-Host "Migrated data directory from FursoyCookieProtector to FursoyVault"
}
$legacyRegistryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.cookie_protector"
if (Test-Path -LiteralPath $legacyRegistryPath) {
  Remove-Item -LiteralPath $legacyRegistryPath -Recurse -Force
  Write-Host "Removed legacy com.fursoy.cookie_protector native messaging registration"
}

$sourceExe = Join-Path $scriptRoot "fursoy-vault-host.exe"
if (-not (Test-Path -LiteralPath $sourceExe)) {
  throw "fursoy-vault-host.exe not found next to install.ps1 — re-download the release, don't run this script on its own."
}

$installRoot = Join-Path $dataRoot "native-host"
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
$installedExe = Join-Path $installRoot "fursoy-vault-host.exe"
Copy-Item -LiteralPath $sourceExe -Destination $installedExe -Force

$configRoot = Join-Path $dataRoot "config"
New-Item -ItemType Directory -Force -Path $configRoot | Out-Null
$installedConfig = Join-Path $configRoot "account-groups.json"
# User data once sites can be added at runtime — never overwrite an existing one.
if (-not (Test-Path -LiteralPath $installedConfig)) {
  $emptyConfig = '{"version":2,"compatibility_version":2,"groups":[]}'
  [System.IO.File]::WriteAllText($installedConfig, $emptyConfig, [System.Text.UTF8Encoding]::new($false))
  Write-Host "Seeded empty account-group config at $installedConfig"
} else {
  Write-Host "Kept existing account-group config at $installedConfig"
}

$manifestPath = Join-Path $installRoot "com.fursoy.vault.json"
$manifest = [ordered]@{
  name = "com.fursoy.vault"
  description = "FURSOY Vault native host"
  path = $installedExe
  type = "stdio"
  allowed_origins = @("chrome-extension://ikodegbaomnahbjiokfogpedaoifhbde/")
}
$manifestJson = $manifest | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, [System.Text.UTF8Encoding]::new($false))
$registryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.vault"
New-Item -Path $registryPath -Force | Out-Null
Set-Item -Path $registryPath -Value $manifestPath

Write-Host ""
Write-Host "FURSOY Vault companion app installed."
Write-Host "Install the FURSOY Vault extension from the Chrome Web Store to continue."
