#Requires -Version 7
# 真实旧包升级的编排与事实操作；导入只定义函数，外部动作可在契约中替换。
# 单跳与连续升级链共用同一套函数：起点条目、期望清单与设置格式都取自
# scripts/fixtures/upgrade_baselines.json，编排不自己拼路径或版本号。
function Invoke-UpgradePython {
    param([Parameter(Mandatory=$true)][string[]]$Arguments,[string]$EvidencePath="")
    # 校验脚本输出固定为 UTF-8；中文 Windows 控制台的默认代码页（GBK）会把
    # UTF-8 字节按 ANSI 解成乱码，显式对齐后捕获结果才能逐字可比。
    [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
    $text = (& python -B @Arguments) -join "`n"
    $code = $LASTEXITCODE
    if ($EvidencePath) {
        New-Item -ItemType Directory -Path (Split-Path -Parent ([IO.Path]::GetFullPath($EvidencePath))) -Force | Out-Null
        $text | Set-Content -LiteralPath $EvidencePath -Encoding utf8NoBOM
    }
    if ($code -ne 0) { throw "升级 Python 校验失败（退出码 $code，证据：$EvidencePath）" }
    return ($text | ConvertFrom-Json)
}

function Invoke-UpgradePythonText {
    # 纯文本读数（历史库结构版本等）：不做 JSON 解析，失败即中断。
    param([Parameter(Mandatory=$true)][string[]]$Arguments)
    [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
    $text = (& python -B @Arguments) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "升级 Python 读取失败（退出码 $LASTEXITCODE）" }
    return $text.Trim()
}

function Get-UpgradeBaselines {
    # 登记表是起点身份、期望清单与安装包的唯一来源（脚本侧同一份校验）。
    param([Parameter(Mandatory=$true)][string]$RepoRoot)
    $registry = Invoke-UpgradePython -Arguments @((Join-Path $RepoRoot "scripts/upgrade_baselines.py"),"entries")
    if ($registry.schema_version -ne 2) { throw "升级起点登记表 schema 不是 2" }
    return $registry.entries
}

function Get-UpgradeBaseline {
    param($Entries,[Parameter(Mandatory=$true)][string]$Origin)
    $matched = @($Entries | Where-Object { $_.version -eq $Origin })
    if ($matched.Count -ne 1) { throw "登记表没有唯一匹配的起点 $Origin" }
    if ($matched[0].status -ne "published") { throw "起点 $Origin 不是已发布版本" }
    return $matched[0]
}

function New-UpgradeCase {
    # 一个升级作业的共享上下文：单跳与链路的每一跳都读写同一个对象。
    param(
        [Parameter(Mandatory=$true)][string]$RepoRoot,
        [Parameter(Mandatory=$true)][string]$InstallerPath,
        [Parameter(Mandatory=$true)][string]$AppVersion,
        [Parameter(Mandatory=$true)][string]$ExpectedCommit,
        [Parameter(Mandatory=$true)][ValidateSet("present","deleted")][string]$SourceState,
        [Parameter(Mandatory=$true)][string]$Origin,
        [Parameter(Mandatory=$true)][string]$EvidenceRoot,
        $Metadata=$null,
        [string]$DataDir="",
        [string]$TempRoot=""
    )
    $entries = Get-UpgradeBaselines -RepoRoot $RepoRoot
    $baseline = Get-UpgradeBaseline -Entries $entries -Origin $Origin
    $manifest = $baseline.expected_after.($SourceState)
    if (-not $DataDir) { $DataDir = Join-Path $env:USERPROFILE ".tokenscope" }
    if (-not $TempRoot) { $TempRoot = $env:RUNNER_TEMP }
    if (-not $TempRoot) { throw "升级编排缺少 Runner 临时目录" }
    $case = [pscustomobject]@{
        RepoRoot=$RepoRoot; DataDir=$DataDir
        TempRoot=$TempRoot; EvidenceRoot=$EvidenceRoot
        InstallerPath=$InstallerPath; AppVersion=$AppVersion; ExpectedCommit=$ExpectedCommit
        SourceState=$SourceState; Origin=$Origin; Baseline=$baseline; Profile=$baseline.profile
        Entries=$entries; Metadata=$Metadata
        ExpectedManifest=$manifest; ExpectedOldManifest=$baseline.expected_old
        ExpectedRequests=0; JumpSettingsFormat=$baseline.persistent_format.settings_format
        JumpTargetSettingsFormat=$baseline.persistent_format.settings_format
        ConsentRevision=$baseline.privacy_revision
        Chain=@($Origin); JumpIndex=0; Jumps=@(); JumpOrigin=$Origin
        SourcesRoot=$null; OldInstallerPath=$null; OldExeHash=$null; InstalledExeHash=$null
        CacheHash=$null; SettingsHash=$null; PricingHash=$null
        OldHistorySchema=$null; BackupName=$null; BackupsBefore=@()
        OldSettingsPath=$null; OldInstallerOrigin=$null
    }
    $case.SourcesRoot = Join-Path $TempRoot ("tokenscope-upgrade-sources-"+$SourceState)
    $case.OldSettingsPath = Join-Path $EvidenceRoot "old-settings.toml"
    $case.ExpectedRequests = (Get-Content -LiteralPath $manifest -Raw -Encoding utf8 | ConvertFrom-Json).events.Count
    return $case
}

function Get-HistorySchemaVersion {
    param([Parameter(Mandatory=$true)][string]$RepoRoot,[Parameter(Mandatory=$true)][string]$Database)
    $value = Invoke-UpgradePythonText -Arguments @((Join-Path $RepoRoot "scripts/inspect_history.py"),
        $Database,"--schema-version")
    if ($value -eq "absent") { return $null }
    return [int]$value
}

function Get-HistoryBackups {
    # 升级前备份清单（名字 + 内容哈希）：I4 要求结构未变的一跳不得新增或覆盖
    # 备份，此前跳留下的备份允许保留但不得被改写。
    param([Parameter(Mandatory=$true)][string]$DataDir)
    return @(Get-ChildItem -LiteralPath $DataDir -Filter "history.db.backup-v*" -ErrorAction SilentlyContinue |
        Sort-Object -Property Name | ForEach-Object {
            "{0}:{1}" -f $_.Name, (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        })
}

function Save-HistoryBackupState {
    # 每次安装目标包之前采样：这一跳是"新增备份"还是"不得动备份"由它决定。
    param($Case)
    $Case.BackupsBefore = @(Get-HistoryBackups -DataDir $Case.DataDir)
}

function Assert-HistoryBackup {
    # I4：发生结构升级的一跳必须新增 history.db.backup-v<旧结构>，且备份只读
    # 可打开并含旧事实；结构未变的一跳不得新增或覆盖备份。判定按**本跳安装前
    # 的实际旧库状态**（OldHistorySchema），与最初起点的夹具类型无关。
    param([Parameter(Mandatory=$true)]$Case,
        [ValidateSet("auto","created","absent")][string]$Expect="auto")
    $before = @($Case.BackupsBefore)
    $after = @(Get-HistoryBackups -DataDir $Case.DataDir)
    if ($null -eq $Case.OldHistorySchema) {
        if (($before -join "|") -ne ($after -join "|")) {
            throw "本跳之前没有历史库，却出现了升级备份：$($after -join '，')"
        }
        return [pscustomobject]@{ applicable=$false; before=$before; after=$after }
    }
    $name = "history.db.backup-v$($Case.OldHistorySchema)"
    $path = Join-Path $Case.DataDir $name
    $exists = Test-Path -LiteralPath $path -PathType Leaf
    $current = Get-HistorySchemaVersion -RepoRoot $Case.RepoRoot -Database (Join-Path $Case.DataDir "history.db")
    $expectation = $Expect
    if ($Expect -eq "auto") {
        $expectation = if ($null -ne $current -and $current -gt [int]$Case.OldHistorySchema) { "created" } else { "absent" }
    }
    if ($expectation -eq "created") {
        if (-not $exists) { throw "历史库结构已升级却没有留下升级前备份：$name" }
        if (@($before | Where-Object { $_ -like ($name + ":*") }).Count -ne 0) {
            throw "结构升级这一跳不得覆盖已有备份：$name"
        }
        $facts = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/inspect_history.py"),
            $path,"--verify-manifest",$Case.ExpectedOldManifest) `
            -EvidencePath (Join-Path $Case.EvidenceRoot "backup-facts.json")
        if (-not $facts.ok) { throw "升级前备份不含旧事实：$name" }
        $Case | Add-Member -NotePropertyName BackupName -NotePropertyValue $name -Force
    }
    else {
        if ($exists -and @($before | Where-Object { $_ -like ($name + ":*") }).Count -eq 0) {
            throw "历史库结构未升级却出现了备份：$name"
        }
        if (($before -join "|") -ne ($after -join "|")) {
            throw "历史库结构未升级却改动了升级备份：$($after -join '，')"
        }
    }
    return [pscustomobject]@{ applicable=$true; expect=$expectation; name=$name; exists=$exists
        old_schema=$Case.OldHistorySchema; current_schema=$current; before=$before; after=$after }
}

function Test-InstalledExecutable {
    param($Case)
    return (Get-FileHash -LiteralPath (Join-Path (Get-UninstallEntry).InstallLocation "tokenscope.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Save-JumpResult {
    param($Case,[string]$Phase,[bool]$Pass,[string]$Detail="")
    $Case.Jumps = @($Case.Jumps) + @([pscustomobject]@{
        phase=$Phase; origin=$Case.JumpOrigin; pass=$Pass; detail=$Detail
        registered_version=(Get-UninstallEntry).DisplayVersion; executable_sha256=$Case.InstalledExeHash })
}

function Invoke-UpgradeLifecycle {
    param([Parameter(Mandatory=$true)]$Case,
        [Parameter(Mandatory=$true)][string]$Phase,
        [Parameter(Mandatory=$true)][scriptblock]$Verify)
    $context = $null
    $evidence = Prepare-LaunchEvidence -DataDir $Case.DataDir -EvidenceRoot $Case.EvidenceRoot `
        -Scenario $Phase -RunId ($Phase+"-1")
    try {
        $exe = Join-Path (Get-UninstallEntry).InstallLocation "tokenscope.exe"
        $context = Launch-AppOnce -ExePath $exe -Scenario $Phase -RunId ($Phase+"-1") `
            -EvidenceDir $evidence -LiveLogsDir (Join-Path $Case.DataDir "logs")
        Wait-CurrentSummary -Context $context -ExpectedRequests $Case.ExpectedRequests -TimeoutSeconds 180 | Out-Null
        & $Verify $Case $context | Out-Null
        Observe-ProcessAlive -Context $context -DurationSeconds 8 -PollMilliseconds 200
        Close-AppGracefully -Context $context -TimeoutSeconds 30
        & $Verify $Case $context | Out-Null
        Save-ScenarioResult -Context $context -Pass $true -Stage "closed"
    }
    catch {
        $detail = $_.Exception.Message
        if ($null -ne $context) {
            Stop-ScenarioProcess -Context $context
            try {
                Copy-LiveLogsToEvidence -Context $context
                Save-ScenarioResult -Context $context -Pass $false -Stage $context.Stage -Detail $detail
            } catch { Write-Host "::warning::升级失败证据保存失败：$($_.Exception.Message)" }
        }
        throw
    }
}

function Install-UpgradePackage {
    # 安装链路上的一个包，并核对注册版本与主程序哈希都换成这个包。
    param($Case,[Parameter(Mandatory=$true)][string]$Origin,[Parameter(Mandatory=$true)][string]$InstallerPath)
    Assert-NoAliveTokenScope
    $previous = if ($Case.InstalledExeHash) { $Case.InstalledExeHash } else { $Case.OldExeHash }
    Invoke-SilentInstall -Path $InstallerPath -EvidenceRoot $Case.EvidenceRoot
    $entry = Get-UninstallEntry
    if ($null -eq $entry -or $entry.DisplayVersion -ne $Origin) {
        throw "覆盖升级后的注册版本不一致：期望 $Origin，实际 $($entry.DisplayVersion)"
    }
    $Case.InstalledExeHash = Test-InstalledExecutable -Case $Case
    if ($null -ne $previous -and $Case.InstalledExeHash -eq $previous) {
        throw "覆盖升级没有替换主程序：$Origin"
    }
    return $Case.InstalledExeHash
}

function Install-UpgradeBaseline {
    param($Case)
    Assert-NoAliveTokenScope
    if ($null -ne (Get-UninstallEntry) -or (Test-Path -LiteralPath $Case.DataDir)) {
        throw "旧包升级必须使用没有 TokenScope 安装或数据的独立 Runner"
    }
    $download = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/release_assets.py"),
        "download-baseline","--origin",$Case.Origin,
        "--directory",(Join-Path $Case.TempRoot ("tokenscope-upgrade-old-"+$Case.Origin)))
    $Case.OldInstallerPath = $download.installer
    if ((Get-FileHash -LiteralPath $Case.OldInstallerPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne
        $download.installer_sha256) {
        throw "旧包执行前 SHA256 校验失败"
    }
    Invoke-SilentInstall -Path $Case.OldInstallerPath -EvidenceRoot $Case.EvidenceRoot
    $entry = Get-UninstallEntry
    if ($null -eq $entry -or $entry.DisplayVersion -ne $Case.Origin) {
        throw "旧包实际安装版本与登记起点不一致"
    }
}

function Prepare-OldUsage {
    # 旧版阶段播种：设置格式 3 起的基线认识修订号，必须按登记的
    # privacy_revision（该基线自己认识的修订号）预置同意，否则基线按
    # StaleRevision 拒绝布尔同意、隐私门不开放，旧阶段等汇总必然超时
    # （0.6.0→0.6.1 首跳 CI 40226439492）。布尔时代基线保持布尔形态。
    param($Case)
    $arguments = @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"),"prepare",
        "--data-dir",$Case.DataDir,"--source-dir",$Case.SourcesRoot,"--profile",$Case.Profile)
    if ([int]$Case.Baseline.persistent_format.settings_format -ge 3) {
        if ($null -eq $Case.Baseline.privacy_revision) {
            throw "设置格式 3 起点 $($Case.Origin) 缺少 privacy_revision 登记"
        }
        $arguments += @("--consent-revision", [string]$Case.Baseline.privacy_revision)
    }
    Invoke-UpgradePython -Arguments $arguments | Out-Null
}

function Test-ConsentGateApplies {
    # WB05 闸门观察只对「旧同意对新版已失效」的跳有意义：来跳同意为布尔
    # 形态或修订号低于当前共同同意修订时，新闸门必须保持封闭、观察段适用；
    # 旧同意已带当前修订号时同意有效、无重新询问路径，观察段不适用
    # （有效同意下观察期必然看到业务）。当前修订从受测源码读取
    # （2026-10-28：升为 3，0.6.0 的 revision 2 因此必须重新同意）。
    param($Case)
    if ($Case.JumpTargetSettingsFormat -lt 3) { return $false }
    if ($null -eq $Case.ConsentRevision) { return $true }
    $repo = if ($Case.PSObject.Properties['RepoRoot'] -and $Case.RepoRoot) {
        $Case.RepoRoot
    } else {
        Split-Path $PSScriptRoot -Parent
    }
    $current = Get-ConsentRevision -RepoRoot $repo
    return ([int]$Case.ConsentRevision -lt $current)
}

function Check-OldUsage {
    param($Case,$Context=$null)
    $evidence = if ($null -ne $Context) { $Context.EvidenceDir } else { $Case.EvidenceRoot }
    $stage = if ($null -ne $Context) { $Context.Stage } else { "facts" }
    if ($Case.Profile -eq "legacy-cache") {
        $result = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"),
            "verify","--database",(Join-Path $Case.DataDir "cache.db")) `
            -EvidencePath (Join-Path $evidence ("old-facts-"+$stage+".json"))
        if (-not $result.ok) { throw "真实旧版 cache.db 事实不一致" }
    }
    else {
        $result = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/inspect_history.py"),
            (Join-Path $Case.DataDir "history.db"),"--verify-manifest",$Case.ExpectedOldManifest) `
            -EvidencePath (Join-Path $evidence ("old-facts-"+$stage+".json"))
        if (-not $result.ok) { throw "真实旧版 history.db 事实不一致" }
    }
    $result | ConvertTo-Json -Depth 12 |
        Set-Content -LiteralPath (Join-Path $Case.EvidenceRoot "old-facts.json") -Encoding utf8NoBOM
}

function Save-OldUpgradeState {
    param($Case)
    $Case.OldExeHash = Test-InstalledExecutable -Case $Case
    $Case.InstalledExeHash = $Case.OldExeHash
    $Case.SettingsHash = (Get-FileHash -LiteralPath (Join-Path $Case.DataDir "settings.toml") -Algorithm SHA256).Hash.ToLowerInvariant()
    $Case.PricingHash = (Get-FileHash -LiteralPath (Join-Path $Case.DataDir "pricing.toml") -Algorithm SHA256).Hash.ToLowerInvariant()
    Copy-Item -LiteralPath (Join-Path $Case.DataDir "settings.toml") -Destination $Case.OldSettingsPath
    if ($Case.Profile -eq "legacy-cache") {
        $Case.CacheHash = (Get-FileHash -LiteralPath (Join-Path $Case.DataDir "cache.db") -Algorithm SHA256).Hash.ToLowerInvariant()
        Copy-Item -LiteralPath (Join-Path $Case.DataDir "cache.db") -Destination (Join-Path $Case.EvidenceRoot "old-cache.db")
    }
    else {
        # 旧库是事实证据，不作字节比对：新版本会正常写入自己的历史库。
        $Case.OldHistorySchema = Get-HistorySchemaVersion -RepoRoot $Case.RepoRoot `
            -Database (Join-Path $Case.DataDir "history.db")
        Copy-Item -LiteralPath (Join-Path $Case.DataDir "history.db") -Destination (Join-Path $Case.EvidenceRoot "old-history.db")
    }
}

function Remove-UpgradeSources {
    param($Case)
    Remove-UpgradeFixtureSources -SourcesRoot $Case.SourcesRoot -TempRoot $Case.TempRoot -SourceState $Case.SourceState
}

function Install-UpgradeCurrent {
    param($Case)
    Install-UpgradePackage -Case $Case -Origin $Case.AppVersion -InstallerPath $Case.InstallerPath | Out-Null
    Assert-InstalledUpgrade -Case $Case
}

function Assert-InstalledUpgrade {
    param($Case)
    @{ registered_version=(Get-UninstallEntry).DisplayVersion; installed_executable_sha256=$Case.InstalledExeHash;
        expected_executable_sha256=$Case.Metadata.executable_sha256; old_executable_sha256=$Case.OldExeHash } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Case.EvidenceRoot "installed-build.json") -Encoding utf8NoBOM
    if ($Case.InstalledExeHash -ne $Case.Metadata.executable_sha256 -or $Case.InstalledExeHash -eq $Case.OldExeHash) {
        throw "覆盖升级没有安装当前构建的主程序"
    }
}

