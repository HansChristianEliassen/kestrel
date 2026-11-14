# Builds and signs a release. Run from anywhere:  powershell -File scripts\release.ps1
# Output: dist\v<version>\kestrel.exe and kestrel-update.sig. Upload BOTH to the GitHub release named
# v<version>; the updater only installs releases whose signature checks out.
param(
    [string]$Key = "$env:USERPROFILE\.kestrel\release-signing.key"
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not (Test-Path $Key)) { throw "Signing key not found at $Key. Restore it from your backup (see README, Releasing)." }

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Build failed." }

$out = Join-Path $root "dist\v$version"
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item target\release\kestrel.exe "$out\kestrel.exe" -Force
& target\release\kestrel-sign.exe sign $Key "$out\kestrel.exe" $version "$out\kestrel-update.sig"
if ($LASTEXITCODE -ne 0) { throw "Signing failed." }

$sha = (Get-FileHash "$out\kestrel.exe" -Algorithm SHA256).Hash
Write-Host ""
Write-Host "Kestrel $version is ready in $out"
Write-Host "SHA-256: $sha"
Write-Host "Next: git tag -a v$version -m 'Kestrel $version'; git push origin v$version"
Write-Host "Then create the GitHub release for tag v$version and attach kestrel.exe and kestrel-update.sig."
