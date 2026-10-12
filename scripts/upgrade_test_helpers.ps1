#Requires -Version 7
# 真实旧包升级的编排与事实操作；导入只定义函数，外部动作可在契约中替换。
function Invoke-UpgradePython {
    param([Parameter(Mandatory=$true)][string[]]$Arguments,[string]$EvidencePath="")
    $text = (& python -B @Arguments) -join "`n"
    $code = $LASTEXITCODE
    if ($EvidencePath) {
        New-Item -ItemType Directory -Path (Split-Path -Parent ([IO.Path]::GetFullPath($EvidencePath))) -Force | Out-Null
        $text | Set-Content -LiteralPath $EvidencePath -Encoding utf8NoBOM
    }
    if ($code -ne 0) { throw "升级 Python 校验失败（退出码 $code，证据：$EvidencePath）" }
    return ($text | ConvertFrom-Json)
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

function Install-UpgradeBaseline {
    param($Case)
    Assert-NoAliveTokenScope
    if ($null -ne (Get-UninstallEntry) -or (Test-Path -LiteralPath $Case.DataDir)) {
        throw "旧包升级必须使用没有 TokenScope 安装或数据的独立 Runner"
    }
    $download = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/release_assets.py"),
        "download-baseline","--directory",(Join-Path $Case.TempRoot "tokenscope-upgrade-old-package"))
    $Case.OldInstallerPath = $download.installer
    $expectedHash = @($Case.Baseline.assets | Where-Object { $_.name.EndsWith("-setup.exe") })[0].sha256
    if ((Get-FileHash -LiteralPath $Case.OldInstallerPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedHash) {
        throw "旧包执行前 SHA256 校验失败"
    }
    Invoke-SilentInstall -Path $Case.OldInstallerPath -EvidenceRoot $Case.EvidenceRoot
    $entry = Get-UninstallEntry
    if ($null -eq $entry -or $entry.DisplayVersion -ne $Case.Baseline.version) {
        throw "旧包实际安装版本与历史基线不一致"
    }
}

function Prepare-OldUsage {
    param($Case)
    Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"),"prepare",
        "--data-dir",$Case.DataDir,"--source-dir",$Case.SourcesRoot) | Out-Null
}

function Check-OldUsage {
    param($Case,$Context=$null)
    $evidence = if ($null -ne $Context) { $Context.EvidenceDir } else { $Case.EvidenceRoot }
    $stage = if ($null -ne $Context) { $Context.Stage } else { "facts" }
    $result = Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"),
        "verify","--database",(Join-Path $Case.DataDir "cache.db")) `
        -EvidencePath (Join-Path $evidence ("old-facts-"+$stage+".json"))
    if (-not $result.ok) { throw "真实旧版 cache.db 事实不一致" }
    $result | ConvertTo-Json -Depth 12 |
        Set-Content -LiteralPath (Join-Path $Case.EvidenceRoot "old-facts.json") -Encoding utf8NoBOM
}

function Save-OldUpgradeState {
    param($Case)
    $Case.OldExeHash = (Get-FileHash -LiteralPath (Join-Path (Get-UninstallEntry).InstallLocation "tokenscope.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
    $Case.CacheHash = (Get-FileHash -LiteralPath (Join-Path $Case.DataDir "cache.db") -Algorithm SHA256).Hash.ToLowerInvariant()
    $Case.SettingsHash = (Get-FileHash -LiteralPath (Join-Path $Case.DataDir "settings.toml") -Algorithm SHA256).Hash.ToLowerInvariant()
    $Case.PricingHash = (Get-FileHash -LiteralPath (Join-Path $Case.DataDir "pricing.toml") -Algorithm SHA256).Hash.ToLowerInvariant()
    Copy-Item -LiteralPath (Join-Path $Case.DataDir "settings.toml") -Destination (Join-Path $Case.EvidenceRoot "old-settings.toml")
    Copy-Item -LiteralPath (Join-Path $Case.DataDir "cache.db") -Destination (Join-Path $Case.EvidenceRoot "old-cache.db")
}

function Remove-UpgradeSources {
    param($Case)
    Remove-UpgradeFixtureSources -SourcesRoot $Case.SourcesRoot -TempRoot $Case.TempRoot -SourceState $Case.SourceState
}

function Install-UpgradeCurrent {
    param($Case)
    Assert-NoAliveTokenScope
    Invoke-SilentInstall -Path $Case.InstallerPath -EvidenceRoot $Case.EvidenceRoot
}

function Assert-InstalledUpgrade {
    param($Case)
    $entry = Get-UninstallEntry
    if ($null -eq $entry -or $entry.DisplayVersion -ne $Case.AppVersion) {
        throw "覆盖升级后的注册版本不一致"
    }
    $hash = (Get-FileHash -LiteralPath (Join-Path $entry.InstallLocation "tokenscope.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $Case.Metadata.executable_sha256 -or $hash -eq $Case.OldExeHash) {
        throw "覆盖升级没有安装当前构建的主程序"
    }
}

function Assert-PreservedBeforeLaunch {
    param($Case)
    foreach ($pair in @(@("cache.db",$Case.CacheHash),@("settings.toml",$Case.SettingsHash),@("pricing.toml",$Case.PricingHash))) {
        if ((Get-FileHash -LiteralPath (Join-Path $Case.DataDir $pair[0]) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $pair[1]) {
            throw "覆盖安装丢失或修改旧数据：$($pair[0])"
        }
    }
}

function Grant-UpgradeConsent {
    param($Case)
    Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"),"consent",
        "--data-dir",$Case.DataDir) | Out-Null
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
    if ((Get-FileHash -LiteralPath (Join-Path $Case.DataDir "cache.db") -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Case.CacheHash) {
        throw "新版修改了只读迁移来源 cache.db"
    }
    if ((Get-FileHash -LiteralPath (Join-Path $Case.DataDir "pricing.toml") -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Case.PricingHash) {
        throw "升级后的自定义价格文件未保留"
    }
    Invoke-UpgradePython -Arguments @((Join-Path $Case.RepoRoot "scripts/upgrade_fixture.py"),"settings",
        "--data-dir",$Case.DataDir,"--original",(Join-Path $Case.EvidenceRoot "old-settings.toml")) `
        -EvidencePath (Join-Path $evidence ("settings-"+$stage+".json")) | Out-Null
}

