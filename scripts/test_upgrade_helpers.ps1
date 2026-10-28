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

function New-HistoryDb {
    # 按期望清单造最小历史库：只用于契约测试，结构版本与事实都与真库同口径。
    param([string]$Path,[string]$Manifest)
    $script = Join-Path $contractRoot "make-history.py"
    [IO.File]::WriteAllText($script, @'
import json, sqlite3, sys
manifest, target = sys.argv[1], sys.argv[2]
events = json.load(open(manifest, encoding="utf-8"))["events"]
con = sqlite3.connect(target)
con.execute("PRAGMA user_version=4")
con.execute("CREATE TABLE usage_events(app TEXT, session_id TEXT, record_id TEXT, model_raw TEXT, "
            "project_key TEXT, session_initial_cwd TEXT, event_cwd TEXT, input_tokens TEXT, "
            "output_tokens TEXT, cache_write_tokens TEXT, cache_read_tokens TEXT)")
for e in events:
    con.execute("INSERT INTO usage_events VALUES(?,?,?,?,?,?,?,?,?,?,?)",
                (e["app"], e["session_id"], e["record_id"], e["model_raw"], e["project_key"],
                 e["session_initial_cwd"], e["event_cwd"], str(e["input_tokens"]),
                 str(e["output_tokens"]), str(e["cache_write_tokens"]), str(e["cache_read_tokens"])))
con.commit()
con.close()
print("ok")
'@, [Text.UTF8Encoding]::new($false))
    & python -B $script $Manifest $Path | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "造历史库失败" }
}

$contractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("tokenscope-upgrade-contracts-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $contractRoot | Out-Null
# 真实实现先留底：后面替换外部动作时要能回到它们（避免递归替换自己）。
$script:RealPython = ${function:Invoke-UpgradePython}
$script:RealSetJump = ${function:Set-UpgradeJump}
$script:RealCaseResult = ${function:Save-UpgradeCaseResult}
$script:RealJumpResult = ${function:Save-JumpResult}
$script:RealConsentGate = ${function:Invoke-ConsentGateStage}
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

    # 真实 Case 构造 + 真实旧事实核对：history 起点必须能从登记表拿到旧期望清单。
    Check ($historyCase.ExpectedOldManifest -eq $legacyCase.ExpectedOldManifest) "history 起点的旧期望清单来自登记表"
    Check ($historyCase.BackupsBefore.Count -eq 0) "作业上下文自带升级前备份清单"
    $historyData = Join-Path $contractRoot "history-case/.tokenscope"
    New-Item -ItemType Directory -Path $historyData -Force | Out-Null
    New-HistoryDb -Path (Join-Path $historyData "history.db") -Manifest $historyCase.ExpectedOldManifest
    $historyCase.DataDir = $historyData
    Check ((Get-HistorySchemaVersion -RepoRoot $repoRoot -Database (Join-Path $historyData "history.db")) -eq 4) "旧历史库结构版本读数"
    Check-OldUsage -Case $historyCase
    Check ((Get-Content -LiteralPath (Join-Path $historyCase.EvidenceRoot "old-facts.json") -Raw -Encoding utf8 |
        ConvertFrom-Json).ok) "真实 Case 与真实旧事实核对通过"

    # 备份规则按本跳实际的旧库状态判定，与起点夹具类型无关：0.1.0 起点的链路里
    # 也会出现历史库，Profile=legacy-cache 时同样必须核对（按 profile 判断会跳过）。
    $backupData = Join-Path $contractRoot "backup-case/.tokenscope"
    New-Item -ItemType Directory -Path $backupData -Force | Out-Null
    New-HistoryDb -Path (Join-Path $backupData "history.db") -Manifest $historyCase.ExpectedOldManifest
    "privacy_policy_accepted = true`nprice_auto_sync = false`nclose_action = ""quit""`n" |
        Set-Content -LiteralPath (Join-Path $backupData "settings.toml") -Encoding utf8NoBOM
    "[[model]]`nprefix = ""x""`n" | Set-Content -LiteralPath (Join-Path $backupData "pricing.toml") -Encoding utf8NoBOM
    "旧缓存字节" | Set-Content -LiteralPath (Join-Path $backupData "cache.db") -Encoding utf8NoBOM
    $guardCase = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
        -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState "present" -Origin "0.1.0" `
        -EvidenceRoot (Join-Path $contractRoot "backup-guard") `
        -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="1" }) }) `
        -DataDir $backupData -TempRoot $tempRoot
    New-Item -ItemType Directory -Path $guardCase.EvidenceRoot -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $backupData "settings.toml") -Destination (Join-Path $guardCase.EvidenceRoot "old-settings.toml")
    $guardCase.OldSettingsPath = Join-Path $guardCase.EvidenceRoot "old-settings.toml"
    $guardCase.CacheHash = (Get-FileHash -LiteralPath (Join-Path $backupData "cache.db") -Algorithm SHA256).Hash.ToLowerInvariant()
    $guardCase.PricingHash = (Get-FileHash -LiteralPath (Join-Path $backupData "pricing.toml") -Algorithm SHA256).Hash.ToLowerInvariant()
    $guardCase.OldHistorySchema = 4
    $guardCase.BackupsBefore = @()
    "非法备份" | Set-Content -LiteralPath (Join-Path $backupData "history.db.backup-v4") -Encoding utf8NoBOM
    Expect-Failure { Check-CurrentUsage -Case $guardCase } "legacy 起点的链路里也会核对历史备份"
    Remove-Item -LiteralPath (Join-Path $backupData "history.db.backup-v4")
    Check-CurrentUsage -Case $guardCase
    Check (Test-Path -LiteralPath (Join-Path $guardCase.EvidenceRoot "history-backup.json")) "备份判定写出证据"

    # I4 备份清单：结构升级不得覆盖已有备份，结构未变不得改动备份。
    $script:CurrentSchema = 4
    function Get-HistorySchemaVersion { param($RepoRoot,$Database) return $script:CurrentSchema }
    function Invoke-UpgradePython { param([string[]]$Arguments,[string]$EvidencePath="")
        $script:Trace.Add("python:"+$Arguments[0])
        return [pscustomobject]@{ ok=$true } }
    $backupCase = [pscustomobject]@{ RepoRoot=$repoRoot; DataDir=$dataRoot; EvidenceRoot=$contractRoot
        OldHistorySchema=4; ExpectedOldManifest=(Join-Path $repoRoot "scripts/fixtures/upgrade-0.1.0/expected.json")
        BackupsBefore=@() }
    $result = Assert-HistoryBackup -Case $backupCase -Expect "absent"
    Check ($result.applicable -and $result.expect -eq "absent") "结构未变时备份规则适用且要求不存在"
    "keep" | Set-Content -LiteralPath (Join-Path $dataRoot "history.db.backup-v3") -Encoding utf8NoBOM
    $backupCase.BackupsBefore = @(Get-HistoryBackups -DataDir $dataRoot)
    Check ((Assert-HistoryBackup -Case $backupCase -Expect "absent").expect -eq "absent") "结构未变时此前跳留下的备份允许保留"
    "new" | Set-Content -LiteralPath (Join-Path $dataRoot "history.db.backup-v4") -Encoding utf8NoBOM
    Expect-Failure { Assert-HistoryBackup -Case $backupCase -Expect "absent" } "结构未升级却新增备份被拒绝"
    $backupCase.BackupsBefore = @(Get-HistoryBackups -DataDir $dataRoot)
    $script:CurrentSchema = 5
    Expect-Failure { Assert-HistoryBackup -Case $backupCase } "结构升级不得覆盖这一跳之前已有的备份"
    Remove-Item -LiteralPath (Join-Path $dataRoot "history.db.backup-v4")
    $backupCase.BackupsBefore = @(Get-HistoryBackups -DataDir $dataRoot)
    "备份由这一跳留下" | Set-Content -LiteralPath (Join-Path $dataRoot "history.db.backup-v4") -Encoding utf8NoBOM
    $result = Assert-HistoryBackup -Case $backupCase
    Check ($result.expect -eq "created") "结构升级时自动要求备份"
    Check ($result.exists -and $backupCase.BackupName -eq "history.db.backup-v4") "结构升级的备份被登记为证据"
    Remove-Item -LiteralPath (Join-Path $dataRoot "history.db.backup-v4")
    Expect-Failure { Assert-HistoryBackup -Case $backupCase } "结构升级却没有备份被拒绝"
    $script:CurrentSchema = 4
    $backupCase.BackupsBefore = @(Get-HistoryBackups -DataDir $dataRoot)
    "tampered" | Set-Content -LiteralPath (Join-Path $dataRoot "history.db.backup-v3") -Encoding utf8NoBOM
    Expect-Failure { Assert-HistoryBackup -Case $backupCase } "结构未变不得改写此前跳留下的备份"
    Remove-Item -LiteralPath (Join-Path $dataRoot "history.db.backup-v3")
    $backupCase.BackupsBefore = @()
    $noHistory = [pscustomobject]@{ RepoRoot=$repoRoot; DataDir=$dataRoot; EvidenceRoot=$contractRoot
        OldHistorySchema=$null; ExpectedOldManifest="unused"; BackupsBefore=@() }
    Check (-not (Assert-HistoryBackup -Case $noHistory).applicable) "没有旧历史库时备份规则不适用"
    "非法备份" | Set-Content -LiteralPath (Join-Path $dataRoot "history.db.backup-v4") -Encoding utf8NoBOM
    Expect-Failure { Assert-HistoryBackup -Case $noHistory } "本跳之前没有历史库却出现备份被拒绝"
    Remove-Item -LiteralPath (Join-Path $dataRoot "history.db.backup-v4")

    # 快照函数（评审 2026-10-25 修复）：absent 哨兵必须稳定——"前后都不存在"
    # 不是变化；存在且变化必须检出。
    & {
        $snapRoot = Join-Path $contractRoot "snapshot"
        $missing = Join-Path $snapRoot "missing-dir"
        if (Test-Path $missing) { Remove-Item -Recurse -Force $missing }
        $a1 = @(Get-PathSnapshot $missing)
        $a2 = @(Get-PathSnapshot $missing)
        Check ((Compare-PathSnapshot -Before $a1 -After $a2).Count -eq 0) "前后都不存在不报变化"
        $present = Join-Path $snapRoot "present"
        New-Item -ItemType Directory -Path (Join-Path $present "sub") -Force | Out-Null
        $db = Join-Path $present "history.db"
        [IO.File]::WriteAllText($db, "seed", [Text.UTF8Encoding]::new($false))
        $b1 = @(Get-PathSnapshot $present)
        Check ((Compare-PathSnapshot -Before $b1 -After @(Get-PathSnapshot $present)).Count -eq 0) "未变化不报变化"
        Start-Sleep -Milliseconds 20
        [IO.File]::WriteAllText($db, "mutated", [Text.UTF8Encoding]::new($false))
        $diff = Compare-PathSnapshot -Before $b1 -After @(Get-PathSnapshot $present)
        Check ($diff.Count -eq 1 -and $diff[0] -like "*history.db|7|*") "内容变化按大小/时间检出：$($diff -join ';')"
        $absentDiff = Compare-PathSnapshot -Before $a1 -After $b1
        Check ($absentDiff.Count -gt 0) "从不存在到出现检为变化"
    }

    # 历史库快照范围（评审 2026-10-25 二轮）：必须覆盖主文件 + -wal + -shm
    # 三个文件——SQLite 的写入先进 WAL，只快照主文件会漏过同意前的写入。
    & {
        $histRoot = Join-Path $contractRoot "history-scope"
        New-Item -ItemType Directory -Path $histRoot -Force | Out-Null
        $encoding = [Text.UTF8Encoding]::new($false)
        [IO.File]::WriteAllText((Join-Path $histRoot 'history.db'), 'seed', $encoding)
        $before = @(Get-HistorySnapshot $histRoot)
        Check ($before.Count -eq 3 -and @($before | Where-Object { $_ -like 'absent|*' }).Count -eq 2) `
            "三文件快照：主文件在、wal/shm 以 absent 哨兵参与"
        Start-Sleep -Milliseconds 20
        # 注入 WAL 写入（评审探针的 wal-created 形态）
        [IO.File]::WriteAllText((Join-Path $histRoot 'history.db-wal'), 'written-before-consent', $encoding)
        $walDiff = Compare-PathSnapshot -Before $before -After @(Get-HistorySnapshot $histRoot)
        Check ($walDiff.Count -eq 1 -and $walDiff[0] -like '*history.db-wal|22|*') `
            "同意前新增 WAL 检为变化：$($walDiff -join ';')"
        # 注入 SHM 写入（shm-created 形态）
        $mid = @(Get-HistorySnapshot $histRoot)
        Start-Sleep -Milliseconds 20
        [IO.File]::WriteAllText((Join-Path $histRoot 'history.db-shm'), 'written-before-consent', $encoding)
        $shmDiff = Compare-PathSnapshot -Before $mid -After @(Get-HistorySnapshot $histRoot)
        Check ($shmDiff.Count -eq 1 -and $shmDiff[0] -like '*history.db-shm|22|*') `
            "同意前新增 SHM 检为变化：$($shmDiff -join ';')"
        # 三文件都不存在 → 前后快照差集为 0（absent 哨兵稳定）
        $empty = Join-Path $contractRoot "history-scope-empty"
        if (Test-Path $empty) { Remove-Item -Recurse -Force $empty }
        $e1 = @(Get-HistorySnapshot $empty)
        $e2 = @(Get-HistorySnapshot $empty)
        Check ((Compare-PathSnapshot -Before $e1 -After $e2).Count -eq 0) "三文件都不存在不报变化"
    }

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
        if ($script:FailAt -eq ("lifecycle:"+$Phase)) { throw "injected lifecycle $Phase" }
        # 与真实实现一致：核对回调会被调用（这样"链路里到底有没有进核对路径"
        # 能在契约里看出来，而不是靠实现自证）。
        if ($null -ne $Verify) { & $Verify $Case $null | Out-Null } }
    function Check-OldUsage { param($Case,$Context=$null) $script:Trace.Add("old-facts:"+$Case.JumpOrigin) }
    function Check-CurrentUsage { param($Case,$Context=$null)
        $script:Trace.Add("check-current:"+$Case.JumpOrigin+":"+$Case.OldHistorySchema+":"+
            $Case.JumpSettingsFormat+"->"+$Case.JumpTargetSettingsFormat) }
    function Save-OldUpgradeState { param($Case) $script:Trace.Add("save-old") }
    function Install-UpgradeCurrent { param($Case) $script:Trace.Add("install-current") }
    function Assert-PreservedBeforeLaunch { param($Case) $script:Trace.Add("preserved") }
    function Assert-InstalledUpgrade { param($Case) $script:Trace.Add("installed") }
    function Grant-UpgradeConsent { param($Case) $script:Trace.Add("consent") }
    function Invoke-ConsentGateStage { param($Case,[string]$Phase) $script:Trace.Add("consent-gate:" + $Phase) }
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
        $expected = "jump:0.1.0,install-baseline,prepare-old,lifecycle:old,old-facts:0.1.0,save-old,"
        if ($state -eq "deleted") { $expected += "delete-source," }
        # 目标格式 1 < 3：无隐私修订号闸门，观察段跳过；Set-UpgradeJump
        # 已提前到授权前（jump:0.3.3 出现在 consent 之前）。
        $expected += "install-current,preserved,jump:0.3.3,consent,lifecycle:current,"
        $expected += "check-current:0.3.3::1->1,lifecycle:restart,check-current:0.3.3::1->1,jump:current:True,case:True"
        Check (($script:Trace -join ",") -eq $expected) "$state：旧版先产数据，再直接覆盖升级并重启复核"
    }
    # WB05：目标格式 3（认识隐私修订号的版本）必须先经同意闸门观察段，
    # 再授权——顺序为 jump 切换 → gate → consent。
    & {
        $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
            -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState "present" -Origin "0.1.0" `
            -EvidenceRoot (Join-Path $contractRoot "scenario-gate") `
            -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="3" }) }) `
            -DataDir $dataRoot -TempRoot $tempRoot
        $script:Trace.Clear(); $script:FailAt = ""
        Invoke-UpgradeScenario -Case $case
        $expected = "jump:0.1.0,install-baseline,prepare-old,lifecycle:old,old-facts:0.1.0,save-old,"
        $expected += "install-current,preserved,jump:0.3.3,consent-gate:current,consent,lifecycle:current,"
        $expected += "check-current:0.3.3::1->3,lifecycle:restart,check-current:0.3.3::1->3,jump:current:True,case:True"
        Check (($script:Trace -join ",") -eq $expected) "格式3：先闸门观察再授权（gate 在 consent 前）"
    }
    # 0.6.0→0.6.1：0.6.0 的隐私修订为 2，当前共同同意修订为 3——旧同意
    # 对新版已失效（用户决定：协议更新后所有用户必须重新明确同意），
    # 因此闸门观察段必须适用、授权在观察之后。历史基线登记保持 2 不变
    # （不修改已发布版本的历史修订号来掩盖重新确认需求）。
    & {
        $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
            -AppVersion "0.6.1" -ExpectedCommit ("a"*40) -SourceState "present" -Origin "0.6.0" `
            -EvidenceRoot (Join-Path $contractRoot "scenario-stale-baseline") `
            -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="3" }) }) `
            -DataDir $dataRoot -TempRoot $tempRoot
        $script:Trace.Clear(); $script:FailAt = ""
        Invoke-UpgradeScenario -Case $case
        $expected = "jump:0.6.0,install-baseline,prepare-old,lifecycle:old,old-facts:0.6.0,save-old,"
        $expected += "install-current,preserved,jump:0.6.1,consent-gate:current,consent,lifecycle:current,"
        $expected += "check-current:0.6.1::3->3,lifecycle:restart,check-current:0.6.1::3->3,jump:current:True,case:True"
        Check (($script:Trace -join ",") -eq $expected) "0.6.0（revision 2）→ 0.6.1（revision 3）：旧同意失效，先闸门观察再授权"
    }
    $currentRevision = Get-ConsentRevision -RepoRoot $repoRoot
    Check (-not (Test-ConsentGateApplies -Case ([pscustomobject]@{ JumpTargetSettingsFormat=3; ConsentRevision=$currentRevision }))) "同修订（revision = 当前 $currentRevision）：闸门观察不适用"
    Check (Test-ConsentGateApplies -Case ([pscustomobject]@{ JumpTargetSettingsFormat=3; ConsentRevision=($currentRevision - 1) })) "低于当前修订：闸门观察适用"
    Check (Test-ConsentGateApplies -Case ([pscustomobject]@{ JumpTargetSettingsFormat=3; ConsentRevision=$null })) "布尔形态同意（无修订号）：闸门观察适用"
    Check (-not (Test-ConsentGateApplies -Case ([pscustomobject]@{ JumpTargetSettingsFormat=2; ConsentRevision=$null }))) "目标格式 < 3：无修订号闸门"

    # PR52 复盘（probe_stale_consent_gate.py）：观察函数把「字段存在」当
    # 「程序新写入」——启动前已有失效修订号（accepted=true、revision=1）
    # 且文件完全未变也被误判绕过。这里测真实 Invoke-ConsentGateStage 的
    # 完整判断路径：外部动作（注册表/进程/等待/证据落盘）全部惰性桩化，
    # 文件快照与比对用真实实现；观察期内的「程序写入」由等待桩注入。
    & {
        $gateRoot = Join-Path $contractRoot "gate-path"
        if (Test-Path $gateRoot) { Remove-Item -Recurse -Force $gateRoot }
        New-Item -ItemType Directory -Path (Join-Path $gateRoot "logs") -Force | Out-Null
        $settingsPath = Join-Path $gateRoot "settings.toml"
        $staleConsent = "privacy_policy_accepted = true`nprivacy_policy_revision = 1`nprice_auto_sync = false`n"
        [IO.File]::WriteAllText($settingsPath, $staleConsent, [Text.UTF8Encoding]::new($false))
        $mutate = $null
        function Get-UninstallEntry { [pscustomobject]@{ InstallLocation = $gateRoot } }
        function Launch-AppOnce { param($ExePath,$Scenario,$RunId,$EvidenceDir,$LiveLogsDir)
            [pscustomobject]@{ Process=[pscustomobject]@{ HasExited=$false; ExitCode=0 }
                EvidenceDir=$EvidenceDir; LiveLogsDir=$LiveLogsDir; Pid=4242 } }
        function Start-Sleep { if ($null -ne $mutate) { & $mutate } }
        function Stop-ScenarioProcess { }
        function Copy-LiveLogsToEvidence { }
        function Save-ScenarioResult { }
        function Invoke-UpgradeGateCase([string]$BaseText, [scriptblock]$Mutation, [string]$Label) {
            [IO.File]::WriteAllText($settingsPath, $BaseText, [Text.UTF8Encoding]::new($false))
            if (Test-Path (Join-Path $gateRoot "logs/new-before-consent.log")) {
                Remove-Item -Force (Join-Path $gateRoot "logs/new-before-consent.log") }
            if (Test-Path (Join-Path $gateRoot "history.db")) {
                Remove-Item -Force (Join-Path $gateRoot "history.db") }
            $mutate = $Mutation
            $case = [pscustomobject]@{ DataDir=$gateRoot
                EvidenceRoot=(Join-Path $gateRoot "evidence")
                JumpTargetSettingsFormat=3; ConsentRevision=1 }
            $errorText = $null
            try { & $script:RealConsentGate -Case $case -Phase "review" } catch { $errorText = $_.Exception.Message }
            return $errorText
        }
        # 已有失效修订号 + 文件完全未变 → 必须通过（红：现实现误判新写入）。
        $noChange = Invoke-UpgradeGateCase $staleConsent $null "stale-unchanged"
        Check ($null -eq $noChange) "闸门观察：已有失效修订号且文件未变 → 通过（不误判新写入）"
        # 观察期内程序新增修订号（布尔同意 → 出现修订号）→ 必须失败。
        $added = Invoke-UpgradeGateCase "privacy_policy_accepted = true`nprice_auto_sync = false`n" {
            [IO.File]::WriteAllText($settingsPath,
                "privacy_policy_accepted = true`nprivacy_policy_revision = 2`nprice_auto_sync = false`n",
                [Text.UTF8Encoding]::new($false)) } "revision-added"
        Check ($null -ne $added -and $added -like "*privacy_policy_revision*") "观察期新增修订号被拒：$added"
        # 观察期内程序把修订号 1 改写为 2 → 必须失败。
        $rewritten = Invoke-UpgradeGateCase $staleConsent { [IO.File]::WriteAllText($settingsPath,
            "privacy_policy_accepted = true`nprivacy_policy_revision = 2`nprice_auto_sync = false`n",
            [Text.UTF8Encoding]::new($false)) } "revision-rewritten"
        Check ($null -ne $rewritten -and $rewritten -like "*privacy_policy_revision*") "观察期改写修订号被拒：$rewritten"
        # 观察期内其他内容被改 → 沿用整文件比对失败。
        $other = Invoke-UpgradeGateCase $staleConsent { [IO.File]::WriteAllText($settingsPath,
            "privacy_policy_accepted = true`nprivacy_policy_revision = 1`nprice_auto_sync = true`n",
            [Text.UTF8Encoding]::new($false)) } "other-rewritten"
        Check ($null -ne $other -and $other -like "*settings.toml*") "观察期改写其他设置被拒：$other"
        # 观察期内业务日志/历史库新增 → 完整路径的文件哨兵仍然拦截。
        $newLog = Invoke-UpgradeGateCase $staleConsent { [IO.File]::WriteAllText((Join-Path $gateRoot "logs/new-before-consent.log"),
            "leak", [Text.UTF8Encoding]::new($false)) } "new-log"
        Check ($null -ne $newLog -and $newLog -like "*业务日志*") "观察期新增业务日志被拒：$newLog"
        $newHistory = Invoke-UpgradeGateCase $staleConsent { [IO.File]::WriteAllText((Join-Path $gateRoot "history.db"),
            "leak", [Text.UTF8Encoding]::new($false)) } "new-history"
        Check ($null -ne $newHistory -and $newHistory -like "*历史库*") "观察期历史库新写入被拒：$newHistory"
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
    # 当前版本设置格式取 2：中间跳装的是旧版，必须按『上一跳格式 → 本跳格式』
    # 核对（1->1），只有装当前包的那一跳才是 1->2（v0.5.0 发布失败的回归）。
    foreach ($state in @("present","deleted")) {
        $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath (Join-Path $contractRoot "current-setup.exe") `
            -AppVersion "0.3.3" -ExpectedCommit ("a"*40) -SourceState $state -Origin "0.1.0" `
            -EvidenceRoot (Join-Path $contractRoot ("chain-"+$state)) `
            -Metadata ([pscustomobject]@{ executable_sha256=("b"*64); persistent_format=([pscustomobject]@{ settings_format="2" }) }) `
            -DataDir $dataRoot -TempRoot $tempRoot
        $script:Trace.Clear(); $script:FailAt = ""
        Invoke-UpgradeChain -Case $case
        $expected = "jump:0.1.0,install-baseline,prepare-old,lifecycle:chain-00-old,old-facts:0.1.0,save-old,jump:chain-00-old:True,"
        if ($state -eq "deleted") { $expected += "delete-source," }
        $expected += "jump:0.3.2,download:0.3.2,install:0.3.2,consent,"
        $expected += "lifecycle:chain-01,check-current:0.3.2:4:1->1,jump:chain-01:True,"
        $expected += "jump:0.3.3,install:0.3.3,installed,consent,lifecycle:chain-02,check-current:0.3.3:4:1->2,"
        $expected += "jump:chain-02:True,lifecycle:restart,check-current:0.3.3:4:1->2,jump:restart:True,case:True"
        Check (($script:Trace -join ",") -eq $expected) "$state：链路逐跳覆盖安装并只播种一次"
        Check ((@($case.Chain) -join "->") -eq "0.1.0->0.3.2->0.3.3") "链路记录起点集合与当前版本"
        Check ((@($script:Trace | Where-Object { $_ -like "prepare-old*" }).Count) -eq 1) "$state：链路只播种一次来源日志"
    }
    Expect-Failure { Get-JumpSettingsFormat -Case $case -Version "9.9.9" } "未登记设置格式的版本被拒绝"
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
