[CmdletBinding()]
param(
    [string]$ToolsDir = (Join-Path $env:LOCALAPPDATA 'Programs\Tuitify\tools'),
    [string]$RequirementsFile = (Join-Path $PSScriptRoot 'music-requirements.txt')
)
$ErrorActionPreference = 'Stop'
$musicRoot = Join-Path ([IO.Path]::GetFullPath($ToolsDir)) 'ytmusic'
$python = Get-Command python.exe -ErrorAction SilentlyContinue
if (-not $python) { throw 'YouTube Music needs Python 3.10 or newer. Install Python, then run tuitify youtube music-setup.' }
$pythonExe = $python.Source
& $pythonExe -I -c 'import sys; sys.exit(0 if sys.version_info >= (3,10) else 1)'
if ($LASTEXITCODE -ne 0) { throw 'Python 3.10 or newer is required; run tuitify youtube music-setup after installing it.' }
New-Item -ItemType Directory -Path $musicRoot -Force | Out-Null
$musicPython = Join-Path $musicRoot 'Scripts\python.exe'
if (-not (Test-Path -LiteralPath $musicPython)) {
    & $pythonExe -I -m venv $musicRoot
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the isolated YouTube Music environment' }
}
& $musicPython -I -m pip --isolated install --disable-pip-version-check --no-cache-dir --only-binary=:all: --no-deps --require-hashes -r $RequirementsFile
if ($LASTEXITCODE -ne 0) { throw 'Pinned YouTube Music dependency installation failed' }
& $musicPython -I -c 'import ytmusicapi; import importlib.metadata; print("YouTube Music", importlib.metadata.version("ytmusicapi"))'
if ($LASTEXITCODE -ne 0) { throw 'YouTube Music dependency check failed' }
Write-Host 'Music is ready. Connect your library from F6 Tools when you want.'
