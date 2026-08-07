param([ValidateSet("Debug", "Release")][string]$Configuration = "Release")
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$profile = if ($Configuration -eq "Release") { "release" } else { "debug" }
& cargo build --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml") $(if ($Configuration -eq "Release") { "--release" })
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$sourceExe = Join-Path $repoRoot "native-host\target\$profile\fcp-host.exe"
$installRoot = Join-Path $env:LOCALAPPDATA "FursoyCookieProtector\native-host"
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
$installedExe = Join-Path $installRoot "fcp-host.exe"
Copy-Item -LiteralPath $sourceExe -Destination $installedExe -Force
$configRoot = Join-Path $env:LOCALAPPDATA "FursoyCookieProtector\config"
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
$manifestPath = Join-Path $installRoot "com.fursoy.cookie_protector.json"
$manifest = [ordered]@{
  name = "com.fursoy.cookie_protector"
  description = "FURSOY Cookie Protector native host"
  path = $installedExe
  type = "stdio"
  allowed_origins = @("chrome-extension://ikodegbaomnahbjiokfogpedaoifhbde/")
}
$manifestJson = $manifest | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, [System.Text.UTF8Encoding]::new($false))
$registryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fursoy.cookie_protector"
New-Item -Path $registryPath -Force | Out-Null
Set-Item -Path $registryPath -Value $manifestPath
Write-Host "Registered com.fursoy.cookie_protector for extension ikodegbaomnahbjiokfogpedaoifhbde"
