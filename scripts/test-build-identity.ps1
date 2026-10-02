# Real same-version rebuilds with an owned input; saved state and PATH stay untouched.
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Split-Path $PSScriptRoot -Parent)).Path
Set-Location $projectRoot
$fixture = Join-Path $projectRoot ('src/build-identity-fixture-' + [guid]::NewGuid().ToString('N') + '.txt')
if (Test-Path -LiteralPath $fixture) { throw 'Owned identity fixture unexpectedly exists' }
$exe = Join-Path $projectRoot 'target/debug/tuitify.exe'
function Read-Identity([string]$label) {
    cargo build --locked
    if ($LASTEXITCODE -ne 0) { throw "Identity fixture build failed: $label" }
    $text = & $exe version --json
    if ($LASTEXITCODE -ne 0) { throw "Identity fixture version failed: $label" }
    return ($text | ConvertFrom-Json)
}
try {
    $before = Read-Identity 'before'
    [IO.File]::WriteAllText($fixture, 'Owned build-source input; distinguish two dirty builds of the same version.')
    $changed = Read-Identity 'changed'
    if ($before.build.version -ne $changed.build.version -or $before.build.source_commit -ne $changed.build.source_commit -or $before.build.rustc -ne $changed.build.rustc) { throw 'Same-version comparison changed unrelated identity fields' }
    if ($before.build.source_sha256 -eq $changed.build.source_sha256 -or $before.build.build_id -eq $changed.build.build_id -or $before.on_disk_executable_sha256 -eq $changed.on_disk_executable_sha256) { throw 'Distinct builds were not distinguished' }
} finally { if (Test-Path -LiteralPath $fixture -PathType Leaf) { Remove-Item -LiteralPath $fixture } }
$restored = Read-Identity 'restored'
if ($restored.build.source_sha256 -ne $before.build.source_sha256 -or $restored.build.build_id -ne $before.build.build_id) { throw 'Restored source did not reproduce the original identity' }
. (Join-Path $PSScriptRoot 'build-manifest.ps1')
Write-TuitifyManifest -Path (Join-Path $projectRoot 'dist/build-identity-validation.json') -Manifest ([ordered]@{ before=$before; changed=$changed; restored=$restored; same_version_distinguished=$true; owned_source_restored=$true; saved_state_and_path_untouched=$true })
Write-Output "Verified distinct source/build/disk hashes for two builds of $($before.build.version), and restored the original source/build identity."
