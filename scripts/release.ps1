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
$cargoToml = Get-Content 'Cargo.toml' -Raw
$version = if ($cargoToml -match 'version\s*=\s*"([^"]+)"') { $Matches[1] } else { '0.1.1' }
$releaseFolder = Join-Path (Get-Location) "dist/Tuitify-$version-windows-x86_64"
New-Item -ItemType Directory -Force -Path $releaseFolder | Out-Null
Copy-Item -LiteralPath 'target/release/tuitify.exe' -Destination $releaseFolder
Copy-Item -LiteralPath 'README.md', 'LICENSE', 'ROADMAP.md', 'VALIDATION.md', 'BENCHMARKS.md' -Destination $releaseFolder
$zipPath = "$releaseFolder.zip"
Compress-Archive -Path "$releaseFolder/*" -DestinationPath $zipPath -Force
$checksum = Get-FileHash -Algorithm SHA256 -LiteralPath $zipPath
"$($checksum.Hash.ToLower())  $([System.IO.Path]::GetFileName($zipPath))" | Set-Content -Encoding ascii -LiteralPath "$zipPath.sha256"
$exeChecksum = Get-FileHash -Algorithm SHA256 -LiteralPath 'target/release/tuitify.exe'
"$($exeChecksum.Hash.ToLower())  tuitify.exe" | Set-Content -Encoding ascii -LiteralPath "dist/tuitify.exe.sha256"
Write-Output "Release: $zipPath"
Write-Output "Version: $version"
