#Requires -Version 7
# M05 生命周期契约测试：用受控进程替身 + 临时目录验证
# scripts/install_test_helpers.ps1 的等待/观察/关闭判定。
#
# 反例先于实现（先红后绿）：旧日志借用、快启动、仅写库、提前退出 0/23、
# 关闭失败/超时/退出非 0 全部必须失败；单实例正例必须记录同一 PID 与
# 本次 query_id。全部使用临时目录，不触碰默认用户目录与注册表。

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "install_test_helpers.ps1")

$Failures = New-Object System.Collections.Generic.List[string]
$Checks = 0

function Test-True {
    param([bool]$Condition, [string]$Message)
    $script:Checks++
    if ($Condition) {
        Write-Host "PASS $Message"
    }
    else {
        $script:Failures.Add($Message) | Out-Null
        Write-Host "FAIL $Message"
    }
}

function Test-Throws {
    param([scriptblock]$Action, [string]$Message, [string]$ExpectedText = "")
    $script:Checks++
    try {
        & $Action
        Write-Host "FAIL $Message（未抛出）"
        $script:Failures.Add($Message) | Out-Null
    }
    catch {
        if ($ExpectedText -ne "" -and -not ($_.Exception.Message -like "*$ExpectedText*")) {
            Write-Host "FAIL $Message（抛出但信息不符：$($_.Exception.Message)）"
            $script:Failures.Add("$Message（信息不符）") | Out-Null
        }
        else {
            Write-Host "PASS $Message"
        }
    }
}

function New-Scenario {
    param([string]$Tag)
    $root = Join-Path ([System.IO.Path]::GetTempPath()) "tokenscope-helper-test-$Tag-$([System.Guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path (Join-Path $root "logs") -Force | Out-Null
    return [pscustomobject]@{
        Root    = $root
        LogsDir = Join-Path $root "logs"
        Context = [pscustomobject]@{
            Process     = $null
            Pid         = $null
            ExePath     = "stand-in"
            Scenario    = $Tag
            RunId       = "run-$Tag"
            EvidenceDir = $root
            LiveLogsDir = Join-Path $root "logs"
            QueryId     = $null
            Requests    = $null
            Stage       = "launched"
        }
    }
}

function Write-LogLine {
    param([string]$LogsDir, [string]$FileName, [string]$Line, [bool]$NoNewline = $false)
    if ($NoNewline) {
        Add-Content -Path (Join-Path $LogsDir $FileName) -Value $Line -NoNewline -Encoding utf8NoBOM
    }
    else {
        Add-Content -Path (Join-Path $LogsDir $FileName) -Value $Line -Encoding utf8NoBOM
    }
}

function New-GuiStandInProcess {
    # GUI 替身：notepad 有真实主窗口，CloseMainWindow（WM_CLOSE）→
    # 干净退出、退出码 0——与真实 GUI 应用（close_action = "quit"）的
    # 关闭语义一致。pwsh 是控制台宿主：关窗即 0xC000013A，不能作为
    # "优雅退出 0"的替身。
    return Start-Process -FilePath "notepad.exe" -PassThru
}

function New-StandInProcess {
    # 存活替身：一个安静存活 60 秒的 pwsh 子进程（等待期/观察期用，
    # 不涉及 CloseMainWindow）。
    Start-Process -FilePath "pwsh" -ArgumentList "-NoProfile", "-Command", "Start-Sleep -Seconds 60" -PassThru
}

function New-ExitProcess {
    param([int]$Code)
    Start-Process -FilePath "pwsh" -ArgumentList "-NoProfile", "-Command", "exit $Code" -PassThru
}

