<#
.SYNOPSIS
    Synthetic contract tests for read-tray-notification-area.ps1.

.DESCRIPTION
    Exercises the shared result-building path with synthetic button records:
    TokenScope matching, non-contiguous command IDs, query timeout, count /
    detail contradiction, unobservable overflow, and truncated text. No real
    notification-area icon is created or modified; the script's own -SelfTest
    mode only builds in-memory records.
#>
$ErrorActionPreference = 'Stop'

function Invoke-SelfTest([string]$scenario) {
    $raw = & "$PSScriptRoot/read-tray-notification-area.ps1" -SelfTest -SelfTestScenario $scenario
    if ($LASTEXITCODE -ne 0) { throw "${scenario}: self-test exited $LASTEXITCODE" }
    return ($raw | ConvertFrom-Json)
}

$match = Invoke-SelfTest 'match'
if (-not $match.ok -or $match.visible_count -ne 2 -or $match.overflow_count -ne 1 -or
    $match.total_count -ne 3 -or $match.token_scope_count -ne 2 -or
    @($match.matches).Count -ne 2) {
    throw "match: unexpected result $($match | ConvertTo-Json -Compress)"
}
if ($match.matches[1].command_id -ne 9001) {
    throw "match: overflow command ID not preserved"
}
Write-Output 'PASS match counts 2+1=3 and both TokenScope details are reported'

$noncontiguous = Invoke-SelfTest 'noncontiguous'
if (-not $noncontiguous.ok -or @($noncontiguous.matches).Count -ne 1) {
    throw "noncontiguous: unexpected result $($noncontiguous | ConvertTo-Json -Compress)"
}
if ($noncontiguous.matches[0].index -ne 3 -or $noncontiguous.matches[0].command_id -ne 5001) {
    throw "noncontiguous: index/command ID not preserved: $($noncontiguous.matches[0] | ConvertTo-Json -Compress)"
}
Write-Output 'PASS non-contiguous button index and command ID are kept separately'

$timeout = Invoke-SelfTest 'timeout'
if ($timeout.ok -or $null -ne $timeout.visible_count -or $null -ne $timeout.total_count -or
    $null -ne $timeout.token_scope_count -or $timeout.error -notmatch 'timeout') {
    throw "timeout: must be unobservable, not pass: $($timeout | ConvertTo-Json -Compress)"
}
Write-Output 'PASS query timeout makes the area unobservable and counts null'

$mismatch = Invoke-SelfTest 'count-mismatch'
if ($mismatch.ok -or $null -ne $mismatch.token_scope_count -or $mismatch.error -notmatch 'declared') {
    throw "count-mismatch: contradiction must not pass: $($mismatch | ConvertTo-Json -Compress)"
}
Write-Output 'PASS button-count contradiction is rejected'

$overflow = Invoke-SelfTest 'overflow-unobservable'
if ($overflow.ok -or $null -ne $overflow.overflow_count -or $null -ne $overflow.total_count -or
    $null -ne $overflow.token_scope_count -or $overflow.error -notmatch 'Overflow') {
    throw "overflow-unobservable: absent overflow must not be treated as 0: $($overflow | ConvertTo-Json -Compress)"
}
Write-Output 'PASS unobservable overflow is not treated as zero'

$truncated = Invoke-SelfTest 'truncated'
if ($truncated.ok -or $null -ne $truncated.token_scope_count -or $truncated.error -notmatch 'truncated') {
    throw "truncated: truncated text must not pass: $($truncated | ConvertTo-Json -Compress)"
}
Write-Output 'PASS truncated button text is rejected'

Write-Output 'read-tray-notification-area synthetic contracts: all passed'
