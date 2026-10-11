# N12（安装部分）：NSIS 安装包的安装/升级/卸载验收。
#
# 场景（按序执行，全部针对同一个安装包）：
#   1. 全新安装   程序、卸载注册项、开始菜单快捷方式与版本正确
#   2. 首次启动   合成 v10 cache.db + 设置 + 价格 → 启动迁移 →
#                 请求数 / token / cwd 按已知值核对（不只是文件存在）
#   3. 重复安装   安装成功，用户数据保留
#   4. 卸载       程序与注册项清除，~/.tokenscope 数据保留
#   5. 重新安装   保留的数据仍能被当前程序读取（不重复计量）
#   6. 旧版升级   （可选）固定旧版本包：装旧版 → 造旧数据 → 升级安装新版
#                 → 迁移与保留断言；未配置 OLD_INSTALLER_URL 时以警告跳过
#
# 用法（CI）：
#   pwsh scripts/test_install.ps1 -InstallerPath <setup.exe> -AppVersion <semver> `
#     [-OldInstallerUrl <url> -OldInstallerSha256 <sha256>]
# 数据全部落在 $env:USERPROFILE\.tokenscope（Runner 上是一次性虚拟用户目录）。
# 退出码非 0 = 任一断言失败；每个断言实时打印 PASS/FAIL 并写入步骤摘要。

#Requires -Version 7
param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$AppVersion,
    [string]$OldInstallerUrl = "",
    [string]$OldInstallerSha256 = ""
)

$ErrorActionPreference = "Stop"
$DataDir = Join-Path $env:USERPROFILE ".tokenscope"
$UninstallKeyRoot = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall"
$SettingsToml = Join-Path $DataDir "settings.toml"
$PricingToml = Join-Path $DataDir "pricing.toml"
$HistoryDb = Join-Path $DataDir "history.db"
$LegacyCache = Join-Path $DataDir "cache.db"
$RepoRoot = Split-Path -Parent $PSScriptRoot
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

function Get-UninstallEntry {
    foreach ($key in Get-ChildItem $UninstallKeyRoot -ErrorAction SilentlyContinue) {
        $entry = Get-ItemProperty $key.PSPath -ErrorAction SilentlyContinue
        if ($entry.DisplayName -like "TokenScope*") {
            return $entry
        }
    }
    return $null
}

function Invoke-SilentInstall {
    param([string]$Path)
    $proc = Start-Process -FilePath $Path -ArgumentList "/S" -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        throw "安装器退出码异常（$Path）：$($proc.ExitCode)"
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
    if (Test-Path $DataDir) {
        Remove-Item $DataDir -Recurse -Force
    }
}

function Stop-App {
    Stop-Process -Name "TokenScope" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}

function Write-SeedData {
    # 旧版数据 = v10 cache.db（3 事件 / 2 项目 / 已知 token）+ 设置（已同意
    # 隐私、关闭价格自动同步）+ 价格文件。升级/迁移断言全部据此核对。
    python (Join-Path $RepoRoot "scripts\make_legacy_cache.py") $LegacyCache
    if ($LASTEXITCODE -ne 0) { throw "合成 cache.db 失败" }
    @"
privacy_policy_accepted = true
price_auto_sync = false
close_action = "quit"

[sources.claude]
enabled = true

[sources.codex]
enabled = true
"@ | Set-Content -Path $SettingsToml -Encoding utf8NoBOM
    # 最小有效价格文件（无条目 → 模型按未知价格计，不影响 token 断言）。
    "" | Set-Content -Path $PricingToml -Encoding utf8NoBOM
}

function Get-HistoryFacts {
    # 返回 (requests, input, output, projects)；库不存在/尚未建表返回 $null。
    if (-not (Test-Path $HistoryDb)) {
        return $null
    }
    $result = python (Join-Path $RepoRoot "scripts\inspect_history.py") $HistoryDb
    if ($LASTEXITCODE -ne 0) {
        throw "inspect_history.py 退出码 $LASTEXITCODE"
    }
    $parts = ($result | Out-String).Trim() -split "\s+"
    if ($parts[0] -eq "absent") {
        return $null
    }
    return [pscustomobject]@{
        Requests = [int]$parts[0]
        Input    = [int]$parts[1]
        Output   = [int]$parts[2]
        Projects = [int]$parts[3]
    }
}

