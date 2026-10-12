#Requires -Version 7
# N12/M05：安装验收的生命周期函数（可测形态）。
# 本文件**只定义函数**——不含安装动作、注册表操作、默认用户目录写入或
# 顶层执行；进程句柄与日志目录一律由调用方传入，便于用替身进程做契约
# 测试（scripts/test_install_helpers.ps1）。判定只认本次启动的证据。

function Assert-HelperTrue {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Get-AliveTokenScopeProcesses {
    # K-P3-3：进程名 = cargo bin 名 tokenscope（Get-Process 本就不分大小写）。
    Get-Process -Name "tokenscope" -ErrorAction SilentlyContinue
}

function Assert-NoAliveTokenScope {
    $alive = Get-AliveTokenScopeProcesses
    Assert-HelperTrue ($null -eq $alive -or @($alive).Count -eq 0) (
        "存在存活的 TokenScope 进程（PID $(@($alive)[0].Id)），启动前必须清理"
    )
}

function Move-LaunchLogsToEvidence {
    # 把数据目录下的 logs 整体移入本次证据目录（Move-Item -LiteralPath）。
    # 校验：源必须是数据目录内的 logs，目的地必须在证据根内。
    param(
        [Parameter(Mandatory = $true)][string]$LogsDir,
        [Parameter(Mandatory = $true)][string]$EvidenceScenarioDir
    )
    $resolvedSource = [System.IO.Path]::GetFullPath($LogsDir)
    $resolvedDest = [System.IO.Path]::GetFullPath($EvidenceScenarioDir)
    Assert-HelperTrue ($resolvedSource -like "*\.tokenscope\logs") (
        "归档源必须是数据目录内的 logs：$resolvedSource"
    )
    Assert-HelperTrue ($resolvedDest -like "*tokenscope-install-evidence*") (
        "证据目的地必须在证据根内：$resolvedDest"
    )
    if (Test-Path -LiteralPath $resolvedSource) {
        New-Item -ItemType Directory -Path $resolvedDest -Force | Out-Null
        Move-Item -LiteralPath $resolvedSource -Destination $resolvedDest -Force
    }
}

function Prepare-LaunchEvidence {
    # 启动前的证据准备：确认没有存活实例 → 归档旧日志 → 返回证据目录。
    # 归档失败或仍有存活实例都会中止（不带着旧证据/旧进程启动）。
    param(
        [Parameter(Mandatory = $true)][string]$DataDir,
        [Parameter(Mandatory = $true)][string]$EvidenceRoot,
        [Parameter(Mandatory = $true)][string]$Scenario,
        [Parameter(Mandatory = $true)][string]$RunId
    )
    Assert-NoAliveTokenScope
    $logsDir = Join-Path $DataDir "logs"
    $scenarioDir = Join-Path (Join-Path $EvidenceRoot $Scenario) $RunId
    Move-LaunchLogsToEvidence -LogsDir $logsDir -EvidenceScenarioDir $scenarioDir
    New-Item -ItemType Directory -Path $scenarioDir -Force | Out-Null
    return (Join-Path $scenarioDir "logs")
}

function Launch-AppOnce {
    # 启动唯一实例并记录上下文。RUST_LOG=info 只在子进程启动期间设置，
    # 启动后立即恢复调用者环境（子进程已继承自己的副本）。
    # LiveLogsDir = 应用**实际写入**的日志目录（DataDir/logs）——等待函数
    # 只读这里；EvidenceDir 是归档/结果落盘处（复核：二者不能接反）。
    param(
        [Parameter(Mandatory = $true)][string]$ExePath,
        [Parameter(Mandatory = $true)][string]$Scenario,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$EvidenceDir,
        [Parameter(Mandatory = $true)][string]$LiveLogsDir
    )
    Assert-NoAliveTokenScope
    $previousRustLog = $env:RUST_LOG
    try {
        $env:RUST_LOG = "info"
        $process = Start-Process -FilePath $ExePath -PassThru
    }
    finally {
        if ($null -ne $previousRustLog) {
            $env:RUST_LOG = $previousRustLog
        }
        else {
            Remove-Item Env:RUST_LOG -ErrorAction SilentlyContinue
        }
    }
    Assert-HelperTrue (-not $process.HasExited) "启动后进程立即退出（退出码 $($process.ExitCode)）"
    return [pscustomobject]@{
        Process     = $process
        Pid         = $process.Id
        ExePath     = $ExePath
        Scenario    = $Scenario
        RunId       = $RunId
        EvidenceDir = $EvidenceDir
        LiveLogsDir = $LiveLogsDir
        QueryId     = $null
        Requests    = $null
        Stage       = "launched"
    }
}

function Copy-LiveLogsToEvidence {
    # 应用退出后把**本次写入**的日志复制进证据目录——归档的是旧日志，
    # 上传的证据必须同时包含本次新增日志（供复核确认"汇总完成"来自
    # 本次实例）。
    param([Parameter(Mandatory = $true)]$Context)
    if ($null -eq $Context.LiveLogsDir -or -not (Test-Path $Context.LiveLogsDir)) {
        return
    }
    $dest = Join-Path $Context.EvidenceDir "live-logs"
    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    Copy-Item -Path (Join-Path $Context.LiveLogsDir "*") -Destination $dest -Recurse -Force -ErrorAction SilentlyContinue
}

function Get-CompleteLogLines {
    # 读取当前 logs 下**全部文件的完整行**：支持跨日产生多个新日志文件；
    # 不以换行结尾的末段（正在写入的半行）不参与匹配。
    param([Parameter(Mandatory = $true)][string]$LogsDir)
    $lines = New-Object System.Collections.Generic.List[string]
    if (-not (Test-Path $LogsDir)) {
        return $lines
    }
    foreach ($logFile in Get-ChildItem $LogsDir -File -ErrorAction SilentlyContinue) {
        $stream = $null
        $reader = $null
        try {
            # 应用的日志 writer 持续打开。Windows 上 ReadAllText 默认仅共享
            # 读取，会和既有写入句柄冲突；读取句柄仍只读，允许共享写入/轮转。
            $stream = [System.IO.File]::Open($logFile.FullName,
                [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read,
                [System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete)
            $reader = [System.IO.StreamReader]::new($stream, [System.Text.Encoding]::UTF8)
            $text = $reader.ReadToEnd()
        }
        catch [System.IO.IOException] {
            continue  # 文件在枚举后轮转/删除等：下一轮轮询再读。
        }
        finally {
            if ($null -ne $reader) { $reader.Dispose() }
            if ($null -ne $stream) { $stream.Dispose() }
        }
        if ($text -eq "") {
            continue
        }
        $parts = $text -split "`r?`n"
        $end = $parts.Count
        if ($text -notmatch "`r?`n$") {
            $end = $parts.Count - 1  # 末段无换行 = 半行，丢弃
        }
        for ($i = 0; $i -lt $end; $i++) {
            if ($parts[$i] -ne "") {
                $lines.Add($parts[$i])
            }
        }
    }
    return $lines
}

function Wait-CurrentSummary {
    # M05 核心：等待**本次启动日志**中的首条成功"汇总完成"记录。
    # - 每轮先检查同一进程仍存活（提前退出一律失败，含退出码 0）；
    # - 只解析本次 logs 的完整日志行（旧日志已在启动前归档）；
    # - 首条成功汇总的请求数 != 期望 → 失败（外部 SQL 读数不能替代）；
    # - "历史库写入"、WAL 存在与时间变化只作诊断，不参与成功判定。
    # 返回 query_id 与 requests。
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][int]$ExpectedRequests,
        [int]$TimeoutSeconds = 180
    )
    Assert-HelperTrue ($null -ne $Context -and $null -ne $Context.Process) "上下文缺少进程句柄"
    Assert-HelperTrue ($null -ne $Context.LiveLogsDir) "上下文缺少 LiveLogsDir（应用实际写入的日志目录）"
    $logsDir = $Context.LiveLogsDir
    $pattern = '汇总完成（会话 (?<qid>[^）]+)）：by=\S+ 组=\d+ 请求=(?<requests>\d+)'
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    while ($stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        Require-ProcessAlive -Context $Context
        foreach ($line in Get-CompleteLogLines -LogsDir $logsDir) {
            if ($line -match $pattern) {
                $queryId = $Matches["qid"]
                $requests = [int]$Matches["requests"]
                if ($requests -ne $ExpectedRequests) {
                    throw "本次汇总请求数 $requests != 期望 $ExpectedRequests（query_id=$queryId）"
                }
                $Context.QueryId = $queryId
                $Context.Requests = $requests
                $Context.Stage = "summary-complete"
                Require-ProcessAlive -Context $Context
                return [pscustomobject]@{ QueryId = $queryId; Requests = $requests }
            }
        }
        Start-Sleep -Milliseconds 200
    }
    throw "等待'汇总完成'超时（${TimeoutSeconds}s）：本次启动日志中没有请求数为 $ExpectedRequests 的成功汇总"
}

function Require-ProcessAlive {
    # 同一进程句柄必须贯穿等待与观察期：提前退出一律失败（含退出码 0）。
    param([Parameter(Mandatory = $true)]$Context)
    Assert-HelperTrue ($null -ne $Context -and $null -ne $Context.Process) "上下文缺少进程句柄"
    if ($Context.Process.HasExited) {
        $code = $null
        try { $code = $Context.Process.ExitCode } catch { }
        throw "进程在场景进行中退出（PID $($Context.Pid)，退出码 $code）——提前退出包括退出码 0 一律失败"
    }
}

function Observe-ProcessAlive {
    # 观察期：同一句柄在 8 秒内保持存活（200ms 轮询），期间退出即失败。
    param(
        [Parameter(Mandatory = $true)]$Context,
        [double]$DurationSeconds = 8,
        [double]$PollMilliseconds = 200
    )
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    while ($stopwatch.Elapsed.TotalSeconds -lt $DurationSeconds) {
        Require-ProcessAlive -Context $Context
        Start-Sleep -Milliseconds $PollMilliseconds
    }
    Require-ProcessAlive -Context $Context
}

function Close-AppGracefully {
    # M05 关闭语义：CloseMainWindow 必须成功；30 秒内退出；退出码 0。
    # 超时后的强制结束只用于清理，不能把失败补成通过。
    param(
        [Parameter(Mandatory = $true)]$Context,
        [int]$TimeoutSeconds = 30
    )
    Require-ProcessAlive -Context $Context
    $requested = $Context.Process.CloseMainWindow()
    if (-not $requested) {
        Stop-ScenarioProcess -Context $Context
        throw "CloseMainWindow 返回 false（关闭请求未成功）"
    }
    if (-not $Context.Process.WaitForExit($TimeoutSeconds * 1000)) {
        Stop-ScenarioProcess -Context $Context
        throw "应用未在 ${TimeoutSeconds}s 内退出（强制结束仅用于清理，不改为通过）"
    }
    Assert-HelperTrue ($Context.Process.ExitCode -eq 0) (
        "优雅关闭退出码 $($Context.Process.ExitCode) != 0"
    )
    Copy-LiveLogsToEvidence -Context $Context
    $Context.Stage = "closed"
}

function Stop-ScenarioProcess {
    param([Parameter(Mandatory = $true)]$Context)
    try {
        if ($null -ne $Context.Process -and -not $Context.Process.HasExited) {
            Stop-Process -Id $Context.Process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    catch {
        # 清理失败不能掩盖原始失败原因。
    }
}

function Save-ScenarioResult {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][bool]$Pass,
        [string]$Stage = "",
        [string]$Detail = ""
    )
    if ($null -ne $Context -and $null -ne $Context.EvidenceDir) {
        $payload = [pscustomobject]@{
            scenario = $Context.Scenario
            run_id   = $Context.RunId
            pid      = $Context.Pid
            exe      = $Context.ExePath
            query_id = $Context.QueryId
            requests = $Context.Requests
            stage    = $Stage
            pass     = $Pass
            detail   = $Detail
        }
        $path = Join-Path $Context.EvidenceDir "result.json"
        $payload | ConvertTo-Json -Depth 4 | Set-Content -Path $path -Encoding utf8NoBOM
    }
}