function Cleanup {
    param($Scenario)
    Stop-ScenarioProcess -Context $Scenario.Context
    if (Test-Path $Scenario.Root) {
        Remove-Item $Scenario.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$SummaryOk = "汇总完成（会话 q-alpha-1）：by=day 组=3 请求=3，12 ms"

# ---------- 反例 1：旧日志借用 ----------

Write-Host "=== 反例：旧日志已有成功汇总，新实例无新日志 ==="
$s = New-Scenario "old-log-borrow"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log.2026-10-10" -Line $SummaryOk
$deadline = 2
Test-Throws {
    Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds $deadline
} "旧日志借用必须等待超时失败（归档后的新日志没有汇总）"
Cleanup $s

# ---------- 反例 2：仅写"历史库写入"没有"汇总完成" ----------

Write-Host "=== 反例：仅写历史库写入 ==="
$s = New-Scenario "write-only"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line "历史库写入：新增 3，更新 0，已存在 0，冲突 0，陈旧 0；别名 +0，来源 +0"
Test-Throws {
    Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
} "仅写库没有汇总完成必须失败"
Cleanup $s

# ---------- 反例 3：本次汇总请求数 0 或 2 ----------

Write-Host "=== 反例：汇总请求数 0 / 2 ==="
foreach ($wrong in 0, 2) {
    $s = New-Scenario "wrong-count-$wrong"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line (
        "汇总完成（会话 q-wrong）：by=day 组=1 请求=$wrong，5 ms"
    )
    Test-Throws {
        Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
    } "汇总请求数 $wrong != 3 必须失败"
    Cleanup $s
}

# ---------- 反例 4：等待期提前退出（退出码 0 与 23） ----------

Write-Host "=== 反例：等待期提前退出 0 / 23 ==="
foreach ($code in 0, 23) {
    $s = New-Scenario "early-exit-$code"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line "价格加载：重建，3 ms"
    $proc = New-ExitProcess -Code $code
    $s.Context.Process = $proc
    $s.Context.Pid = $proc.Id
    Test-Throws {
        Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 10
    } "等待期提前退出（退出码 $code）必须失败"
    Cleanup $s
}

# ---------- 反例 5：观察期提前退出 ----------

Write-Host "=== 反例：观察期提前退出 ==="
foreach ($code in 0, 23) {
    $s = New-Scenario "observe-exit-$code"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk
    $proc = New-ExitProcess -Code $code
    $s.Context.Process = $proc
    $s.Context.Pid = $proc.Id
    $s.Context.QueryId = "q-alpha-1"
    Test-Throws {
        Observe-ProcessAlive -Context $s.Context -DurationSeconds 1 -PollMilliseconds 100
    } "观察期提前退出（退出码 $code）必须失败"
    Cleanup $s
}

# ---------- 反例 6：关闭失败 / 超时 / 退出非 0 ----------

Write-Host "=== 反例：关闭语义 ==="
function New-FakeContext {
    param([bool]$CloseResult, [bool]$WaitResult, $ExitCode)
    $proc = [pscustomobject]@{ HasExited = $false; ExitCode = $ExitCode }
    $proc | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { return $script:CloseResult }.GetNewClosure()
    $proc | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param([int]$ms) return $script:WaitResult }.GetNewClosure()
    return [pscustomobject]@{ Process = $proc; Pid = 0; ExePath = "fake"; Scenario = "fake"; RunId = "fake"; EvidenceDir = $null; QueryId = $null; Requests = $null; Stage = "launched" }
}
Test-Throws { Close-AppGracefully -Context (New-FakeContext -CloseResult $false -WaitResult $true -ExitCode $null) } "CloseMainWindow=false 必须失败" "CloseMainWindow"
Test-Throws { Close-AppGracefully -Context (New-FakeContext -CloseResult $true -WaitResult $false -ExitCode $null) } "关闭等待超时必须失败" "30s"
Test-Throws { Close-AppGracefully -Context (New-FakeContext -CloseResult $true -WaitResult $true -ExitCode 3) } "退出码非 0 必须失败" "退出码 3"

# ---------- 反例 7：半行日志（UTF-8/写入边界）不算完整记录 ----------

Write-Host "=== 反例：半行日志不算完成 ==="
$s = New-Scenario "partial-line"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk -NoNewline $true
Test-Throws {
    Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
} "无换行的半行汇总不得参与匹配"
Cleanup $s

# ---------- 正例 1：快启动完整汇总并存活 ----------

Write-Host "=== 正例：快启动完整汇总 ==="
$s = New-Scenario "fast-start"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
try {
    $summary = Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 10
    Test-True ($summary.QueryId -eq "q-alpha-1") "记录本次 query_id"
    Test-True ($summary.Requests -eq 3) "记录本次请求数"
    Test-True ($s.Context.Pid -eq $proc.Id) "全程同一进程句柄"
    Observe-ProcessAlive -Context $s.Context -DurationSeconds 1 -PollMilliseconds 100
    Test-True $true "观察期存活"
}
finally {
    Cleanup $s
}

# ---------- 正例 2：分多次写完 + 跨文件（跨日） ----------

Write-Host "=== 正例：分次写入与跨文件 ==="
$s = New-Scenario "split-write"
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
try {
    # 先写半行（无换行），再在第二个文件里补完整行——半行不匹配，
    # 完整行匹配后成功。
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk -NoNewline $true
    Test-Throws {
        Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
    } "半行阶段必须继续等待"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log.1" -Line $SummaryOk
    $summary = Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 10
    Test-True ($summary.QueryId -eq "q-alpha-1") "跨文件完整行匹配"
}
finally {
    Cleanup $s
}

# ---------- 正例 3：完整生命周期（单实例 + 观察 + 优雅关闭） ----------

Write-Host "=== 正例：完整生命周期 ==="
$s = New-Scenario "full-lifecycle"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk
$proc = New-GuiStandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
try {
    $summary = Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 15
    Observe-ProcessAlive -Context $s.Context -DurationSeconds 1 -PollMilliseconds 100
    Close-AppGracefully -Context $s.Context
    Test-True ($s.Context.Stage -eq "closed") "生命周期到 closed"
    Test-True ($s.Context.Pid -eq $proc.Id) "PID 全程一致"
    Test-True ($s.Context.QueryId -eq "q-alpha-1") "query_id 全程一致"
}
catch {
    Stop-ScenarioProcess -Context $s.Context
    throw
}
finally {
    if (Test-Path $s.Root) {
        Remove-Item $s.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "契约断言通过 $Checks 项，失败 $($Failures.Count) 项"
if ($Failures.Count -gt 0) {
    foreach ($f in $Failures) {
        Write-Host "  - $f"
    }
    exit 1
}
exit 0
