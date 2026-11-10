# Owned sleeping child only. Test files are sent to Recycle Bin, never permanently deleted.
#Requires -Version 7
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ("tokenscope-startup-dump-contract-" + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
$child = $null
try {
    $before = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - 1000
    $child = Start-Process -FilePath (Get-Command pwsh).Source -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 90') -WindowStyle Hidden -PassThru
    $exe = $child.Path
    $after = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() + 1000
    $argsForDump = @{ TargetPid=$child.Id; ExpectedParentPid=$PID; ExpectedExe=$exe; StartedAfterMs=$before; StartedBeforeMs=$after }
    $file = Join-Path $root 'startup-1.dmp'
    $result = & "$PSScriptRoot/capture-native-startup-dump.ps1" @argsForDump -OutputPath $file | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $result.status -ne 'captured') { throw "Controlled dump failed: $($result | ConvertTo-Json -Compress)" }
    $header = [IO.File]::ReadAllBytes($file)
    if ($header.Length -le 32 -or [Text.Encoding]::ASCII.GetString($header,0,4) -ne 'MDMP') { throw 'Not a valid minidump header' }
    $image = Join-Path $root 'startup-1.exe'
    if ($result.exe -ne 'startup-1.exe' -or -not (Test-Path -LiteralPath $image) -or
        $result.exe_sha256 -ne (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() -or
        $result.exe_sha256 -ne (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'Matching executable was not preserved' }
    foreach ($wrong in @('parent','exe','time')) {
        $candidate = $argsForDump.Clone()
        if ($wrong -eq 'parent') { $candidate.ExpectedParentPid = 1 }
        if ($wrong -eq 'exe') { $candidate.ExpectedExe = Join-Path $root 'other.exe' }
        if ($wrong -eq 'time') { $candidate.StartedBeforeMs = 1 }
        $wrongFile = Join-Path $root 'startup-2.dmp'
        $rejected = & "$PSScriptRoot/capture-native-startup-dump.ps1" @candidate -OutputPath $wrongFile | ConvertFrom-Json
        if ($LASTEXITCODE -eq 0 -or $rejected.status -ne 'unavailable' -or (Test-Path -LiteralPath $wrongFile)) { throw "Wrong $wrong identity was not rejected" }
    }
    $again = & "$PSScriptRoot/capture-native-startup-dump.ps1" @argsForDump -OutputPath $file | ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -or $again.status -ne 'unavailable') { throw 'Existing dump must not be overwritten' }
    Write-Output 'PASS startup dump: owned process, three identity rejections, no overwrite'
} finally {
    if ($child -and -not $child.HasExited) { $child.Kill(); $child.WaitForExit() }
    # Resolve and validate this exact owned path before the Recycle Bin operation.
    $resolved = (Resolve-Path -LiteralPath $root).Path
    if ($resolved -ne [IO.Path]::GetFullPath($root) -or [IO.Path]::GetFileName($resolved) -notmatch '^tokenscope-startup-dump-contract-[a-f0-9]{32}$') { throw 'Unexpected cleanup path' }
    Add-Type -AssemblyName Microsoft.VisualBasic.Core
    [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($resolved,[Microsoft.VisualBasic.FileIO.UIOption]::OnlyErrorDialogs,[Microsoft.VisualBasic.FileIO.RecycleOption]::SendToRecycleBin,[Microsoft.VisualBasic.FileIO.UICancelOption]::ThrowException)
}
