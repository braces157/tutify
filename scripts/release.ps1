param([string]$OutputDir = (Join-Path (Split-Path $PSScriptRoot -Parent) 'dist'))
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
cargo fmt --check
if ($LASTEXITCODE -ne 0) { throw 'Formatting failed' }
cargo test --locked
if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
cargo clippy --all-targets --locked -- -D warnings
if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
& (Join-Path $PSScriptRoot 'package-release.ps1') -OutputDir $OutputDir -Validation 'Formatting, locked tests, strict Clippy and locked release build passed'
