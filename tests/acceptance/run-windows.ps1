param([switch]$SkipBuild)
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path

if (-not $SkipBuild) {
  & cargo build --release --locked --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml")
  if ($LASTEXITCODE -ne 0) { throw "native host build failed" }
  Push-Location (Join-Path $repoRoot "extension")
  try { & npm.cmd run package; if ($LASTEXITCODE -ne 0) { throw "extension package failed" } }
  finally { Pop-Location }
}

$hostExe = Join-Path $repoRoot "native-host\target\release\fursoy-vault-host.exe"
& python (Join-Path $PSScriptRoot "native_handshake.py") $hostExe
if ($LASTEXITCODE -ne 0) { throw "native handshake acceptance failed" }

Write-Host "Automated Windows acceptance gates passed."
Write-Host "Run the hardware rows in MATRIX.md before a public release."