function Save-UpgradeFailureData {
    param($Case)
    $destination = Join-Path $Case.EvidenceRoot "failed-data"
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    foreach ($name in @("cache.db","cache.db-wal","cache.db-shm","history.db","history.db-wal",
            "history.db-shm","settings.toml","pricing.toml")) {
        $source = Join-Path $Case.DataDir $name
        if (Test-Path -LiteralPath $source -PathType Leaf) {
            Copy-Item -LiteralPath $source -Destination (Join-Path $destination $name) -Force
        }
    }
}

function Save-UpgradeCaseResult {
    param($Case,[bool]$Pass,[string]$Detail="")
    $payload = @{ schema_version=1; pass=$Pass; detail=$Detail; source_state=$Case.SourceState;
        baseline_version=$Case.Baseline.version; baseline_commit=$Case.Baseline.commit;
        current_version=$Case.AppVersion; current_commit=$Case.ExpectedCommit;
        old_executable_sha256=$Case.OldExeHash; current_executable_sha256=$Case.Metadata.executable_sha256;
        old_cache_sha256=$Case.CacheHash; settings_sha256=$Case.SettingsHash; pricing_sha256=$Case.PricingHash }
    $payload | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $Case.EvidenceRoot "upgrade-result.json") -Encoding utf8NoBOM
}

function Invoke-UpgradeScenario {
    param([Parameter(Mandatory=$true)]$Case)
    try {
        Install-UpgradeBaseline -Case $Case
        Prepare-OldUsage -Case $Case
        Invoke-UpgradeLifecycle -Case $Case -Phase "old" -Verify { param($Value,$Context) Check-OldUsage -Case $Value -Context $Context }
        Save-OldUpgradeState -Case $Case
        if ($Case.SourceState -eq "deleted") { Remove-UpgradeSources -Case $Case }
        Install-UpgradeCurrent -Case $Case
        Assert-InstalledUpgrade -Case $Case
        Assert-PreservedBeforeLaunch -Case $Case
        Grant-UpgradeConsent -Case $Case
        Invoke-UpgradeLifecycle -Case $Case -Phase "current" -Verify { param($Value,$Context) Check-CurrentUsage -Case $Value -Context $Context }
        Invoke-UpgradeLifecycle -Case $Case -Phase "restart" -Verify { param($Value,$Context) Check-CurrentUsage -Case $Value -Context $Context }
        Save-UpgradeCaseResult -Case $Case -Pass $true
    }
    catch {
        $detail = $_.Exception.Message
        try { Save-UpgradeFailureData -Case $Case }
        catch { Write-Host "::warning::升级数据证据保存失败：$($_.Exception.Message)" }
        Save-UpgradeCaseResult -Case $Case -Pass $false -Detail $detail
        throw
    }
}