function Assert-PreservedBeforeLaunch {
    param($Case)
    $pairs = @(@("settings.toml",$Case.SettingsHash),@("pricing.toml",$Case.PricingHash))
    if ($Case.Profile -eq "legacy-cache") { $pairs += ,@("cache.db",$Case.CacheHash) }
    foreach ($pair in $pairs) {
        if ((Get-FileHash -LiteralPath (Join-Path $Case.DataDir $pair[0]) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $pair[1]) {
            throw "覆盖安装丢失或修改旧数据：$($pair[0])"
        }
    }
}

function Get-PathSnapshot {
    # 路径快照：不存在时返回稳定的 "absent|<路径>" 标记。早先返回空
    # hashtable——每次调用都是新实例，-notcontains 按引用比较恒不相等，
    # 会把"前后都不存在"误报成一次变化（0.1.0 起点没有 history.db 时
    # 正是这样误报的）。返回裸字符串（调用方 @() 包裹；, @() 的包裹在
    # 管道/foreach 收集场景会产生嵌套数组）。
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return "absent|$Path" }
    Get-ChildItem -LiteralPath $Path -Recurse -File -ErrorAction SilentlyContinue |
        ForEach-Object { "$($_.FullName)|$($_.Length)|$($_.LastWriteTimeUtc.Ticks)" }
}

