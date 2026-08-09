# Assembles the companion-app release zip: builds the release exe from source, then bundles it
# with install.ps1/install.bat/uninstall.ps1/uninstall.bat/README.txt from install/release/ into
# a single zip ready to attach to a GitHub Release. Run by the maintainer when cutting a release —
# end users never run this, they only ever see the zip it produces.
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$releaseTemplate = Join-Path $PSScriptRoot "release"

& cargo build --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml") --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$version = (& cargo metadata --manifest-path (Join-Path $repoRoot "native-host\Cargo.toml") --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version

$stagingRoot = Join-Path $repoRoot "native-host\target\release-package"
if (Test-Path -LiteralPath $stagingRoot) { Remove-Item -LiteralPath $stagingRoot -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stagingRoot | Out-Null

Copy-Item -LiteralPath (Join-Path $repoRoot "native-host\target\release\fursoy-vault-host.exe") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "install.ps1") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "install.bat") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "uninstall.ps1") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "uninstall.bat") -Destination $stagingRoot
Copy-Item -LiteralPath (Join-Path $releaseTemplate "README.txt") -Destination $stagingRoot

# Filename is deliberately NOT versioned: onboarding.ts links directly to
# https://github.com/FURSOY/fursoy-vault/releases/latest/download/fursoy-vault-windows.zip, a
# GitHub URL that always redirects to whatever the latest release's same-named asset is. Keeping
# this exact filename across releases is what makes that link never go stale. The release/tag
# itself is still versioned normally (v$version) for the changelog and history.
$zipPath = Join-Path $repoRoot "native-host\target\fursoy-vault-windows.zip"
if (Test-Path -LiteralPath $zipPath) { Remove-Item -LiteralPath $zipPath -Force }
Compress-Archive -Path (Join-Path $stagingRoot "*") -DestinationPath $zipPath

Write-Host ""
Write-Host "Release package ready: $zipPath"
Write-Host "Upload it as a GitHub Release asset (keep the filename fursoy-vault-windows.zip exactly) and tag the release v$version."
