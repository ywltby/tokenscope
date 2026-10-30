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

# AC10 真实通知区域协议验证：同一个 ReadButtons 读真实可见+溢出 toolbar。
# 成功路径断言：每个按钮文本长度都在容量内（测长阶段上界 + text-grew 检查由
# ReadButtons 内部强制，这里复核输出）。环境不可读时输出结构化 pending，
# 绝不把失败当通过。
$real = & "$PSScriptRoot/read-tray-notification-area.ps1" -SelfTestReal
if ($LASTEXITCODE -ne 0) { throw "self-test-real exited $LASTEXITCODE" }
$real = $real | ConvertFrom-Json
if ($real.observable) {
    if (-not $real.within_capacity) {
        throw "self-test-real: text length $($real.max_text_length) exceeds capacity $($real.capacity_chars)"
    }
    foreach ($area in @($real.areas)) {
        if (-not $area.ok) { throw "self-test-real: area $($area.area) not ok: $($area.error)" }
        foreach ($b in @($area.buttons)) {
            if ($b.text_length -gt $real.capacity_chars) {
                throw "self-test-real: button $($b.index) text length $($b.text_length) exceeds capacity"
            }
            if ($b.error) { throw "self-test-real: button $($b.index) has error: $($b.error)" }
        }
    }
    Write-Output "PASS real notification area: protocol holds on $($real.areas[0].declared_count)+$($real.areas[1].declared_count) buttons, max text $($real.max_text_length)/$($real.capacity_chars) within capacity"
} else {
    # 不可读环境（CI 变体/精简会话）：必须输出 pending 形态，不得伪造成 ok
    if ($real.within_capacity -ne $false -and $real.max_text_length -ne 0) {
        throw "self-test-real: unobservable environment must not report text lengths"
    }
    Write-Output 'PENDING real notification area unobservable in this environment (structured pending, not a pass)'
}

# AC10 自有受控窗口：本进程真实 ToolbarWindow32 + 真实 ReadButtons 全链路
# （OpenProcess 本进程 / VirtualAllocEx / TB_BUTTONCOUNT / TB_GETBUTTON /
# TB_GETBUTTONTEXTW）。comctl32 普通窗口对自建按钮的文本查询返回 -1（shell
# 任务栏 toolbar 是私有构建），确定性断言是失败路径：按钮计数真实读出、
# 文本查询失败 → 整个区域不可观察。绝不把半可观察结果当成通过。
$window = & "$PSScriptRoot/read-tray-notification-area.ps1" -SelfTestWindow
if ($LASTEXITCODE -ne 0) { throw "self-test-window exited $LASTEXITCODE" }
$window = $window | ConvertFrom-Json
if ($window.declared_count -ne 1) {
    throw "self-test-window: real toolbar button count must be 1, got $($window.declared_count)"
}
if ($window.read_ok) {
    throw "self-test-window: unexpected readable text on controlled toolbar (contract changed? investigate): $($window | ConvertTo-Json -Compress)"
}
if ($window.row_error -notmatch 'TB_GETBUTTONTEXTW') {
    throw "self-test-window: failure must come from the real text query, got: $($window.row_error)"
}
if (@($window.buttons).Count -ne 1 -or $window.buttons[0].command_id -ne 5001) {
    throw "self-test-window: inserted button (idCommand 5001) must be read back through the real cross-process buffer: $($window | ConvertTo-Json -Compress)"
}
Write-Output 'PASS controlled-window: real ToolbarWindow32 read fails closed through the genuine two-phase protocol'

Write-Output 'read-tray-notification-area synthetic contracts: all passed'
