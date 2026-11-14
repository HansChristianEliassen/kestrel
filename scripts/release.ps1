# Builds and signs a release. Run from anywhere:  powershell -File scripts\release.ps1
# Output: dist\v<version>\kestrel.exe, kestrel-update.sig and THIRD-PARTY-NOTICES.txt. Upload all three to the GitHub
# release named v<version>; the updater only installs releases whose signature checks out.
param(
    [string]$Key = "$env:USERPROFILE\.kestrel\release-signing.key"
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not (Test-Path $Key)) { throw "Signing key not found at $Key. Restore it from your backup (see README, Releasing)." }

# The exe embeds THIRD-PARTY-NOTICES.txt, so it must match the dependencies being built (and be committed).
powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'third-party-notices.ps1')
git diff --quiet -- THIRD-PARTY-NOTICES.txt
if ($LASTEXITCODE -ne 0) { throw 'THIRD-PARTY-NOTICES.txt was out of date and has been regenerated. Review and commit it, then run this again.' }

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Build failed." }

$out = Join-Path $root "dist\v$version"
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item target\release\kestrel.exe "$out\kestrel.exe" -Force
Copy-Item THIRD-PARTY-NOTICES.txt "$out\THIRD-PARTY-NOTICES.txt" -Force
& target\release\kestrel-sign.exe sign $Key "$out\kestrel.exe" $version "$out\kestrel-update.sig"
if ($LASTEXITCODE -ne 0) { throw "Signing failed." }

$sha = (Get-FileHash "$out\kestrel.exe" -Algorithm SHA256).Hash
Write-Host ""
Write-Host "Kestrel $version is ready in $out"
Write-Host "SHA-256: $sha"
Write-Host "Next: git tag -a v$version -m 'Kestrel $version'; git push origin v$version"
Write-Host "Then create the GitHub release for tag v$version and attach kestrel.exe, kestrel-update.sig and THIRD-PARTY-NOTICES.txt."