function Wait-AppDbOpen {
    # M05：等待**新启动的应用实例**实际打开历史库——WAL 连接建立时
    # history.db-wal 文件出现，这是"新应用读取了保留数据"的可观察信号
    #（仅靠既有事件数判定会让预置数据直接通过，应用没读也算成功）。
    param([int]$TimeoutSeconds = 60)
    $wal = "$HistoryDb-wal"
    Start-Process -FilePath (Join-Path (Get-UninstallEntry).InstallLocation "TokenScope.exe")
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $wal) {
            return
        }
        Start-Sleep -Seconds 1
    }
    throw "应用未在 ${TimeoutSeconds}s 内打开历史库（$wal 未出现）"
}

function Wait-AppMigration {
    # 启动应用并等待历史库出现已知的三条事实（含迁移 + 幂等），超时失败。
    param([int]$TimeoutSeconds = 180)
    $exe = Join-Path (Get-UninstallEntry).InstallLocation "TokenScope.exe"
    Start-Process -FilePath $exe
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        $facts = Get-HistoryFacts
        if ($null -ne $facts -and $facts.Requests -ge 3) {
            return $facts
        }
    }
    throw "应用未在 ${TimeoutSeconds}s 内完成迁移/读取（history.db 未出现 3 条事实）"
}

function Assert-MigratedFacts {
    param($Facts)
    Assert-True ($null -ne $facts -and $facts.Requests -eq 3) "请求数 = 3（实际 $($facts.Requests)）"
    Assert-True ($null -ne $facts -and $facts.Input -eq 180) "输入 token 合计 = 180（实际 $($facts.Input)）"
    Assert-True ($null -ne $facts -and $facts.Output -eq 35) "输出 token 合计 = 35（实际 $($facts.Output)）"
    Assert-True ($null -ne $facts -and $facts.Projects -eq 2) "项目 cwd 去重 = 2（实际 $($facts.Projects)）"
    # M06：项目 cwd 必须是**已知的那两条路径**（数量相同但路径错误不得通过）。
    $projects = (python (Join-Path $RepoRoot "scripts\inspect_history.py") $HistoryDb "--projects") | Out-String
    foreach ($expected in @("C:/work/alpha", "C:/work/beta")) {
        Assert-True ($projects -match [regex]::Escape($expected)) "项目 cwd 包含 $expected"
    }
}

$Summary = New-Object System.Collections.Generic.List[string]
$Summary.Add("## 安装/升级/卸载验收（N12 安装部分）") | Out-Null

try {
    # ---------- 场景 1：全新安装 ----------
    Write-Host "=== 场景 1：全新安装 ==="
    Reset-UserHome
    Invoke-SilentInstall $InstallerPath
    $entry = Get-UninstallEntry
    Assert-True ($null -ne $entry) "卸载注册项存在（HKCU Uninstall）"
    Assert-True ($null -ne $entry -and $entry.DisplayVersion -eq $AppVersion) "DisplayVersion = $AppVersion（实际 $($entry.DisplayVersion)）"
    $installLocation = $entry.InstallLocation
    Assert-True ($null -ne $installLocation -and (Test-Path (Join-Path $installLocation "TokenScope.exe"))) "主程序 TokenScope.exe 存在（$installLocation）"
    $startMenuLnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\TokenScope.lnk"
    Assert-True (Test-Path $startMenuLnk) "开始菜单快捷方式存在"
    $Summary.Add("- 全新安装：程序/注册项/快捷方式/版本 $($entry.DisplayVersion) 正确") | Out-Null

    # ---------- 场景 2：首次启动迁移（请求数 / token / cwd） ----------
    Write-Host "=== 场景 2：首次启动迁移 ==="
    Stop-App
    Write-SeedData
    $facts = Wait-AppMigration
    Assert-MigratedFacts $facts
    $Summary.Add("- 首次启动迁移：3 请求 / 180 输入 / 35 输出 / 2 项目 cwd，与合成旧数据一致") | Out-Null
    Stop-App

    # ---------- 场景 3：重复安装（数据保留） ----------
    Write-Host "=== 场景 3：重复安装 ==="
    Invoke-SilentInstall $InstallerPath
    $facts = Get-HistoryFacts
    Assert-True ($null -ne $facts -and $facts.Requests -eq 3) "重复安装后历史用量保留（3 请求）"
    Assert-True ($null -ne (Get-UninstallEntry)) "重复安装后卸载注册项存在"
    $Summary.Add("- 重复安装：成功且用户数据保留") | Out-Null

    # ---------- 场景 4：卸载（程序清除、数据保留） ----------
    Write-Host "=== 场景 4：卸载 ==="
    $entry = Get-UninstallEntry
    $uninstaller = Get-UninstallerExe $entry.UninstallString
    Stop-App
    $proc = Start-Process -FilePath $uninstaller -ArgumentList "/S" -Wait -PassThru
    Start-Sleep -Seconds 2
    Assert-True ($proc.ExitCode -eq 0) "卸载器退出码 0（实际 $($proc.ExitCode)）"
    Assert-True (-not (Test-Path (Join-Path $entry.InstallLocation "TokenScope.exe"))) "卸载后主程序已清除"
    Assert-True ($null -eq (Get-UninstallEntry)) "卸载后注册项已清除"
    Assert-True ((Test-Path $SettingsToml) -and (Test-Path $PricingToml) -and (Test-Path $HistoryDb)) "卸载后 ~/.tokenscope 数据保留"
    $Summary.Add("- 卸载：程序与注册项清除，~/.tokenscope 数据保留") | Out-Null

    # ---------- 场景 5：重新安装（保留数据仍可读，不重复计量） ----------
    Write-Host "=== 场景 5：重新安装 ==="
    Invoke-SilentInstall $InstallerPath
    # 新应用实例必须实际打开既有历史库（-wal 出现），读取一段观察期后
    # 事件数不重复——只查既有行数会让"应用没读"也判成功（M05）。
    Wait-AppDbOpen
    Start-Sleep -Seconds 8
    Stop-App
    $facts = Get-HistoryFacts
    Assert-MigratedFacts $facts
    $Summary.Add("- 重新安装：保留数据被新应用实例打开并读取，事件数不重复（仍 3 条）") | Out-Null
}
catch {
    $Failures.Add($_.Exception.Message) | Out-Null
    Write-Host "FAIL 异常：$($_.Exception.Message)"
    Stop-App
}

