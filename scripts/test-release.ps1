# Package regressions use owned fixtures only; never mutate saved listening data or PATH.
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Split-Path $PSScriptRoot -Parent)).Path
. (Join-Path $PSScriptRoot 'build-manifest.ps1')
$id = [guid]::NewGuid().ToString('N')
$outputRoot = Join-Path $projectRoot "dist/package-tests-$id"
$ghost = Join-Path $projectRoot "docs/package-fixture-$id.txt"
$broken = Join-Path $projectRoot "docs/package-fixture-$id.md"
$staleSource = Join-Path $projectRoot "src/package-fixture-$id.txt"
$packageScript = Join-Path $PSScriptRoot 'package-release.ps1'
$identity = Get-TuitifyBuildIdentity (Join-Path $projectRoot 'target/release/tuitify.exe')
$name = "Tuitify-$($identity.build.version)-windows-x86_64"
$zip = Join-Path $outputRoot "$name.zip"
$results = New-Object 'System.Collections.Generic.List[string]'
function Read-Package {
    $extract = Join-Path $outputRoot ('inspection-' + [guid]::NewGuid().ToString('N'))
    Expand-Archive -LiteralPath $zip -DestinationPath $extract
    $external = Get-Content -LiteralPath (Join-Path $outputRoot "$name.manifest.json") -Raw | ConvertFrom-Json
    $manifest = Get-Content -LiteralPath (Join-Path $extract 'release-manifest.json') -Raw | ConvertFrom-Json
    if ($manifest.format -ne 'tuitify-package' -or $manifest.build.build_id -ne $identity.build.build_id -or $external.build.build_id -ne $identity.build.build_id) { throw 'Build/package identities differ' }
    foreach ($entry in $manifest.files) {
        $file = Join-Path $extract $entry.path
        if ((Get-Item -LiteralPath $file).Length -ne $entry.bytes -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) { throw 'Extracted file disagrees with payload manifest' }
    }
    if (@(Get-ChildItem -LiteralPath $extract -File -Force -Recurse).Count -ne $manifest.files.Count + 1) { throw 'Unexpected extracted file' }
    foreach ($entry in $external.artifacts) {
        $file = if ($entry.location) { Join-Path $extract $entry.path } else { Join-Path $outputRoot $entry.path }
        if ((Get-Item -LiteralPath $file).Length -ne $entry.bytes -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) { throw 'External asset hash mismatch' }
    }
    foreach ($file in @('scripts/install.ps1', 'scripts/build-manifest.ps1', 'README.md', 'docs/index.html')) {
        if ((Get-FileHash -LiteralPath (Join-Path $extract $file)).Hash -ne (Get-FileHash -LiteralPath (Join-Path $projectRoot $file)).Hash) { throw 'Installer/current docs absent or stale' }
    }
    if (@($manifest.files | Where-Object { $_.path -match '^(?:work|src|target|\.agents|\.git)/|IMPLEMENTATION_PROGRESS|AGENTS\.md|package-fixture.*\.md' }).Count) { throw 'Unintended/private artifact packaged' }
    return $manifest
}
function Expect-Rejected([string]$reason) {
    $zipBefore = (Get-FileHash -LiteralPath $zip).Hash
    $manifestBefore = (Get-FileHash -LiteralPath (Join-Path $outputRoot "$name.manifest.json")).Hash
    try { & $packageScript -OutputDir $outputRoot | Out-Null; throw 'Expected rejection did not occur' }
    catch { if ($_.Exception.Message -notmatch $reason) { throw } }
    if ((Get-FileHash -LiteralPath $zip).Hash -ne $zipBefore -or (Get-FileHash -LiteralPath (Join-Path $outputRoot "$name.manifest.json")).Hash -ne $manifestBefore) { throw 'Rejected package changed existing artifacts' }
    if (@(Get-ChildItem -LiteralPath $outputRoot -Directory -Force | Where-Object Name -Like '.staging-*').Count) { throw 'Rejected package left staging behind' }
}
try {
    foreach ($file in @($ghost, $broken, $staleSource)) { if (Test-Path -LiteralPath $file) { throw 'Owned fixture path unexpectedly exists' } }
    [IO.File]::WriteAllText($ghost, 'This removed asset must not survive a subsequent package.')
    & $packageScript -OutputDir $outputRoot | Out-Null
    $first = Read-Package
    if (-not @($first.files | Where-Object path -eq "docs/package-fixture-$id.txt").Count) { throw 'Initial package fixture missing' }
    Remove-Item -LiteralPath $ghost
    & $packageScript -OutputDir $outputRoot | Out-Null
    $second = Read-Package
    if (@($second.files | Where-Object path -eq "docs/package-fixture-$id.txt").Count) { throw 'Removed asset survived fresh staging' }
    $results.Add('Same-version rebuild removes deleted asset; exact manifests, installer and current docs verified')
    [IO.File]::WriteAllText($broken, '[Broken](asset-that-does-not-exist.txt)')
    Expect-Rejected 'Missing or out-of-package link'
    $results.Add('Missing local link rejected without changing published artifacts')
    [IO.File]::WriteAllText($broken, '[Escape](../../Cargo.toml)')
    Expect-Rejected 'Missing or out-of-package link'
    $results.Add('Escaping local link rejected without changing published artifacts')
    Remove-Item -LiteralPath $broken
    [IO.File]::WriteAllText($staleSource, 'A different compiled source tree must require a rebuild.')
    Expect-Rejected 'does not match the current package/build source'
    $results.Add('Stale same-version executable rejected by source digest')
    Remove-Item -LiteralPath $staleSource
    [IO.File]::WriteAllText($ghost, '[not a link]')
    # Reparse-point rejection is verified with an owned junction, needing no symlink privilege.
    $junction = Join-Path $projectRoot "docs/package-junction-$id"
    if (Test-Path -LiteralPath $junction) { throw 'Owned junction path unexpectedly exists' }
    New-Item -ItemType Junction -Path $junction -Target $outputRoot | Out-Null
    try { Expect-Rejected 'reparse points' } finally { [IO.Directory]::Delete($junction) }
    $results.Add('Reparse-point payload rejected; owned junction removed without deleting its target')
    Write-TuitifyManifest -Path (Join-Path $projectRoot 'dist/package-validation.json') -Manifest ([ordered]@{ build=$identity.build; results=$results.ToArray(); payload_files=$second.files.Count; saved_state_and_path_untouched=$true })
    $results | Write-Output
} finally {
    foreach ($file in @($ghost, $broken, $staleSource)) { if (Test-Path -LiteralPath $file -PathType Leaf) { Remove-Item -LiteralPath $file } }
    # Keep generated evidence/extractions under ignored dist for inspection.
}
