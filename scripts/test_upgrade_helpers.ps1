#Requires -Version 7
# 升级编排契约：外部安装/应用动作均替换；文件边界只用命名临时目录。
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "install_test_helpers.ps1")
. (Join-Path $PSScriptRoot "installer_helpers.ps1")
. (Join-Path $PSScriptRoot "upgrade_test_helpers.ps1")

$script:Trace = [System.Collections.Generic.List[string]]::new()
$script:Assertions = 0
function Check([bool]$Value, [string]$Message) {
    if (-not $Value) { throw "FAIL $Message" }
    $script:Assertions++
    Write-Host "PASS $Message"
}
function Expect-Failure([scriptblock]$Action, [string]$Message) {
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Check $failed $Message
}

$contractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("tokenscope-upgrade-contracts-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $contractRoot | Out-Null
try {
    Expect-Failure { Assert-HostedInstallRunner -Actions "false" -Environment "github-hosted" -TempRoot $contractRoot } "本机执行被拒绝"
    Expect-Failure { Assert-HostedInstallRunner -Actions "true" -Environment "self-hosted" -TempRoot $contractRoot } "持久自托管目录被拒绝"
    Expect-Failure { Assert-HostedInstallRunner -Actions "true" -Environment "github-hosted" -TempRoot "" } "缺少 Runner 临时目录被拒绝"
    Assert-HostedInstallRunner -Actions "true" -Environment "github-hosted" -TempRoot $contractRoot
    Check $true "一次性 Runner 前提通过"

    $probe = Join-Path $contractRoot "参数 probe.py"
    [IO.File]::WriteAllText($probe,@'
import json, sys
sys.stdout.reconfigure(encoding="utf-8")
print(json.dumps({"arguments":sys.argv[1:]},ensure_ascii=False))
sys.exit(37 if "--fail" in sys.argv else 0)
'@,[Text.UTF8Encoding]::new($false))
    $bound = Invoke-UpgradePython -Arguments @($probe,"中文 路径","--value","3")
    Check (($bound.arguments -join ",") -eq "中文 路径,--value,3") "实际 Python 调用保持参数数组与 UTF-8"
    $firstLaunchFacts = Join-Path $contractRoot "first-launch/no-old-logs/facts.json"
    Invoke-UpgradePython -Arguments @($probe,"首次启动") -EvidencePath $firstLaunchFacts | Out-Null
    Check ((Get-Content -LiteralPath $firstLaunchFacts -Raw -Encoding utf8 | ConvertFrom-Json).arguments[0] -eq "首次启动") "首次启动没有旧日志时也能保存事实证据"
    Expect-Failure { Invoke-UpgradePython -Arguments @($probe,"--fail") } "实际 Python 非零退出传递为失败"
    $failedPythonEvidence = Join-Path $contractRoot "failed-facts.json"
    Expect-Failure {
        Invoke-UpgradePython -Arguments @($probe,"--fail","具体字段差异") -EvidencePath $failedPythonEvidence
    } "事实差异保存后仍阻断验收"
    Check ((Test-Path -LiteralPath $failedPythonEvidence) -and
        (Get-Content -LiteralPath $failedPythonEvidence -Raw -Encoding utf8 | ConvertFrom-Json).arguments[-1] -eq "具体字段差异") "失败的 Python 原始 JSON 仍可取证"

    # 调用实际 Launch-AppOnce，仅替换启动动作；立即退出发生在上下文返回前。
    $instantEvidence = Join-Path $contractRoot "instant-exit"
    $instantLogs = Join-Path $contractRoot "instant-live-logs"
    New-Item -ItemType Directory -Path $instantEvidence,$instantLogs | Out-Null
    "本次启动失败日志" | Set-Content -LiteralPath (Join-Path $instantLogs "tokenscope.log") -Encoding utf8NoBOM
    function Assert-NoAliveTokenScope { }
    function Start-Process { param($FilePath,[switch]$PassThru)
        return [pscustomobject]@{ Id=84; HasExited=$true; ExitCode=23 } }
    Expect-Failure {
        Launch-AppOnce -ExePath "inert.exe" -Scenario "old" -RunId "old-1" `
            -EvidenceDir $instantEvidence -LiveLogsDir $instantLogs
    } "进程立即退出仍阻断验收"
    $instantResultPath = Join-Path $instantEvidence "result.json"
    Check ((Test-Path -LiteralPath $instantResultPath) -and
        (Get-Content -LiteralPath $instantResultPath -Raw -Encoding utf8 | ConvertFrom-Json).pid -eq 84) "上下文返回前失败也记录实际 PID"
    Check ((Test-Path -LiteralPath (Join-Path $instantEvidence "live-logs/tokenscope.log")) -and
        -not (Get-Content -LiteralPath $instantResultPath -Raw -Encoding utf8 | ConvertFrom-Json).pass) "启动即退出保留日志和失败结果"
    Remove-Item Function:Start-Process

    $source = Join-Path $contractRoot "tokenscope-upgrade-sources-deleted"
    New-Item -ItemType Directory -Path (Join-Path $source "claude") -Force | Out-Null
    "合成数据" | Set-Content (Join-Path $source "claude/session.jsonl") -Encoding utf8
    Remove-UpgradeFixtureSources -SourcesRoot $source -TempRoot $contractRoot -SourceState "deleted"
    Check (-not (Test-Path (Join-Path $source "claude/session.jsonl"))) "只删除合成来源"
    Check ((Test-Path (Join-Path $source "claude")) -and (Test-Path (Join-Path $source "codex"))) "删除日志后配置来源目录仍存在"
    Expect-Failure { Remove-UpgradeFixtureSources -SourcesRoot $contractRoot -TempRoot $contractRoot -SourceState "deleted" } "越出固定来源目录被拒绝"
    Expect-Failure { Reset-InstallData -DataDir $contractRoot -UserRoot $contractRoot } "越出 .tokenscope 数据目录被拒绝"

    # 实际生命周期函数的调用链；M05 的进程健康条件由既有 26 项契约覆盖。
    function Prepare-LaunchEvidence { param($DataDir,$EvidenceRoot,$Scenario,$RunId)
        $script:Trace.Add("prepare"); return (Join-Path $contractRoot "logs") }
    function Get-UninstallEntry { return [pscustomobject]@{ InstallLocation=$contractRoot; DisplayVersion="0.2.0" } }
    function Launch-AppOnce { param($ExePath,$Scenario,$RunId,$EvidenceDir,$LiveLogsDir)
        $script:Trace.Add("launch")
        return [pscustomobject]@{ Process=[pscustomobject]@{Id=42}; Pid=42; Stage="launched";
            QueryId=$null; Requests=$null; EvidenceDir=$EvidenceDir; LiveLogsDir=$LiveLogsDir } }
    function Wait-CurrentSummary { param($Context,$ExpectedRequests,$TimeoutSeconds)
        $script:Trace.Add("wait"); $Context.QueryId="q-fixture"; $Context.Requests=$ExpectedRequests }
    function Observe-ProcessAlive { param($Context,$DurationSeconds,$PollMilliseconds) $script:Trace.Add("observe") }
    function Close-AppGracefully { param($Context,$TimeoutSeconds) $script:Trace.Add("close"); $Context.Stage="closed" }
    function Save-ScenarioResult { param($Context,$Pass,$Stage,$Detail) $script:Trace.Add("save:$Pass") }
    function Copy-LiveLogsToEvidence { param($Context) $script:Trace.Add("copy") }
    function Stop-ScenarioProcess { param($Context) $script:Trace.Add("stop") }
    $case = [pscustomobject]@{ DataDir=(Join-Path $contractRoot "profile/.tokenscope");
        EvidenceRoot=$contractRoot; SourceState="present"; ExpectedRequests=3 }
    $script:Trace.Clear()
    Invoke-UpgradeLifecycle -Case $case -Phase "current" -Verify {
        param($Value) $script:Trace.Add("verify")
    }
    Check (($script:Trace -join ",") -eq "prepare,launch,wait,verify,observe,close,verify,save:True") "同一次启动贯穿验证、观察、关闭和关闭后复核"
    $script:Trace.Clear()
    Expect-Failure { Invoke-UpgradeLifecycle -Case $case -Phase "current" -Verify { throw "facts differ" } } "事实差异终止生命周期"
    Check (-not $script:Trace.Contains("save:True")) "失败不能记成功"
    Check ($script:Trace.Contains("stop")) "失败清理同一实例"

    # 直接调用事实函数，确认非零输出落在本阶段，不依赖后续成功快照。
    $shadowScripts = Join-Path $contractRoot "shadow/scripts"
    New-Item -ItemType Directory -Path $shadowScripts,$case.DataDir -Force | Out-Null
    $case | Add-Member RepoRoot (Split-Path -Parent $shadowScripts)
    $case | Add-Member ExpectedManifest "inert-expected.json"
    foreach ($name in @("upgrade_fixture.py","inspect_history.py")) {
        [IO.File]::WriteAllText((Join-Path $shadowScripts $name),@'
import json, sys
sys.stdout.reconfigure(encoding="utf-8")
print(json.dumps({"ok":False,"problems":["input_tokens expected=100 actual=99"]}))
sys.exit(1)
'@,[Text.UTF8Encoding]::new($false))
    }
    $factsContext = [pscustomobject]@{ EvidenceDir=$contractRoot; Stage="summary-complete" }
    Expect-Failure { Check-OldUsage -Case $case -Context $factsContext } "旧版字段差异不能回退成功"
    Check ((Get-Content -LiteralPath (Join-Path $contractRoot "old-facts-summary-complete.json") -Raw -Encoding utf8 |
        ConvertFrom-Json).problems[0] -eq "input_tokens expected=100 actual=99") "旧版失败保留逐字段差异"
    Expect-Failure { Check-CurrentUsage -Case $case -Context $factsContext } "新版字段差异不能回退成功"
    Check ((Get-Content -LiteralPath (Join-Path $contractRoot "current-facts-summary-complete.json") -Raw -Encoding utf8 |
        ConvertFrom-Json).problems[0] -eq "input_tokens expected=100 actual=99") "新版失败保留逐字段差异"
    $failedCache = Join-Path $case.DataDir "cache.db"
    [IO.File]::WriteAllBytes($failedCache,[byte[]](1,2,3))
    Save-UpgradeFailureData -Case $case
    Check ((Get-FileHash -LiteralPath $failedCache).Hash -eq
        (Get-FileHash -LiteralPath (Join-Path $contractRoot "failed-data/cache.db")).Hash) "成功旧库快照前失败也保留原库"

    # 实际总编排函数：任何阶段失败均不能进入新版后续启动或发布成功记录。
    $phases = @("Install-UpgradeBaseline","Prepare-OldUsage","Save-OldUpgradeState",
        "Install-UpgradeCurrent","Assert-InstalledUpgrade","Assert-PreservedBeforeLaunch","Grant-UpgradeConsent")
    $failureControl = [pscustomobject]@{ Name="" }
    foreach ($phase in $phases) {
        $name = $phase
        Set-Item -Path ("Function:" + $name) -Value {
            param($Case) $script:Trace.Add($name)
            if ($failureControl.Name -eq $name) { throw "injected $name" }
        }.GetNewClosure()
    }
    function Check-OldUsage { param($Case) }
    function Check-CurrentUsage { param($Case) }
    function Remove-UpgradeSources { param($Case) $script:Trace.Add("delete-source") }
    function Invoke-UpgradeLifecycle { param($Case,$Phase,$Verify)
        $script:Trace.Add("lifecycle:$Phase")
        if ($script:FailAt -eq "lifecycle:$Phase") { throw "injected lifecycle $Phase" } }
    function Save-UpgradeCaseResult { param($Case,$Pass,$Detail) $script:Trace.Add("case:$Pass") }
    foreach ($state in @("present","deleted")) {
        $case.SourceState = $state
        $script:Trace.Clear(); $script:FailAt = ""; $failureControl.Name = ""
        Invoke-UpgradeScenario -Case $case
        $expected = "Install-UpgradeBaseline,Prepare-OldUsage,lifecycle:old,Save-OldUpgradeState,"
        if ($state -eq "deleted") { $expected += "delete-source," }
        $expected += "Install-UpgradeCurrent,Assert-InstalledUpgrade,Assert-PreservedBeforeLaunch,Grant-UpgradeConsent,lifecycle:current,lifecycle:restart,case:True"
        Check (($script:Trace -join ",") -eq $expected) "$state：旧版先产数据，再直接覆盖升级并重启复核"
    }
    foreach ($failure in @($phases) + @("lifecycle:old","lifecycle:current","lifecycle:restart")) {
        $script:Trace.Clear(); $script:FailAt = $failure; $failureControl.Name = $failure
        Expect-Failure { Invoke-UpgradeScenario -Case $case } "$failure 失败不回退成功"
        Check (-not $script:Trace.Contains("case:True")) "$failure 不保存成功结果"
    }
    Write-Host "升级契约 $script:Assertions 项通过"
}
finally {
    $resolved = [System.IO.Path]::GetFullPath($contractRoot)
    $tempBoundary = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempBoundary,[StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path $resolved -Leaf) -notlike "tokenscope-upgrade-contracts-*") { throw "拒绝清理异常测试根" }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
