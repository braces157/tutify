param(
    [string]$OutputDir = (Join-Path (Split-Path $PSScriptRoot -Parent) 'dist'),
    [string]$Validation = 'Packaging only; no build/test checks were run by this helper'
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Split-Path $PSScriptRoot -Parent)).Path
. (Join-Path $PSScriptRoot 'build-manifest.ps1')
$metadataText = cargo metadata --manifest-path (Join-Path $projectRoot 'Cargo.toml') --locked --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'Could not read package manifest' }
$version = (($metadataText | ConvertFrom-Json).packages | Where-Object name -eq 'tuitify').version
$exe = Join-Path $projectRoot 'target/release/tuitify.exe'
$identity = Get-TuitifyBuildIdentity -SourceExe $exe
if ($identity.build.version -ne $version -or $identity.build.source_sha256 -ne (Get-TuitifySourceDigest $projectRoot)) { throw 'Release executable does not match the current package/build source; rebuild before packaging' }
$outputRoot = [IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$outputRoot = (Resolve-Path -LiteralPath $outputRoot).Path
if ((Get-Item -LiteralPath $outputRoot).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release output must not be a reparse point' }
$workspace = Join-Path $outputRoot ('.staging-' + [guid]::NewGuid().ToString('N'))
$releaseName = "Tuitify-$version-windows-x86_64"
$stage = Join-Path $workspace $releaseName
New-Item -ItemType Directory -Path $stage | Out-Null
function Publish-File([string]$source, [string]$destination) {
    if (Test-Path -LiteralPath $destination) { [IO.File]::Replace($source, $destination, "$source.previous") }
    else { [IO.File]::Move($source, $destination) }
}
try {
    $documents = @('README.md', 'LICENSE', 'ROADMAP.md', 'VALIDATION.md', 'BENCHMARKS.md', 'ARCHITECTURE.md', 'CHANGELOG.md', 'PERFORMANCE_REVIEW.md', 'REFACTOR_REVIEW.md')
    $items = @($exe) + @($documents | ForEach-Object { Join-Path $projectRoot $_ }) + @(Join-Path $projectRoot 'docs')
    foreach ($item in $items) {
        $entry = Get-Item -LiteralPath $item -Force
        if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release payload must not contain reparse points' }
        if ($entry.PSIsContainer -and @(Get-ChildItem -LiteralPath $item -Force -Recurse | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) { throw 'Release payload must not contain reparse points' }
        Copy-Item -LiteralPath $item -Destination $stage -Recurse
    }
    $scriptDir = Join-Path $stage 'scripts'
    New-Item -ItemType Directory -Path $scriptDir | Out-Null
    foreach ($script in @('install.ps1', 'build-manifest.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $script) -Destination $scriptDir }
    $notes = Join-Path $projectRoot "docs/releases/v$version.md"
    if (Test-Path -LiteralPath $notes -PathType Leaf) { Copy-Item -LiteralPath $notes -Destination (Join-Path $stage "RELEASE_NOTES-$version.md") }
    foreach ($file in Get-ChildItem -LiteralPath $stage -File -Recurse | Where-Object Extension -In @('.md', '.html', '.css')) {
        $text = [IO.File]::ReadAllText($file.FullName)
        $links = @()
        if ($file.Extension -eq '.md') { $links += @([regex]::Matches($text, '\]\(([^\s)]+)(?:\s+"[^"]*")?\)') | ForEach-Object { $_.Groups[1].Value }) }
        if ($file.Extension -eq '.html') { $links += @([regex]::Matches($text, '(?i)(?:href|src)\s*=\s*["'']([^"'']+)["'']') | ForEach-Object { $_.Groups[1].Value }) }
        if ($file.Extension -eq '.css') { $links += @([regex]::Matches($text, 'url\(\s*(?:"([^"]*)"|''([^'']*)''|([^"''()\s]+))\s*\)') | ForEach-Object { if ($_.Groups[1].Success) { $_.Groups[1].Value } elseif ($_.Groups[2].Success) { $_.Groups[2].Value } else { $_.Groups[3].Value } }) }
        foreach ($link in $links) {
            if ($link -match '^(?:https?://|mailto:|data:|#|//)') { continue }
            if ($link -match '^[A-Za-z][A-Za-z0-9+.-]*:|^[\\/]') { throw "Unsupported package link in $($file.Name): $link" }
            $relative = [Uri]::UnescapeDataString(($link -split '[?#]', 2)[0])
            $target = [IO.Path]::GetFullPath((Join-Path $file.DirectoryName $relative))
            if (-not $target.StartsWith($stage + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "Missing or out-of-package link in $($file.Name): $link" }
        }
    }
    $payload = @(Get-ChildItem -LiteralPath $stage -File -Force -Recurse | Sort-Object FullName | ForEach-Object {
        [ordered]@{ path=$_.FullName.Substring($stage.Length + 1).Replace('\', '/'); bytes=$_.Length; sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    $exeEntry = @($payload | Where-Object path -eq 'tuitify.exe')[0]
    if ($exeEntry.sha256 -ne $identity.on_disk_executable_sha256) { throw 'Staged executable hash mismatch' }
    $manifestPath = Join-Path $stage 'release-manifest.json'
    Write-TuitifyManifest -Path $manifestPath -Manifest ([ordered]@{ format='tuitify-package'; format_version=1; build=$identity.build; validation=$Validation; hash_scope='Every payload file; the external asset manifest hashes this manifest to avoid a circular self-hash'; files=$payload })
    $tempZip = Join-Path $workspace "$releaseName.zip"
    Compress-Archive -LiteralPath @(Get-ChildItem -LiteralPath $stage -Force | ForEach-Object FullName) -DestinationPath $tempZip
    $extracted = Join-Path $workspace 'extracted'
    Expand-Archive -LiteralPath $tempZip -DestinationPath $extracted
    if (@(Get-ChildItem -LiteralPath $extracted -File -Force -Recurse).Count -ne $payload.Count + 1) { throw 'Archive file set differs from the manifest' }
    foreach ($entry in $payload) {
        $file = Join-Path $extracted $entry.path
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -ne $entry.bytes -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) { throw "Archive payload mismatch: $($entry.path)" }
    }
    $manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ((Get-FileHash -LiteralPath (Join-Path $extracted 'release-manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifestHash) { throw 'Archive manifest mismatch' }
    $zipHash = (Get-FileHash -LiteralPath $tempZip -Algorithm SHA256).Hash.ToLowerInvariant()
    $assetManifest = [ordered]@{ format='tuitify-release-assets'; format_version=1; build=$identity.build; validation=$Validation; artifacts=@(
        [ordered]@{ path='tuitify.exe'; bytes=$exeEntry.bytes; sha256=$exeEntry.sha256 },
        [ordered]@{ path="$releaseName.zip"; bytes=(Get-Item -LiteralPath $tempZip).Length; sha256=$zipHash },
        [ordered]@{ path='release-manifest.json'; location='Inside ZIP'; bytes=(Get-Item -LiteralPath $manifestPath).Length; sha256=$manifestHash }
    ) }
    $tempManifest = Join-Path $workspace "$releaseName.manifest.json"
    Write-TuitifyManifest -Path $tempManifest -Manifest $assetManifest
    $tempZipHash = "$tempZip.sha256"
    "$zipHash  $releaseName.zip" | Set-Content -LiteralPath $tempZipHash -Encoding ascii
    $tempExeHash = Join-Path $workspace 'tuitify.exe.sha256'
    "$($exeEntry.sha256)  tuitify.exe" | Set-Content -LiteralPath $tempExeHash -Encoding ascii
    # Publish validated, immutable staged bytes. The manifest is published last.
    foreach ($pair in @(@($tempZip, "$releaseName.zip"), @((Join-Path $stage 'tuitify.exe'), 'tuitify.exe'), @($tempZipHash, "$releaseName.zip.sha256"), @($tempExeHash, 'tuitify.exe.sha256'), @($tempManifest, "$releaseName.manifest.json"))) { Publish-File $pair[0] (Join-Path $outputRoot $pair[1]) }
    foreach ($artifact in $assetManifest.artifacts | Where-Object { -not $_.location }) {
        if ((Get-FileHash -LiteralPath (Join-Path $outputRoot $artifact.path) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $artifact.sha256) { throw 'Published artifact hash mismatch' }
    }
    Write-Output "Release: $(Join-Path $outputRoot "$releaseName.zip")"
    Write-Output "Build ID: $($identity.build.build_id)"
    Write-Output "Checks: $Validation"
} finally {
    $resolvedWorkspace = (Resolve-Path -LiteralPath $workspace).Path
    if (-not $resolvedWorkspace.StartsWith($outputRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or (Split-Path $resolvedWorkspace -Leaf) -notmatch '^\.staging-[0-9a-f]{32}$' -or ((Get-Item -LiteralPath $resolvedWorkspace).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Refusing cleanup outside the release workspace' }
    Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force
}
