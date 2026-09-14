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
try {
    Copy-Item -LiteralPath $source -Destination $destination -Force -ErrorAction Stop
} catch {
    $backup = Join-Path $installRoot "tuitify.exe.old"
    if (Test-Path -LiteralPath $backup) {
        Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
    }
    Move-Item -LiteralPath $destination -Destination $backup -Force
    Copy-Item -LiteralPath $source -Destination $destination -Force
}

$currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$pathEntries = if ([string]::IsNullOrWhiteSpace($currentUserPath)) {
    @()
} else {
    $currentUserPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)
}

$otherEntries = @($pathEntries | Where-Object {
    -not [string]::Equals(
        [System.IO.Path]::GetFullPath($_.Trim()),
        $installRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    )
})

$newUserPath = ((@($installRoot) + $otherEntries) -join ';').Trim(';')
if ($newUserPath -ne $currentUserPath) {
    [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
}

$otherProcessEntries = @(($env:Path -split ';') | Where-Object {
    $_ -and -not [string]::Equals(
        [System.IO.Path]::GetFullPath($_.Trim()),
        $installRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    )
})
$env:Path = (@($installRoot) + $otherProcessEntries) -join ';'

$installedCommand = Get-Command tuitify -ErrorAction Stop
if ($installedCommand.Source -ne $destination) {
    throw "Another command shadows the installation: $($installedCommand.Source)"
}
& $installedCommand.Source --version | Out-Host
Write-Host "Installed Tuitify to $destination"
Write-Host 'Open a new terminal and run: tuitify'
