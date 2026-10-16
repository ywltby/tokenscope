#Requires -Version 7
# 升级编排契约：外部安装/应用动作均替换；文件边界只用命名临时目录。
# 编排本身（登记表条目、按场景切换期望清单、备份规则、链路顺序）用真实实现。
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "install_test_helpers.ps1")
. (Join-Path $PSScriptRoot "installer_helpers.ps1")
. (Join-Path $PSScriptRoot "upgrade_test_helpers.ps1")

$script:Trace = [System.Collections.Generic.List[string]]::new()
$script:Assertions = 0
$script:FailAt = ""
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
# 真实实现先留底：后面替换外部动作时要能回到它们（避免递归替换自己）。
$script:RealPython = ${function:Invoke-UpgradePython}
$script:RealSetJump = ${function:Set-UpgradeJump}
$script:RealCaseResult = ${function:Save-UpgradeCaseResult}
$script:RealJumpResult = ${function:Save-JumpResult}
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

    # 登记表驱动的作业上下文：起点身份、夹具类型、期望清单与请求数都来自登记表。
    $repoRoot = Split-Path -Parent $PSScriptRoot
    $script:InstalledVersion = "0.1.0"
    function Get-UninstallEntry { return [pscustomobject]@{ InstallLocation=$contractRoot; DisplayVersion=$script:InstalledVersion } }
    $dataRoot = Join-Path $contractRoot "profile/.tokenscope"
    $tempRoot = Join-Path $contractRoot "runner-temp"
    New-Item -ItemType Directory -Path $dataRoot,$tempRoot -Force | Out-Null
    "price_auto_sync = false`n" | Set-Content -LiteralPath (Join-Path $dataRoot "settings.toml") -Encoding utf8NoBOM
    "[[model]]`nprefix = ""x""`n" | Set-Content -LiteralPath (Join-Path $dataRoot "pricing.toml") -Encoding utf8NoBOM
    $legacyCase = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
        -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState "present" -Origin "0.1.0" `
        -EvidenceRoot (Join-Path $contractRoot "legacy-evidence") -Metadata ([pscustomobject]@{ executable_sha256=("b"*64) }) `
        -DataDir $dataRoot -TempRoot $tempRoot
    Check ($legacyCase.Profile -eq "legacy-cache") "0.1.0 起点使用旧缓存夹具类型"
    Check ($legacyCase.ExpectedRequests -eq 3) "期望请求数取自登记表缺省清单"
    Check ($legacyCase.ExpectedManifest -like "*upgrade-0.1.0*expected.json") "期望清单缺省指向 0.1.0 清单"
    $historyCase = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
        -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState "deleted" -Origin "0.3.2" `
        -EvidenceRoot (Join-Path $contractRoot "history-evidence") -Metadata ([pscustomobject]@{ executable_sha256=("b"*64) }) `
        -DataDir $dataRoot -TempRoot $tempRoot
    Check ($historyCase.Profile -eq "history") "0.3.2 起点使用历史库夹具类型"
    Check ($historyCase.Baseline.commit -eq "19aae8b91eb61c0de2d0aaeea3016a4fef64e477") "登记提交与标签解引用一致"
    Expect-Failure { New-UpgradeCase -RepoRoot $repoRoot -InstallerPath "x" -AppVersion "0.3.3" `
        -ExpectedCommit ("a"*40) -SourceState "present" -Origin "9.9.9" `
        -EvidenceRoot $contractRoot -Metadata $null } "未登记的起点被拒绝"

    # I4 备份规则：真实函数 + 注入的结构版本读数。
    $script:CurrentSchema = 4
    function Get-HistorySchemaVersion { param($RepoRoot,$Database) return $script:CurrentSchema }
    function Invoke-UpgradePython { param([string[]]$Arguments,[string]$EvidencePath="")
        $script:Trace.Add("python:"+$Arguments[0])
        return [pscustomobject]@{ ok=$true } }
    $backupCase = [pscustomobject]@{ RepoRoot=$repoRoot; DataDir=$dataRoot; EvidenceRoot=$contractRoot
        OldHistorySchema=4; ExpectedOldManifest=(Join-Path $repoRoot "scripts/fixtures/upgrade-0.1.0/expected.json") }
    $result = Assert-HistoryBackup -Case $backupCase -Expect "absent"
    Check ($result.applicable -and $result.expect -eq "absent") "结构未变时备份规则适用且要求不存在"
    "old" | Set-Content -LiteralPath (Join-Path $dataRoot "history.db.backup-v4") -Encoding utf8NoBOM
    Expect-Failure { Assert-HistoryBackup -Case $backupCase -Expect "absent" } "结构未升级却出现备份被拒绝"
    $script:CurrentSchema = 5
    $result = Assert-HistoryBackup -Case $backupCase
    Check ($result.expect -eq "created") "结构升级时自动要求备份"
    Check ($result.exists -and $backupCase.BackupName -eq "history.db.backup-v4") "结构升级的备份被登记为证据"
    Remove-Item -LiteralPath (Join-Path $dataRoot "history.db.backup-v4")
    Expect-Failure { Assert-HistoryBackup -Case $backupCase } "结构升级却没有备份被拒绝"
    $script:CurrentSchema = 4
    $noHistory = [pscustomobject]@{ RepoRoot=$repoRoot; DataDir=$dataRoot; EvidenceRoot=$contractRoot
        OldHistorySchema=$null; ExpectedOldManifest="unused" }
    Check (-not (Assert-HistoryBackup -Case $noHistory).applicable) "没有旧历史库时备份规则不适用"

    # 编排顺序：真实 Invoke-UpgradeScenario/Invoke-UpgradeChain，只替换外部动作。
    function Invoke-UpgradePython { param([string[]]$Arguments,[string]$EvidencePath="")
        if ($Arguments -contains "download-baseline") {
            $script:Trace.Add("download:"+$Arguments[$Arguments.IndexOf("--origin")+1])
            return [pscustomobject]@{ installer=(Join-Path $contractRoot "old-setup.exe"); installer_sha256=("c"*64) }
        }
        return (& $script:RealPython -Arguments $Arguments -EvidencePath $EvidencePath) }
    function Set-UpgradeJump { param($Case,[string]$Version)
        $script:Trace.Add("jump:"+$Version)
        return (& $script:RealSetJump -Case $Case -Version $Version) }
    function Install-UpgradeBaseline { param($Case) $script:Trace.Add("install-baseline")
        if ($script:FailAt -eq "install-baseline") { throw "injected install-baseline" } }
    function Prepare-OldUsage { param($Case) $script:Trace.Add("prepare-old") }
    function Invoke-UpgradeLifecycle { param($Case,$Phase,$Verify)
        $script:Trace.Add("lifecycle:"+$Phase)
        if ($script:FailAt -eq ("lifecycle:"+$Phase)) { throw "injected lifecycle $Phase" } }
    function Save-OldUpgradeState { param($Case) $script:Trace.Add("save-old") }
    function Install-UpgradeCurrent { param($Case) $script:Trace.Add("install-current") }
    function Assert-PreservedBeforeLaunch { param($Case) $script:Trace.Add("preserved") }
    function Assert-InstalledUpgrade { param($Case) $script:Trace.Add("installed") }
    function Grant-UpgradeConsent { param($Case) $script:Trace.Add("consent") }
    function Install-UpgradePackage { param($Case,$Origin,$InstallerPath) $script:Trace.Add("install:"+$Origin)
        if ($script:FailAt -eq ("install:"+$Origin)) { throw "injected install $Origin" } }
    function Save-UpgradeCaseResult { param($Case,$Pass,$Detail)
        $script:Trace.Add("case:$Pass")
        & $script:RealCaseResult -Case $Case -Pass $Pass -Detail $Detail }
    function Save-UpgradeFailureData { param($Case) $script:Trace.Add("failure-data") }
    function Save-JumpResult { param($Case,$Phase,$Pass,$Detail="")
        $script:Trace.Add("jump:${Phase}:${Pass}")
        & $script:RealJumpResult -Case $Case -Phase $Phase -Pass $Pass -Detail $Detail }
    function Remove-UpgradeSources { param($Case) $script:Trace.Add("delete-source") }
    function Assert-NoAliveTokenScope { }
    function Test-InstalledExecutable { param($Case) return ("d"*64) }

    foreach ($state in @("present","deleted")) {
        $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
            -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState $state -Origin "0.1.0" `
            -EvidenceRoot (Join-Path $contractRoot ("scenario-"+$state)) `
            -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="1" }) }) `
            -DataDir $dataRoot -TempRoot $tempRoot
        $script:Trace.Clear(); $script:FailAt = ""
        Invoke-UpgradeScenario -Case $case
        $expected = "jump:0.1.0,install-baseline,prepare-old,lifecycle:old,save-old,"
        if ($state -eq "deleted") { $expected += "delete-source," }
        $expected += "install-current,preserved,consent,jump:0.3.3,lifecycle:current,lifecycle:restart,jump:current:True,case:True"
        Check (($script:Trace -join ",") -eq $expected) "$state：旧版先产数据，再直接覆盖升级并重启复核"
    }
    foreach ($failure in @("install-baseline","lifecycle:old","lifecycle:current","lifecycle:restart")) {
        $script:Trace.Clear(); $script:FailAt = $failure
        $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
            -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState "present" -Origin "0.3.2" `
            -EvidenceRoot (Join-Path $contractRoot "failure") `
            -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="1" }) }) `
            -DataDir $dataRoot -TempRoot $tempRoot
        Expect-Failure { Invoke-UpgradeScenario -Case $case } "$failure 失败不回退成功"
        Check (-not $script:Trace.Contains("case:True")) "$failure 不保存成功结果"
    }

    # 连续升级链：起点集合按版本升序逐跳，只播种一次，失败即中止后续跳。
    foreach ($state in @("present","deleted")) {
        $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
            -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState $state -Origin "0.1.0" `
            -EvidenceRoot (Join-Path $contractRoot ("chain-"+$state)) `
            -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="1" }) }) `
            -DataDir $dataRoot -TempRoot $tempRoot
        $script:Trace.Clear(); $script:FailAt = ""
        Invoke-UpgradeChain -Case $case
        $expected = "jump:0.1.0,install-baseline,prepare-old,lifecycle:chain-00-old,save-old,jump:chain-00-old:True,"
        if ($state -eq "deleted") { $expected += "delete-source," }
        $expected += "jump:0.3.2,download:0.3.2,install:0.3.2,consent,"
        $expected += "lifecycle:chain-01,jump:chain-01:True,"
        $expected += "jump:0.3.3,install:0.3.3,installed,consent,lifecycle:chain-02,jump:chain-02:True,"
        $expected += "lifecycle:restart,jump:restart:True,case:True"
        Check (($script:Trace -join ",") -eq $expected) "$state：链路逐跳覆盖安装并只播种一次"
        Check ((@($case.Chain) -join "->") -eq "0.1.0->0.3.2->0.3.3") "链路记录起点集合与当前版本"
        Check ((@($script:Trace | Where-Object { $_ -like "prepare-old*" }).Count) -eq 1) "$state：链路只播种一次来源日志"
    }
    $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
        -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState "present" -Origin "0.1.0" `
        -EvidenceRoot (Join-Path $contractRoot "chain-failure") `
        -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="1" }) }) `
        -DataDir $dataRoot -TempRoot $tempRoot
    $script:Trace.Clear(); $script:FailAt = "install:0.3.2"
    Expect-Failure { Invoke-UpgradeChain -Case $case } "链中某一跳失败即中止"
    Check (-not ($script:Trace -join ",").Contains("chain-01")) "失败跳之后的核对不再执行"
    Check (-not $script:Trace.Contains("case:True")) "链路失败不保存成功结果"
    $script:FailAt = ""
    $result = Get-Content -LiteralPath (Join-Path $case.EvidenceRoot "upgrade-result.json") -Raw -Encoding utf8 | ConvertFrom-Json
    Check (($result.pass -eq $false) -and (@($result.chain) -join "->" -eq "0.1.0->0.3.2->0.3.3")) "链路失败写出逐跳结果"
    Check (($result.jumps | Where-Object { -not $_.pass } | Select-Object -First 1).phase -eq "failed") "链路失败记录失败阶段"

    Write-Host "升级契约 $script:Assertions 项通过"
}
finally {
    $resolved = [System.IO.Path]::GetFullPath($contractRoot)
    $tempBoundary = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempBoundary,[StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path $resolved -Leaf) -notlike "tokenscope-upgrade-contracts-*") { throw "拒绝清理异常测试根" }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
