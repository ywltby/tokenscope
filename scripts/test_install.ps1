#Requires -Version 7
# N12（安装部分）：NSIS 安装包的安装/升级/卸载验收。
#
# 生命周期判定（等待/观察/关闭）全部来自 scripts/install_test_helpers.ps1
# 的同一组函数（契约测试见 scripts/test_install_helpers.ps1）；事实断言
# 统一走 scripts/inspect_history.py --verify-manifest 与
# scripts/fixtures/install_history_expected.json（按业务身份逐条核对，
# 含三个 cwd 字段与 NULL 语义）。
#
# 场景：
#   1. 全新安装   程序、卸载注册项、开始菜单快捷方式与版本正确
#   2. 首次启动   合成 v10 cache.db + 设置 + 价格 → 启动 → 等待本次
#                 "汇总完成（请求=3）" → 逐条事实校验 → 优雅关闭 → 复核
#   3. 重复安装   安装成功，历史事实保留（manifest 复核）
#   4. 卸载       程序与注册项清除，~/.tokenscope 数据保留
#   5. 重新安装   保留的数据仍被新实例读取（汇总 + manifest 复核，不重复）
# 真实旧包升级由独立 Runner 的 scripts/test_upgrade.ps1 强制执行。
#
# 数据全部落在 $env:USERPROFILE\.tokenscope（Runner 上是一次性虚拟用户
# 目录）；每次启动前旧日志归档进 Runner 临时证据目录（always() 上传）。

param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$AppVersion
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "install_test_helpers.ps1")
. (Join-Path $PSScriptRoot "installer_helpers.ps1")

# Reset-UserHome 会整目录删除 $env:USERPROFILE\.tokenscope——开发机上这是
# 真实数据。真实安装只允许 GitHub-hosted 的一次性 Runner；本机只运行契约。
Assert-HostedInstallRunner
# 一次性 Runner 的证据与临时来源目录。
$TempRoot = $env:RUNNER_TEMP

$DataDir = Join-Path $env:USERPROFILE ".tokenscope"
$UninstallKeyRoot = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall"
$SettingsToml = Join-Path $DataDir "settings.toml"
$PricingToml = Join-Path $DataDir "pricing.toml"
$HistoryDb = Join-Path $DataDir "history.db"
$LegacyCache = Join-Path $DataDir "cache.db"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$ExpectedManifest = Join-Path $RepoRoot "scripts\fixtures\install_history_expected.json"
# "本次汇总请求数"的期望从期望 manifest 的事件数推导——fixture 与该数字
# 是两个独立来源，钉死一起改（脚本顶部加载一次）。
$ExpectedRequests = (Get-Content $ExpectedManifest -Raw | ConvertFrom-Json).events.Count
$EvidenceRoot = Join-Path $TempRoot "tokenscope-install-evidence"
$Failures = New-Object System.Collections.Generic.List[string]
$PassCount = 0

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) {
        $script:PassCount++
        Write-Host "PASS $Message"
    }
    else {
        $script:Failures.Add($Message) | Out-Null
        Write-Host "FAIL $Message"
    }
}

function Write-StepSummary {
    param([string]$Text)
    if ($env:GITHUB_STEP_SUMMARY) {
        Add-Content -Path $env:GITHUB_STEP_SUMMARY -Value $Text -Encoding utf8NoBOM
    }
}

function Get-UninstallerExe {
    param([string]$UninstallString)
    $value = $UninstallString.Trim()
    if ($value.StartsWith('"')) {
        return $value.Split('"')[1]
    }
    return ($value -split " ")[0]
}

function Reset-UserHome {
    Reset-InstallData -DataDir $DataDir -UserRoot $env:USERPROFILE
}

function Stop-App {
    Stop-Process -Name "tokenscope" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}

function Write-SeedData {
    # 旧版数据 = v10 cache.db（3 事件 / 2 项目 / 已知 token）+ 设置 +
    # 价格文件。两个**临时空来源目录**保证启动后的实际汇总只命中迁移的
    # 历史事实（3 请求），不混入任何真实来源日志。
    $sourceDirClaude = Join-Path $TempRoot "tokenscope-empty-source-claude"
    $sourceDirCodex = Join-Path $TempRoot "tokenscope-empty-source-codex"
    New-Item -ItemType Directory -Path $sourceDirClaude -Force | Out-Null
    New-Item -ItemType Directory -Path $sourceDirCodex -Force | Out-Null
    python -B (Join-Path $RepoRoot "scripts\make_legacy_cache.py") $LegacyCache
    if ($LASTEXITCODE -ne 0) { throw "合成 cache.db 失败" }
    $claudeDirJson = $sourceDirClaude -replace '\\', '/'
    $codexDirJson = $sourceDirCodex -replace '\\', '/'
    @"
privacy_policy_accepted = true
price_auto_sync = false
close_action = "quit"

[sources.claude]
enabled = true
dir = "$claudeDirJson"

[sources.codex]
enabled = true
dir = "$codexDirJson"
"@ | Set-Content -Path $SettingsToml -Encoding utf8NoBOM
    # 最小有效价格文件（无条目 → 模型按未知价格计，不影响 token 断言）。
    "" | Set-Content -Path $PricingToml -Encoding utf8NoBOM
}

