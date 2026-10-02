# Read-only artifact/source identity helpers; Windows PowerShell 5.1 compatible.
function Get-TuitifyBuildIdentity {
    param([Parameter(Mandatory = $true)][string]$SourceExe)
    $source = (Resolve-Path -LiteralPath $SourceExe -ErrorAction Stop).Path
    $before = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $source
    $start.Arguments = 'version --json'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'Could not read executable build identity' }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(10000)) {
            $process.Kill() # Only this helper's own version child, never another player.
            $process.WaitForExit()
            throw 'Executable build identity exceeded ten seconds'
        }
        $text = $stdout.GetAwaiter().GetResult()
        $errorText = $stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0 -or $errorText.Length -gt 0 -or $text.Length -gt 65536) { throw 'Executable build identity failed; rebuild with identity support' }
        $report = $text | ConvertFrom-Json -ErrorAction Stop
    } finally { $process.Dispose() }
    $after = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($after -ne $before -or $report.on_disk_executable_sha256 -ne $before) { throw 'Executable changed during identity inspection, or reported another disk hash' }
    if ($report.format -ne 'tuitify-build' -or $report.format_version -ne 1) { throw 'Unsupported build identity format' }
    foreach ($name in @('source_sha256', 'build_id', 'settings_sha256')) {
        if ($report.build.$name -cnotmatch '^[0-9a-f]{64}$') { throw "Invalid build identity field: $name" }
    }
    if ($report.build.version -cnotmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') { throw 'Invalid executable package version' }
    if ($null -ne $report.build.source_commit -and $report.build.source_commit -cnotmatch '^[0-9a-f]{40}(?:[0-9a-f]{24})?$') { throw 'Invalid source commit' }
    if ($null -ne $report.build.source_dirty -and $report.build.source_dirty -isnot [bool]) { throw 'Invalid source dirty marker' }
    if ($report.build.os -ne 'windows' -or $report.build.target -ne 'x86_64-pc-windows-msvc' -or $report.build.architecture -ne 'x86_64') { throw 'This release workflow supports Windows x64 only' }
    if ($report.build.profile -ne 'release') { throw 'Release executable required' }
    return $report
}

function Get-TuitifySourceDigest {
    param([Parameter(Mandatory = $true)][string]$ProjectRoot)
    $root = (Resolve-Path -LiteralPath $ProjectRoot -ErrorAction Stop).Path.TrimEnd('\', '/')
    $names = New-Object 'System.Collections.Generic.List[string]'
    foreach ($name in @('Cargo.toml', 'Cargo.lock', 'build.rs', 'build_support.rs')) { $names.Add($name) }
    function Add-SourceFiles([string]$directory) {
        $item = Get-Item -LiteralPath $directory -Force -ErrorAction Stop
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Build source must not contain reparse points' }
        foreach ($item in Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Build source must not contain reparse points' }
            if ($item.PSIsContainer) { Add-SourceFiles $item.FullName }
            else { $names.Add($item.FullName.Substring($root.Length + 1).Replace('\', '/')) }
        }
    }
    Add-SourceFiles (Join-Path $root 'src')
    $ordered = $names.ToArray()
    # UTF-8 byte sorting matches Rust even for non-BMP filenames.
    [Array]::Sort($ordered, [Comparison[string]]{
        param($left, $right)
        [StringComparer]::Ordinal.Compare([BitConverter]::ToString([Text.Encoding]::UTF8.GetBytes($left)), [BitConverter]::ToString([Text.Encoding]::UTF8.GetBytes($right)))
    })
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $header = [Text.Encoding]::UTF8.GetBytes("tuitify-source-tree-v1`0")
        [void]$hash.TransformBlock($header, 0, $header.Length, $null, 0)
        foreach ($name in $ordered) {
            $path = Join-Path $root $name
            if ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Build source must not contain reparse points' }
            $nameBytes = [Text.Encoding]::UTF8.GetBytes($name)
            $bytes = [IO.File]::ReadAllBytes($path)
            foreach ($part in @([BitConverter]::GetBytes([UInt64]$nameBytes.Length), $nameBytes, [BitConverter]::GetBytes([UInt64]$bytes.Length), $bytes)) {
                [void]$hash.TransformBlock($part, 0, $part.Length, $null, 0)
            }
        }
        [void]$hash.TransformFinalBlock([byte[]]@(), 0, 0)
        return [BitConverter]::ToString($hash.Hash).Replace('-', '').ToLowerInvariant()
    } finally { $hash.Dispose() }
}

function Write-TuitifyManifest {
    param([Parameter(Mandatory = $true)]$Manifest, [Parameter(Mandatory = $true)][string]$Path)
    [IO.File]::WriteAllText($Path, (($Manifest | ConvertTo-Json -Depth 12) + "`n"), (New-Object Text.UTF8Encoding($false)))
}
