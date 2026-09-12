param(
    [string]$SourceExe,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Tuitify')
)

$ErrorActionPreference = 'Stop'

function Resolve-TuitifySource {
    param([string]$ExplicitSource)

    if ($ExplicitSource) {
        $resolved = Resolve-Path -LiteralPath $ExplicitSource -ErrorAction Stop
        return $resolved.Path
    }

    $candidates = @(
        (Join-Path $PSScriptRoot '..\target\release\tuitify.exe'),
        (Join-Path $PSScriptRoot '..\tuitify.exe')
    )

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    throw 'Could not find tuitify.exe. Run cargo build --release first, or pass -SourceExe <path>.'
}

$source = Resolve-TuitifySource -ExplicitSource $SourceExe
$installRoot = [System.IO.Path]::GetFullPath($InstallDir)
$destination = Join-Path $installRoot 'tuitify.exe'

New-Item -ItemType Directory -Path $installRoot -Force | Out-Null
Copy-Item -LiteralPath $source -Destination $destination -Force

$currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$pathEntries = if ([string]::IsNullOrWhiteSpace($currentUserPath)) {
    @()
} else {
    $currentUserPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)
}

$alreadyPresent = $pathEntries | Where-Object {
    [string]::Equals(
        [System.IO.Path]::GetFullPath($_.Trim()),
        $installRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    )
}

if (-not $alreadyPresent) {
    $newUserPath = (($pathEntries + $installRoot) -join ';').Trim(';')
    [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
}

if (-not (($env:Path -split ';') | Where-Object {
    $_ -and [string]::Equals(
        [System.IO.Path]::GetFullPath($_.Trim()),
        $installRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    )
})) {
    $env:Path = "$installRoot;$env:Path"
}

$installedCommand = Get-Command tuitify -ErrorAction Stop
& $installedCommand.Source --version | Out-Host
Write-Host "Installed Tuitify to $destination"
Write-Host 'Open a new terminal and run: tuitify'