function Verify-HistoryManifest {
    # 事实校验器：与期望文件逐条核对（身份/三 cwd/模型/四桶/NULL 语义）。
    # 退出码 0 = 通过；1 = 事实差异；2 = 打开/查询/参数错误——都直接失败。
    python -B (Join-Path $RepoRoot "scripts\inspect_history.py") $HistoryDb `
        --verify-manifest $ExpectedManifest
    if ($LASTEXITCODE -ne 0) {
        throw "事实校验失败（inspect_history --verify-manifest 退出码 $LASTEXITCODE）"
    }
}

function Run-AppLifecycle {
    # 场景 2/5/6 共用的单实例生命周期：归档旧日志 → 唯一启动 → 等待本次
    # "汇总完成（请求=期望数）" → 事实校验 → 8 秒存活观察 → 优雅关闭（退出码
    # 0）→ 关闭后再次事实校验 → 证据落盘。
    param(
        [Parameter(Mandatory = $true)][string]$Scenario,
        [Parameter(Mandatory = $true)][string]$RunId
    )
    Stop-App
    $logsDir = Join-Path $DataDir "logs"
    $evidenceDir = Prepare-LaunchEvidence -DataDir $DataDir `
        -EvidenceRoot $EvidenceRoot -Scenario $Scenario -RunId $RunId
    # K-P3-3：cargo bin 名是小写 tokenscope（NSIS 沿用），断言与实际一致。
    $exe = Join-Path (Get-UninstallEntry).InstallLocation "tokenscope.exe"
    $context = Launch-AppOnce -ExePath $exe -Scenario $Scenario `
        -RunId $RunId -EvidenceDir $evidenceDir `
        -LiveLogsDir (Join-Path $DataDir "logs")
    try {
        $summary = Wait-CurrentSummary -Context $context -ExpectedRequests $ExpectedRequests `
            -TimeoutSeconds 180
        Write-Host "本次汇总：query_id=$($summary.QueryId) requests=$($summary.Requests)"
        Verify-HistoryManifest
        Observe-ProcessAlive -Context $context -DurationSeconds 8 `
            -PollMilliseconds 200
        Close-AppGracefully -Context $context -TimeoutSeconds 30
        Verify-HistoryManifest
        Save-ScenarioResult -Context $context -Pass $true -Stage "closed"
    }
    catch {
        # A-P2-1：失败路径也要把**本次启动**的日志并入证据（成功路径由
        # Close-AppGracefully 复制）；A-P3-1：Launch-AppOnce 抛错时
        # $context 尚未赋值，先判空再保存/停进程。C-P3：证据动作自身
        # 失败只告警，不得掩盖原始异常。
        $original = $_.Exception.Message
        if ($null -ne $context) {
            try { Copy-LiveLogsToEvidence -Context $context }
            catch { Write-Host "::warning::证据日志归档失败：$($_.Exception.Message)" }
            try { Save-ScenarioResult -Context $context -Pass $false -Stage $context.Stage `
                    -Detail $original }
            catch { Write-Host "::warning::场景结果落盘失败：$($_.Exception.Message)" }
            Stop-ScenarioProcess -Context $context
        }
        throw
    }
}

$Summary = New-Object System.Collections.Generic.List[string]
$Summary.Add("## 安装/升级/卸载验收（N12 安装部分）") | Out-Null

try {
    # ---------- 场景 1：全新安装 ----------
    Write-Host "=== 场景 1：全新安装 ==="
    Reset-UserHome
    Invoke-SilentInstall -Path $InstallerPath -EvidenceRoot $EvidenceRoot
    $entry = Get-UninstallEntry
    Assert-True ($null -ne $entry) "卸载注册项存在（HKCU Uninstall）"
    Assert-True ($null -ne $entry -and $entry.DisplayVersion -eq $AppVersion) "DisplayVersion = $AppVersion（实际 $($entry.DisplayVersion)）"
    $installLocation = $entry.InstallLocation
    Assert-True ($null -ne $installLocation -and (Test-Path (Join-Path $installLocation "tokenscope.exe"))) "主程序 tokenscope.exe 存在（$installLocation）"
    $startMenuLnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\TokenScope.lnk"
    Assert-True (Test-Path $startMenuLnk) "开始菜单快捷方式存在"
    $Summary.Add("- 全新安装：程序/注册项/快捷方式/版本 $($entry.DisplayVersion) 正确") | Out-Null

    # ---------- 场景 2：首次启动迁移（请求数 / token / cwd） ----------
    Write-Host "=== 场景 2：首次启动迁移 ==="
    Write-SeedData
    Run-AppLifecycle -Scenario "fresh-migrate" -RunId "migrate-1"
    Assert-True $true "首次启动迁移：3 请求 / 180 输入 / 35 输出 / 2 项目 cwd（manifest 逐条核对）"
    $Summary.Add("- 首次启动迁移：3 请求 / 180 输入 / 35 输出 / 2 项目 cwd（manifest 逐条核对一致）") | Out-Null

    # ---------- 场景 3：重复安装（数据保留） ----------
    Write-Host "=== 场景 3：重复安装 ==="
    Invoke-SilentInstall -Path $InstallerPath -EvidenceRoot $EvidenceRoot
    Verify-HistoryManifest
    Assert-True ($null -ne (Get-UninstallEntry)) "重复安装后卸载注册项存在"
    $Summary.Add("- 重复安装：成功且历史事实逐条保留") | Out-Null

    # ---------- 场景 4：卸载（程序清除、数据保留） ----------
    Write-Host "=== 场景 4：卸载 ==="
    $entry = Get-UninstallEntry
    $uninstaller = Get-UninstallerExe $entry.UninstallString
    Stop-App
    $proc = Start-Process -FilePath $uninstaller -ArgumentList "/S" -Wait -PassThru
    # A-P3-2：NSIS 卸载器把自身复制到 %TEMP% 后由副本执行删除，原进程先
    # 退出——固定 2 秒睡眠在慢节点上有假失败风险，改为 30 秒内轮询。
    $uninstallDeadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $uninstallDeadline) {
        if (-not (Test-Path (Join-Path $entry.InstallLocation "tokenscope.exe")) -and
            $null -eq (Get-UninstallEntry)) {
            break
        }
        Start-Sleep -Milliseconds 500
    }
    Assert-True ($proc.ExitCode -eq 0) "卸载器退出码 0（实际 $($proc.ExitCode)）"
    Assert-True (-not (Test-Path (Join-Path $entry.InstallLocation "tokenscope.exe"))) "卸载后主程序已清除"
    Assert-True ($null -eq (Get-UninstallEntry)) "卸载后注册项已清除"
    Assert-True ((Test-Path $SettingsToml) -and (Test-Path $PricingToml) -and (Test-Path $HistoryDb)) "卸载后 ~/.tokenscope 数据保留"
    # 复核要求：卸载后不只检查文件存在——保留的事实逐条校验。
    Verify-HistoryManifest
    $Summary.Add("- 卸载：程序与注册项清除，~/.tokenscope 数据保留（事实逐条校验一致）") | Out-Null

    # ---------- 场景 5：重新安装（保留数据仍可读，不重复计量） ----------
    Write-Host "=== 场景 5：重新安装 ==="
    Invoke-SilentInstall -Path $InstallerPath -EvidenceRoot $EvidenceRoot
    Run-AppLifecycle -Scenario "reinstall" -RunId "reinstall-1"
    Assert-True $true "重新安装：保留数据被新实例读取（manifest 复核，不重复计量）"
    $Summary.Add("- 重新安装：保留数据被新实例读取（manifest 复核一致，事件数不重复）") | Out-Null

}
catch {
    $Failures.Add($_.Exception.Message) | Out-Null
    Write-Host "FAIL 异常：$($_.Exception.Message)"
    Stop-App
}

# ---------- 汇总 ----------
# 失败模型：Assert-True 累计失败后**继续**跑后续场景（单条断言失败不影响
# 其它事实的取证）；但任何**异常**（环境/前提破坏，如安装器失败）会跳过
# 剩余场景直接进汇总——此时后续场景的缺失是刻意为之，证据不完整本身
# 就是失败信号，不以"部分通过"呈现。
foreach ($line in $Summary) {
    Write-StepSummary $line
}
Write-Host ""
Write-Host "断言通过 $PassCount 项，失败 $($Failures.Count) 项"
if ($Failures.Count -gt 0) {
    foreach ($f in $Failures) {
        Write-Host "  - $f"
    }
    exit 1
}
exit 0
