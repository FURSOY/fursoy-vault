# FURSOY Vault companion app installer. This release script copies a prebuilt executable; it does
# not require Rust or Cargo on the user's machine.
$ErrorActionPreference = "Stop"
$scriptRoot = $PSScriptRoot

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
  throw "fursoy-vault-host.exe not found next to install.ps1; re-download the complete release."
}
$versionFile = Join-Path $scriptRoot "VERSION"
if (-not (Test-Path -LiteralPath $versionFile)) { throw "VERSION metadata is missing from the release." }
$version = (Get-Content -LiteralPath $versionFile -Raw).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "VERSION metadata is malformed." }

$installRoot = Join-Path $dataRoot "native-host"
$deployment = "$version-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
$deploymentRoot = Join-Path $installRoot "versions\$deployment"
New-Item -ItemType Directory -Force -Path $deploymentRoot | Out-Null
$installedExe = Join-Path $deploymentRoot "fursoy-vault-host.exe"
Copy-Item -LiteralPath $sourceExe -Destination $installedExe

$configRoot = Join-Path $dataRoot "config"
New-Item -ItemType Directory -Force -Path $configRoot | Out-Null
$installedConfig = Join-Path $configRoot "account-groups.json"
if (-not (Test-Path -LiteralPath $installedConfig)) {
  $emptyConfig = '{"version":3,"compatibility_version":3,"groups":[]}'
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
$manifestTemp = "$manifestPath.new"
$manifestBackup = "$manifestPath.rollback"
[System.IO.File]::WriteAllText($manifestTemp, $manifestJson, [System.Text.UTF8Encoding]::new($false))
$registryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.vault"
try {
  if (Test-Path -LiteralPath $manifestPath) { Copy-Item -LiteralPath $manifestPath -Destination $manifestBackup -Force }
  Move-Item -LiteralPath $manifestTemp -Destination $manifestPath -Force
  New-Item -Path $registryPath -Force | Out-Null
  Set-Item -Path $registryPath -Value $manifestPath
  if (Test-Path -LiteralPath $manifestBackup) { Remove-Item -LiteralPath $manifestBackup -Force }
} catch {
  if (Test-Path -LiteralPath $manifestBackup) { Move-Item -LiteralPath $manifestBackup -Destination $manifestPath -Force }
  elseif (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath -Force }
  if (Test-Path -LiteralPath $deploymentRoot) { Remove-Item -LiteralPath $deploymentRoot -Recurse -Force }
  throw
}

Write-Host ""
Write-Host "FURSOY Vault companion app installed."
Write-Host "Install the FURSOY Vault extension from the Chrome Web Store to continue."
