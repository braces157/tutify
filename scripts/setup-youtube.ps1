[CmdletBinding()]
param([string]$ToolsDir = (Join-Path $env:LOCALAPPDATA 'Programs\Tuitify\tools'))
$ErrorActionPreference = 'Stop'
& (Join-Path (Split-Path $PSScriptRoot -Parent) 'src\youtube\setup.ps1') -ToolsDir $ToolsDir
