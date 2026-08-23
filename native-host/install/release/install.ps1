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
  allowed_origins = @("chrome-extension://ibjddphkjppgkdbegjibddbjkagdlaea/")
}
$manifestJson = $manifest | ConvertTo-Json -Depth 4
$manifestTemp = "$manifestPath.new"
$manifestBackup = "$manifestPath.rollback"
$updaterPathFile = Join-Path $installRoot "updater-path.txt"
$updaterPathTemp = "$updaterPathFile.new"
$updaterPathBackup = "$updaterPathFile.rollback"
if ($UpdaterPath -ne "") {
  try {
    $UpdaterPath = (Resolve-Path -LiteralPath $UpdaterPath -ErrorAction Stop).Path
  } catch {
    throw "UpdaterPath must identify the installed Velopack executable."
  }
  $localAppDataRoot = [System.IO.Path]::GetFullPath($env:LOCALAPPDATA).TrimEnd('\') + '\'
  if (-not $UpdaterPath.StartsWith($localAppDataRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
      -not (Test-Path -LiteralPath $UpdaterPath -PathType Leaf)) {
    throw "UpdaterPath must identify the installed Velopack executable."
  }
}
[System.IO.File]::WriteAllText($manifestTemp, $manifestJson, [System.Text.UTF8Encoding]::new($false))
# Every Chromium browser reads Native Messaging manifests from its own HKCU key, and that is the
# only thing that differs between them: the extension ships a fixed id (see "key" in the
# manifest), so one Chrome Web Store install carries the same id into Edge, Brave and the rest,
# and the host's origin check accepts it unchanged. Registering a browser that is not installed
# writes an inert key nobody reads, and means a browser installed later works without re-running
# Setup — cheaper and more reliable than detecting what is on the machine.
$registryPaths = @(
  "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Vivaldi\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Opera Software\Opera Stable\NativeMessagingHosts\com.fursoy.vault"
  "HKCU:\Software\Chromium\NativeMessagingHosts\com.fursoy.vault"
)
$previousRegistryState = @()
foreach ($path in $registryPaths) {
  $existed = Test-Path -LiteralPath $path
  $value = $null
  if ($existed) { $value = (Get-Item -LiteralPath $path).GetValue("") }
  $previousRegistryState += [pscustomobject]@{ Path = $path; Existed = $existed; Value = $value }
}
try {
  if (Test-Path -LiteralPath $manifestPath) { Copy-Item -LiteralPath $manifestPath -Destination $manifestBackup -Force }
  Move-Item -LiteralPath $manifestTemp -Destination $manifestPath -Force
  foreach ($path in $registryPaths) {
    New-Item -Path $path -Force | Out-Null
    Set-Item -Path $path -Value $manifestPath
  }
  if ($UpdaterPath -ne "") {
    if (Test-Path -LiteralPath $updaterPathFile) { Copy-Item -LiteralPath $updaterPathFile -Destination $updaterPathBackup -Force }
    [System.IO.File]::WriteAllText($updaterPathTemp, "$UpdaterPath`n", [System.Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $updaterPathTemp -Destination $updaterPathFile -Force
  }
  if (Test-Path -LiteralPath $manifestBackup) { Remove-Item -LiteralPath $manifestBackup -Force }
  if (Test-Path -LiteralPath $updaterPathBackup) { Remove-Item -LiteralPath $updaterPathBackup -Force }
} catch {
  foreach ($state in $previousRegistryState) {
    if ($state.Existed) {
      New-Item -Path $state.Path -Force | Out-Null
      Set-Item -Path $state.Path -Value $state.Value
    } elseif (Test-Path -LiteralPath $state.Path) {
      Remove-Item -LiteralPath $state.Path -Recurse -Force
    }
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
