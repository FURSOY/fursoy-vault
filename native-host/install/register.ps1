param([ValidateSet("Debug", "Release")][string]$Configuration = "Release")
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path

# Renamed from FursoyCookieProtector/com.fursoy.cookie_protector. Move any existing installation's
# data directory before anything below creates a fresh one under the new name, so an upgrade keeps
# the vault, Hello credential, leases and audit chain instead of silently starting empty.
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

$profile = if ($Configuration -eq "Release") { "release" } else { "debug" }
& cargo build --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml") $(if ($Configuration -eq "Release") { "--release" })
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$sourceExe = Join-Path $repoRoot "native-host\target\$profile\fursoy-vault-host.exe"
$installRoot = Join-Path $dataRoot "native-host"
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
$version = (& cargo metadata --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml") --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
$deployment = "$version-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
$deploymentRoot = Join-Path $installRoot "versions\$deployment"
New-Item -ItemType Directory -Force -Path $deploymentRoot | Out-Null
$installedExe = Join-Path $deploymentRoot "fursoy-vault-host.exe"
Copy-Item -LiteralPath $sourceExe -Destination $installedExe
$configRoot = Join-Path $dataRoot "config"
New-Item -ItemType Directory -Force -Path $configRoot | Out-Null
$installedConfig = Join-Path $configRoot "account-groups.json"
# The installed config is user data once sites can be added at runtime (ADR-020 slice 2), so it
# is only seeded on first install. Overwriting it here would silently drop protected sites.
if (-not (Test-Path $installedConfig)) {
  Copy-Item -LiteralPath (Join-Path $repoRoot "config\account-groups.json") -Destination $installedConfig
  Write-Host "Seeded account-group config at $installedConfig"
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
Write-Host "Registered com.fursoy.vault for extension ikodegbaomnahbjiokfogpedaoifhbde"
