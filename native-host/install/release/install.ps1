param([string]$UpdaterPath = "")
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
$updaterPathFile = Join-Path $installRoot "updater-path.txt"
$updaterPathTemp = "$updaterPathFile.new"
$updaterPathBackup = "$updaterPathFile.rollback"
if ($UpdaterPath -ne "") {
  if (-not [System.IO.Path]::IsPathFullyQualified($UpdaterPath) -or -not (Test-Path -LiteralPath $UpdaterPath -PathType Leaf)) {
    throw "UpdaterPath must identify the installed Velopack executable."
  }
}
[System.IO.File]::WriteAllText($manifestTemp, $manifestJson, [System.Text.UTF8Encoding]::new($false))
$registryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.vault"
$registryExisted = Test-Path -LiteralPath $registryPath
$previousRegistryValue = if ($registryExisted) { (Get-Item -LiteralPath $registryPath).GetValue("") } else { $null }
try {
  if (Test-Path -LiteralPath $manifestPath) { Copy-Item -LiteralPath $manifestPath -Destination $manifestBackup -Force }
  Move-Item -LiteralPath $manifestTemp -Destination $manifestPath -Force
  New-Item -Path $registryPath -Force | Out-Null
  Set-Item -Path $registryPath -Value $manifestPath
  if ($UpdaterPath -ne "") {
    if (Test-Path -LiteralPath $updaterPathFile) { Copy-Item -LiteralPath $updaterPathFile -Destination $updaterPathBackup -Force }
    [System.IO.File]::WriteAllText($updaterPathTemp, "$UpdaterPath`n", [System.Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $updaterPathTemp -Destination $updaterPathFile -Force
  }
  if (Test-Path -LiteralPath $manifestBackup) { Remove-Item -LiteralPath $manifestBackup -Force }
  if (Test-Path -LiteralPath $updaterPathBackup) { Remove-Item -LiteralPath $updaterPathBackup -Force }
} catch {
  if ($registryExisted) {
    New-Item -Path $registryPath -Force | Out-Null
    Set-Item -Path $registryPath -Value $previousRegistryValue
  } elseif (Test-Path -LiteralPath $registryPath) {
    Remove-Item -LiteralPath $registryPath -Recurse -Force
  }
  if (Test-Path -LiteralPath $manifestBackup) { Move-Item -LiteralPath $manifestBackup -Destination $manifestPath -Force }
  elseif (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath -Force }
  if (Test-Path -LiteralPath $updaterPathBackup) { Move-Item -LiteralPath $updaterPathBackup -Destination $updaterPathFile -Force }
  elseif (($UpdaterPath -ne "") -and (Test-Path -LiteralPath $updaterPathFile)) { Remove-Item -LiteralPath $updaterPathFile -Force }
  if (Test-Path -LiteralPath $updaterPathTemp) { Remove-Item -LiteralPath $updaterPathTemp -Force }
  if (Test-Path -LiteralPath $deploymentRoot) { Remove-Item -LiteralPath $deploymentRoot -Recurse -Force }
  throw
}

Write-Host ""
Write-Host "FURSOY Vault companion app installed."
Write-Host "Install the FURSOY Vault extension from the Chrome Web Store to continue."
