param([string]$VpkCommand = "vpk")

# Builds the Rust host and packages it as a one-click Velopack Setup.exe plus the feed/package
# assets required by automatic updates. End users only need FURSOY-Vault-Setup.exe.
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$releaseTemplate = Join-Path $PSScriptRoot "release"
$targetRoot = Join-Path $repoRoot "native-host\target"

& cargo build --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml") --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$version = (& cargo metadata --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml") --locked --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
$stagingRoot = Join-Path $targetRoot "release-package"
$outputRoot = Join-Path $targetRoot "velopack"
foreach ($path in @($stagingRoot, $outputRoot)) {
  $resolvedParent = (Resolve-Path (Split-Path -Parent $path)).Path
  if (-not $resolvedParent.StartsWith($targetRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to clean a package directory outside native-host target."
  }
  if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
  New-Item -ItemType Directory -Force -Path $path | Out-Null
}

Copy-Item -LiteralPath (Join-Path $targetRoot "release\fursoy-vault-host.exe") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "install.ps1") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "uninstall.ps1") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "README.txt") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $repoRoot "LICENSE") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $repoRoot "SOURCE.txt") -Destination $stagingRoot
[System.IO.File]::WriteAllText((Join-Path $stagingRoot "VERSION"), "$version`n", [System.Text.UTF8Encoding]::new($false))

& $VpkCommand pack `
  --packId "FURSOY.Vault" `
  --packVersion $version `
  --packDir $stagingRoot `
  --mainExe "fursoy-vault-host.exe" `
  --packTitle "FURSOY Vault" `
  --packAuthors "FURSOY" `
  --shortcuts "None" `
  --channel "win" `
  --outputDir $outputRoot `
  --delta "None" `
  --noPortable
if ($LASTEXITCODE -ne 0) { throw "vpk pack failed" }

$generatedSetup = @(Get-ChildItem -LiteralPath $outputRoot -Filter "*-Setup.exe" -File)
if ($generatedSetup.Count -ne 1) { throw "Velopack did not produce exactly one Setup.exe" }
$friendlySetup = Join-Path $outputRoot "FURSOY-Vault-Setup.exe"
Move-Item -LiteralPath $generatedSetup[0].FullName -Destination $friendlySetup -Force

Write-Host ""
Write-Host "Release package ready: $friendlySetup"
Write-Host "Publish every file under $outputRoot; onboarding downloads FURSOY-Vault-Setup.exe."
