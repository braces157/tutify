# Embedded dependency setup, included in the application's source fingerprint.
[CmdletBinding()]
param([string]$ToolsDir = (Join-Path $env:LOCALAPPDATA 'Programs\Tuitify\tools'))

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$toolsRoot = [IO.Path]::GetFullPath($ToolsDir)
$stage = Join-Path ([IO.Path]::GetTempPath()) ('tuitify-youtube-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage -Force | Out-Null

function Get-VerifiedFile([string]$Url, [string]$Destination, [string]$ExpectedHash) {
    if ($ExpectedHash -notmatch '^[a-fA-F0-9]{64}$') { throw 'Upstream did not publish a valid SHA-256 checksum' }
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Destination
    if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -ne $ExpectedHash) {
        throw "Checksum mismatch for $([IO.Path]::GetFileName($Destination)); existing tools were preserved"
    }
}

try {
    $headers = @{ 'User-Agent' = 'Tuitify-YouTube-Setup'; 'Accept' = 'application/vnd.github+json' }
    $yt = Invoke-RestMethod -Uri 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest' -Headers $headers
    $ytBase = "https://github.com/yt-dlp/yt-dlp/releases/download/$($yt.tag_name)"
    $checksums = (Invoke-WebRequest -UseBasicParsing -Uri "$ytBase/SHA2-256SUMS").Content
    if ($checksums -is [byte[]]) { $checksums = [Text.Encoding]::UTF8.GetString($checksums) }
    $match = [regex]::Match($checksums, '(?m)^([a-fA-F0-9]{64})\s+\*?yt-dlp\.exe\s*$')
    if (-not $match.Success) { throw 'yt-dlp checksum is missing' }
    Get-VerifiedFile "$ytBase/yt-dlp.exe" (Join-Path $stage 'yt-dlp.exe') $match.Groups[1].Value

    $deno = Invoke-RestMethod -Uri 'https://api.github.com/repos/denoland/deno/releases/latest' -Headers $headers
    $denoAsset = @($deno.assets | Where-Object name -eq 'deno-x86_64-pc-windows-msvc.zip')[0]
    if (-not $denoAsset -or $denoAsset.digest -notmatch '^sha256:([a-fA-F0-9]{64})$') {
        throw 'Deno release is missing its Windows x64 asset or SHA-256 digest'
    }
    Get-VerifiedFile $denoAsset.browser_download_url (Join-Path $stage 'deno.zip') $Matches[1]
    Expand-Archive -LiteralPath (Join-Path $stage 'deno.zip') -DestinationPath (Join-Path $stage 'deno')
    $denoExe = Join-Path $stage 'deno\deno.exe'
    if (-not (Test-Path -LiteralPath $denoExe -PathType Leaf)) { throw 'Deno archive did not contain deno.exe' }

    $ffmpeg = Get-Command ffmpeg.exe -ErrorAction SilentlyContinue
    $stagedFfmpeg = $null
    if (-not $ffmpeg -and -not (Test-Path -LiteralPath (Join-Path $toolsRoot 'ffmpeg.exe'))) {
        $ffUrl = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip'
        $ffHash = (Invoke-WebRequest -UseBasicParsing -Uri "$ffUrl.sha256").Content
        if ($ffHash -is [byte[]]) { $ffHash = [Text.Encoding]::UTF8.GetString($ffHash) }
        $ffHash = ([string]$ffHash).Trim().Split(' ')[0]
        Get-VerifiedFile $ffUrl (Join-Path $stage 'ffmpeg.zip') $ffHash
        Expand-Archive -LiteralPath (Join-Path $stage 'ffmpeg.zip') -DestinationPath (Join-Path $stage 'ffmpeg')
        $stagedFfmpeg = @(Get-ChildItem -LiteralPath (Join-Path $stage 'ffmpeg') -Recurse -Filter ffmpeg.exe -File)[0].FullName
        if (-not $stagedFfmpeg) { throw 'FFmpeg archive did not contain ffmpeg.exe' }
    }

    # Validate every staged executable before replacing any existing dependency.
    & (Join-Path $stage 'yt-dlp.exe') --version
    if ($LASTEXITCODE -ne 0) { throw 'yt-dlp validation failed' }
    & $denoExe --version
    if ($LASTEXITCODE -ne 0) { throw 'Deno validation failed' }
    if ($stagedFfmpeg) {
        & $stagedFfmpeg -version | Select-Object -First 1
        if ($LASTEXITCODE -ne 0) { throw 'FFmpeg validation failed' }
    }
    New-Item -ItemType Directory -Path $toolsRoot -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $stage 'yt-dlp.exe') -Destination (Join-Path $toolsRoot 'yt-dlp.exe') -Force
    Copy-Item -LiteralPath $denoExe -Destination (Join-Path $toolsRoot 'deno.exe') -Force
    if ($stagedFfmpeg) { Copy-Item -LiteralPath $stagedFfmpeg -Destination (Join-Path $toolsRoot 'ffmpeg.exe') -Force }
    Write-Host "YouTube tools installed in $toolsRoot"
    Write-Host 'Playback tools are ready. Returning to Tuitify.'
} finally {
    $resolvedStage = [IO.Path]::GetFullPath($stage)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ($resolvedStage.StartsWith($expectedParent, [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($resolvedStage).StartsWith('tuitify-youtube-')) {
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force -ErrorAction SilentlyContinue
    }
}