function Compare-PathSnapshot {
    # 快照差集：After 中不在 Before 里的条目（新增或变化的文件）。
    # 结尾逗号防单元素展开：否则单结果时调用方拿到的是字符串，
    # [0] 取到第一个字符而不是整条路径。
    param([string[]]$Before, [string[]]$After)
    , @($After | Where-Object { $Before -notcontains $_ })
}

function Get-HistorySnapshot {
    # 历史库快照范围 = 主文件 + WAL + SHM 三个文件（评审 2026-10-25 二轮：
    # 只快照主文件时，同意前写入 history.db-wal / -shm 会漏过——SQLite
    # 的实际写入恰恰先进 WAL）。任一文件缺失都以 absent 哨兵参与比较。
    # 结尾不用 , @() 包裹：那会在输出端产生嵌套数组（调用方 @() 包裹时
    # 收成单元素）；管道本身逐字符串流出，调用方包 @() 即得平铺数组。
    param([string]$DataDir)
    @('history.db', 'history.db-wal', 'history.db-shm') |
        ForEach-Object { Get-PathSnapshot (Join-Path $DataDir $_) }
}

function Invoke-ConsentGateStage {
    # WB05 第一段：装新版后、授权前，先以旧同意（或无同意）启动一次——
    # 新闸门必须保持封闭，保存"零业务读取"证据并阻断任何违反：
    # - settings.toml 的同意记录原样。privacy_policy_revision 按**前后内容
    #   比对**判定「程序在未同意阶段新增/改写」：启动前已有失效修订号
    #   （如 revision=1）且内容未变不算违规（PR52 复盘：按「字段存在」
    #   判定会误拒同表单合法的旧同意状态）；真实新增或改写仍必须失败；
    # - DataDir/logs 无新增（业务日志初始化只发生在同意之后）；
    # - history.db（含 -wal/-shm，按内容/大小/mtime 快照）无新增写入
    #   （采集不发生）。任何一项违反都抛错结束本阶段，阶段证据按实际
    #   结果保存（失败不写 pass: true）。
    # 观察窗口 45 秒后结束进程；第二段由 Grant-UpgradeConsent 授权。
    param($Case, [string]$Phase)
    $evidence = Join-Path $Case.EvidenceRoot "consent-gate"
    New-Item -ItemType Directory -Path $evidence -Force | Out-Null
    $logsDir = Join-Path $Case.DataDir "logs"
    $settingsPath = Join-Path $Case.DataDir "settings.toml"
    $logsBefore = @(Get-PathSnapshot $logsDir)
    $historyBefore = @(Get-HistorySnapshot $Case.DataDir)
    $settingsBefore = (Get-Content -LiteralPath $settingsPath -Raw -Encoding UTF8)

    $exe = Join-Path (Get-UninstallEntry).InstallLocation "tokenscope.exe"
    $context = Launch-AppOnce -ExePath $exe -Scenario $Phase -RunId ($Phase + "-gate-1") `
        -EvidenceDir $evidence -LiveLogsDir $logsDir
    $gateOk = $false
    $gateDetail = ""
    try {
        Start-Sleep -Seconds 45
        if ($context.Process.HasExited) {
            throw "旧同意下新版进程提前退出（退出码 $($context.Process.ExitCode)）"
        }
        $settingsAfter = (Get-Content -LiteralPath $settingsPath -Raw -Encoding UTF8)
        $revisionPattern = '(?m)^[ \t]*privacy_policy_revision[ \t]*=.*$'
        $revisionBefore = [regex]::Match($settingsBefore, $revisionPattern).Value
        $revisionAfter = [regex]::Match($settingsAfter, $revisionPattern).Value
        if ($revisionAfter -cne $revisionBefore) {
            throw "同意闸门前程序新增/改写了 privacy_policy_revision（前=[$revisionBefore] 后=[$revisionAfter]）"
        }
        if ($settingsAfter -ne $settingsBefore) {
            throw "同意闸门前 settings.toml 被改写"
        }
        $newLogs = Compare-PathSnapshot -Before $logsBefore -After @(Get-PathSnapshot $logsDir)
        if ($newLogs.Count -gt 0) {
            throw "同意前出现业务日志新增：$($newLogs[0])"
        }
        $newHistory = Compare-PathSnapshot -Before $historyBefore -After @(Get-HistorySnapshot $Case.DataDir)
        if ($newHistory.Count -gt 0) {
            throw "同意前历史库出现新写入：$($newHistory[0])"
        }
        $gateOk = $true
        [ordered]@{
            settings_unchanged    = $true
            revision_not_written  = $true
            logs_new_files        = 0
            history_new_entries   = 0
            observed_seconds      = 45
        } | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $evidence ($Phase + "-gate.json")) -Encoding utf8NoBOM
    }
    catch {
        $gateDetail = $_.Exception.Message
        throw
    }
    finally {
        Stop-ScenarioProcess -Context $context
        Copy-LiveLogsToEvidence -Context $context
        Save-ScenarioResult -Context $context -Pass $gateOk `
            -Stage ($Phase + "-gate-" + $(if ($gateOk) { "closed" } else { "failed" })) `
            -Detail $gateDetail
    }
}

function Get-ConsentRevision {
    # 当前共同同意修订号：从受测源码读取（单一事实源，不硬编码）。
    # 正则行首锚定常量声明，注释里的同名文本不会命中。
    param([string]$RepoRoot = (Split-Path $PSScriptRoot -Parent))
    if (-not $RepoRoot) { $RepoRoot = Split-Path $PSScriptRoot -Parent }
    $source = Join-Path $RepoRoot "crates/tokenscope-core/src/settings.rs"
    $text = Get-Content -LiteralPath $source -Raw -Encoding utf8
    $m = [regex]::Match($text,
        '(?m)^(?:pub\s+)?const\s+PRIVACY_POLICY_REVISION\s*:\s*u8\s*=\s*(\d+)\s*;')
    if (-not $m.Success) { throw "settings.rs 缺少 PRIVACY_POLICY_REVISION 常量" }
    return [int]$m.Groups[1].Value
}

function Grant-UpgradeConsent {
    # 幂等：起点已带同意时不动设置文件，0.1.0 起点升级后补上。
    # WB05：本跳目标格式 ≥ 3 的版本认识修订号——写入**受测源码里的当前
    # 共同同意修订号**（隐私政策 + 用户许可协议同一次写入四项），让升级后的
    # 启动直接进入业务；历史跳保持布尔形态（受测旧版本不认识这些字段）。
    param($Case)
    $arguments = @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"), "ensure-consent",
        "--data-dir", $Case.DataDir)
    if ($Case.JumpTargetSettingsFormat -ge 3) {
        $arguments += @("--revision", [string](Get-ConsentRevision -RepoRoot $Case.RepoRoot))
    }
    Invoke-UpgradePython -Arguments $arguments | Out-Null
}

function Check-CurrentUsage {
    param($Case,$Context=$null)
    $evidence = if ($null -ne $Context) { $Context.EvidenceDir } else { $Case.EvidenceRoot }
    $stage = if ($null -ne $Context) { $Context.Stage } else { "facts" }
    $facts = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/inspect_history.py"),
        (Join-Path $Case.DataDir "history.db"),"--verify-manifest",$Case.ExpectedManifest) `
        -EvidencePath (Join-Path $evidence ("current-facts-"+$stage+".json"))
    if (-not $facts.ok) { throw "升级后的历史事实不一致" }
    $facts | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $Case.EvidenceRoot "current-facts.json") -Encoding utf8NoBOM
    if ($Case.Profile -eq "legacy-cache") {
        if ((Get-FileHash -LiteralPath (Join-Path $Case.DataDir "cache.db") -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Case.CacheHash) {
            throw "新版修改了只读迁移来源 cache.db"
        }
    }
    if ((Get-FileHash -LiteralPath (Join-Path $Case.DataDir "pricing.toml") -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Case.PricingHash) {
        throw "升级后的自定义价格文件未保留"
    }
    Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"),"settings",
        "--data-dir",$Case.DataDir,"--original",$Case.OldSettingsPath,
        "--from-format",$Case.JumpSettingsFormat,
        "--to-format",$Case.JumpTargetSettingsFormat) `
        -EvidencePath (Join-Path $evidence ("settings-"+$stage+".json")) | Out-Null
    # 备份规则按本跳实际的旧库状态判定，与最初起点的夹具类型无关。
    $backup = Assert-HistoryBackup -Case $Case
    if ($backup.applicable) {
        $backup | ConvertTo-Json -Depth 6 |
            Set-Content -LiteralPath (Join-Path $Case.EvidenceRoot "history-backup.json") -Encoding utf8NoBOM
    }
}

function Save-UpgradeFailureData {
    param($Case)
    $destination = Join-Path $Case.EvidenceRoot "failed-data"
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    $names = @("cache.db","cache.db-wal","cache.db-shm","history.db","history.db-wal",
            "history.db-shm","settings.toml","pricing.toml")
    $names += @(Get-ChildItem -LiteralPath $Case.DataDir -Filter "history.db.backup-v*" -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty Name)
    foreach ($name in $names) {
        $source = Join-Path $Case.DataDir $name
        if (Test-Path -LiteralPath $source -PathType Leaf) {
            Copy-Item -LiteralPath $source -Destination (Join-Path $destination $name) -Force
        }
    }
}

function Save-UpgradeCaseResult {
    param($Case,[bool]$Pass,[string]$Detail="")
    $payload = @{ schema_version=2; pass=$Pass; detail=$Detail; source_state=$Case.SourceState;
        origin=$Case.Origin; profile=$Case.Profile; baseline_commit=$Case.Baseline.commit;
        current_version=$Case.AppVersion; current_commit=$Case.ExpectedCommit;
        old_executable_sha256=$Case.OldExeHash; current_executable_sha256=$Case.Metadata.executable_sha256;
        installed_executable_sha256=$Case.InstalledExeHash;
        old_cache_sha256=$Case.CacheHash; settings_sha256=$Case.SettingsHash; pricing_sha256=$Case.PricingHash;
        old_history_schema=$Case.OldHistorySchema; history_backup=$Case.BackupName;
        chain=@($Case.Chain); jumps=@($Case.Jumps) }
    $payload | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $Case.EvidenceRoot "upgrade-result.json") -Encoding utf8NoBOM
}

function Invoke-UpgradeScenario {
    # 单个起点的一次覆盖升级：旧版自己产数据 → 覆盖 → 重启幂等。
    param([Parameter(Mandatory=$true)]$Case)
    New-Item -ItemType Directory -Path $Case.EvidenceRoot -Force | Out-Null
    try {
        Set-UpgradeJump -Case $Case -Version $Case.Origin | Out-Null
        Install-UpgradeBaseline -Case $Case
        Prepare-OldUsage -Case $Case
        Invoke-UpgradeLifecycle -Case $Case -Phase "old" -Verify { param($Value,$Context) Check-OldUsage -Case $Value -Context $Context }
        Save-OldUpgradeState -Case $Case
        if ($Case.SourceState -eq "deleted") { Remove-UpgradeSources -Case $Case }
        Save-HistoryBackupState -Case $Case
        Install-UpgradeCurrent -Case $Case
        Assert-PreservedBeforeLaunch -Case $Case
        # 先把跳指针切到新版本：Grant 与观察段都按「本跳目标格式」判断是否
        # 涉及隐私修订号（设置格式 3 起引入）；Set-UpgradeJump 只更新期望
        # 字段，不触发外部动作，提前不影响语义。
        Set-UpgradeJump -Case $Case -Version $Case.AppVersion | Out-Null
        # WB05：目标版本认识修订号且旧同意已失效时，先验证旧同意被新闸门
        # 拒绝（零业务读取）再授权当前修订；旧同意已带当前修订号（同修订
        # 基线跳）或目标不认识修订号（格式 < 3）时无重新询问路径，观察段
        # 跳过。
        if (Test-ConsentGateApplies -Case $Case) {
            Invoke-ConsentGateStage -Case $Case -Phase "current"
        }
        Grant-UpgradeConsent -Case $Case
        $Case.ConsentRevision = if ($Case.JumpTargetSettingsFormat -ge 3) { 2 } else { $null }
        Invoke-UpgradeLifecycle -Case $Case -Phase "current" -Verify { param($Value,$Context) Check-CurrentUsage -Case $Value -Context $Context }
        Invoke-UpgradeLifecycle -Case $Case -Phase "restart" -Verify { param($Value,$Context) Check-CurrentUsage -Case $Value -Context $Context }
        Save-JumpResult -Case $Case -Phase "current" -Pass $true
        Save-UpgradeCaseResult -Case $Case -Pass $true
    }
    catch {
        $detail = $_.Exception.Message
        try { Save-JumpResult -Case $Case -Phase "failed" -Pass $false -Detail $detail } catch { }
        try { Save-UpgradeFailureData -Case $Case }
        catch { Write-Host "::warning::升级数据证据保存失败：$($_.Exception.Message)" }
        Save-UpgradeCaseResult -Case $Case -Pass $false -Detail $detail
        throw
    }
}

function Get-JumpSettingsFormat {
    # 某个版本写出的设置格式：当前版本取构建元数据，历史版本取登记表。
    param($Case,[Parameter(Mandatory=$true)][string]$Version)
    if ($Version -eq $Case.AppVersion) { return $Case.Metadata.persistent_format.settings_format }
    $matched = @($Case.Entries | Where-Object { $_.version -eq $Version })
    if ($matched.Count -ne 1 -or $null -eq $matched[0].persistent_format) {
        throw "版本 $Version 没有登记设置格式"
    }
    return $matched[0].persistent_format.settings_format
}

function Set-UpgradeJump {
    # 把编排切到某一跳：期望清单按这一跳的版本解析（当前版本尚未登记时
    # 沿用起点期望）。设置核对按『上一跳版本写出的格式 → 本跳版本的格式』：
    # 链路中间跳装的是旧版，不能拿当前版本的格式核对（0.1.0→0.4.0 仍是 v1）。
    param($Case,[Parameter(Mandatory=$true)][string]$Version)
    $matched = @($Case.Entries | Where-Object { $_.version -eq $Version })
    $source = if ($matched.Count -eq 1) { $matched[0] } else { $Case.Baseline }
    $previous = $Case.JumpOrigin
    $Case.JumpOrigin = $Version
    $Case.JumpTargetSettingsFormat = Get-JumpSettingsFormat -Case $Case -Version $Version
    $Case.JumpSettingsFormat = if ($previous -and $previous -ne $Version) {
        Get-JumpSettingsFormat -Case $Case -Version $previous
    } else { $Case.JumpTargetSettingsFormat }
    $Case.ExpectedManifest = $source.expected_after.($Case.SourceState)
    $Case.ExpectedRequests = (Get-Content -LiteralPath $Case.ExpectedManifest -Raw -Encoding utf8 | ConvertFrom-Json).events.Count
    return $source
}

function Get-UpgradeChain {
    # 链路只经过起点集合（登记表选择规则的输出），不经过全部已发布版本。
    param($Case)
    $selection = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/upgrade_baselines.py"),
        "select","--current-version",$Case.AppVersion)
    $chain = @($selection.chain)
    if ($chain.Count -eq 0) { throw "起点集合为空：不能用空矩阵当通过" }
    if ($chain[-1] -eq $Case.AppVersion) { $chain = $chain[0..($chain.Count-2)] }
    return $chain
}

function Invoke-UpgradeChain {
    # 连续升级链：从最老起点逐跳覆盖安装，最后装当前包；任一跳失败即中止。
    # 只播种一次；deleted 链在最老版本采集后删除日志，后续跳只能依赖已存事实。
    param([Parameter(Mandatory=$true)]$Case)
    New-Item -ItemType Directory -Path $Case.EvidenceRoot -Force | Out-Null
    try {
        $chain = Get-UpgradeChain -Case $Case
        $Case.Chain = @($chain) + @($Case.AppVersion)
        Set-UpgradeJump -Case $Case -Version $chain[0] | Out-Null
        Install-UpgradeBaseline -Case $Case
        Prepare-OldUsage -Case $Case
        Invoke-UpgradeLifecycle -Case $Case -Phase "chain-00-old" -Verify { param($Value,$Context) Check-OldUsage -Case $Value -Context $Context }
        Save-OldUpgradeState -Case $Case
        Save-JumpResult -Case $Case -Phase "chain-00-old" -Pass $true
        if ($Case.SourceState -eq "deleted") { Remove-UpgradeSources -Case $Case }
        $tail = @()
        if ($chain.Count -gt 1) { $tail = @($chain[1..($chain.Count-1)]) }
        $step = 0
        foreach ($version in $tail + @($Case.AppVersion)) {
            $step++
            $suffix = "{0:d2}" -f $step
            Set-UpgradeJump -Case $Case -Version $version | Out-Null
            Copy-Item -LiteralPath (Join-Path $Case.DataDir "settings.toml") -Destination $Case.OldSettingsPath -Force
            $Case.SettingsHash = (Get-FileHash -LiteralPath $Case.OldSettingsPath -Algorithm SHA256).Hash.ToLowerInvariant()
            # 这一跳安装前留下的历史库结构版本（可能还不存在）与备份清单：
            # 备份规则按它们判定，不按起点夹具类型。
            $Case.OldHistorySchema = Get-HistorySchemaVersion -RepoRoot $Case.RepoRoot `
                -Database (Join-Path $Case.DataDir "history.db")
            Save-HistoryBackupState -Case $Case
            $installer = $Case.InstallerPath
            if ($version -ne $Case.AppVersion) {
                $download = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/release_assets.py"),
                    "download-baseline","--origin",$version,
                    "--directory",(Join-Path $Case.TempRoot ("tokenscope-upgrade-old-"+$version)))
                $installer = $download.installer
            }
            Install-UpgradePackage -Case $Case -Origin $version -InstallerPath $installer | Out-Null
            if ($version -eq $Case.AppVersion) { Assert-InstalledUpgrade -Case $Case }
            # WB05：本跳目标格式 ≥ 3 且来跳同意已失效时，先验证旧同意被闸门
            # 拒绝（零业务读取）再授权；中间跳装的是旧版，布尔同意即放行、
            # 同修订跳旧同意仍有效，观察段跳过（Set-UpgradeJump 已在安装前
            # 把 JumpTargetSettingsFormat 切到本跳目标）。
            if (Test-ConsentGateApplies -Case $Case) {
                Invoke-ConsentGateStage -Case $Case -Phase ("chain-" + $suffix)
            }
            # 0.2.0 起没有同意就不会采集：安装后、启动前补上（幂等）。
            Grant-UpgradeConsent -Case $Case
            $Case.ConsentRevision = if ($Case.JumpTargetSettingsFormat -ge 3) { 2 } else { $null }
            Invoke-UpgradeLifecycle -Case $Case -Phase ("chain-"+$suffix) -Verify { param($Value,$Context) Check-CurrentUsage -Case $Value -Context $Context }
            Save-JumpResult -Case $Case -Phase ("chain-"+$suffix) -Pass $true
        }
        Invoke-UpgradeLifecycle -Case $Case -Phase "restart" -Verify { param($Value,$Context) Check-CurrentUsage -Case $Value -Context $Context }
        Save-JumpResult -Case $Case -Phase "restart" -Pass $true
        Save-UpgradeCaseResult -Case $Case -Pass $true
    }
    catch {
        $detail = $_.Exception.Message
        try { Save-JumpResult -Case $Case -Phase "failed" -Pass $false -Detail $detail } catch { }
        try { Save-UpgradeFailureData -Case $Case }
        catch { Write-Host "::warning::升级链数据证据保存失败：$($_.Exception.Message)" }
        Save-UpgradeCaseResult -Case $Case -Pass $false -Detail $detail
        throw
    }
}