# ---------- 场景 6：旧版升级（固定旧版本包，可选） ----------
if ($OldInstallerUrl -ne "") {
    Write-Host "=== 场景 6：旧版升级 ==="
    try {
        if ($OldInstallerSha256 -eq "") {
            throw "旧版升级已配置 OLD_INSTALLER_URL 但缺少 OLD_INSTALLER_SHA256：不允许在未校验固定旧包的情况下安装"
        }
        $oldPath = Join-Path $env:RUNNER_TEMP "tokenscope-old-setup.exe"
        if (Test-Path $oldPath) { Remove-Item $oldPath -Force }
        Invoke-WebRequest -Uri $OldInstallerUrl -OutFile $oldPath -UseBasicParsing
        $hash = (Get-FileHash $oldPath -Algorithm SHA256).Hash.ToLowerInvariant()
        # M07：SHA 不匹配在**安装之前**中止，绝不继续安装来源不明的包。
        if ($hash -ne $OldInstallerSha256.ToLowerInvariant()) {
            throw "旧版本包 SHA256 不匹配：$hash != $($OldInstallerSha256.ToLowerInvariant())"
        }
        Assert-True $true "旧版本包 SHA256 匹配"
        # 旧版安装 → 造旧版数据 → 升级安装新版 → 迁移与保留断言。
        Reset-UserHome
        Invoke-SilentInstall $oldPath
        Stop-App
        Write-SeedData
        Invoke-SilentInstall $InstallerPath
        $entry = Get-UninstallEntry
        Assert-True ($null -ne $entry -and $entry.DisplayVersion -eq $AppVersion) "升级后版本 = $AppVersion（实际 $($entry.DisplayVersion)）"
        $facts = Wait-AppMigration
        Assert-MigratedFacts $facts
        Assert-True ((Test-Path $SettingsToml) -and $SettingsToml -and ((Get-Content $SettingsToml -Raw) -match "privacy_policy_accepted = true")) "升级后设置保留"
        Assert-True (Test-Path $PricingToml) "升级后价格文件保留"
        $Summary.Add("- 旧版升级：固定旧包升级后历史用量/设置/价格保留，迁移计数一致") | Out-Null
        Stop-App
    }
    catch {
        $Failures.Add("旧版升级：$($_.Exception.Message)") | Out-Null
        Write-Host "FAIL 旧版升级：$($_.Exception.Message)"
        Stop-App
    }
}
else {
    Write-Host "::warning::未配置 OLD_INSTALLER_URL/OLD_INSTALLER_SHA256（仓库变量）：固定旧版本包的升级场景跳过；迁移与保留已由合成 v10 旧数据 + 新安装包覆盖。N12 的该项需在配置变量后复跑。"
    $Summary.Add("- 旧版升级：跳过（未配置 OLD_INSTALLER_URL 仓库变量）；迁移与保留由合成旧数据覆盖") | Out-Null
}

# ---------- 汇总 ----------
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
