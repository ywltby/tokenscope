#Requires -Version 7
# 安装真实历史 0.1.0，让旧程序产数据，再直接覆盖当前包；仅一次性 Runner。
param(
    [Parameter(Mandatory=$true)][string]$InstallerPath,
    [Parameter(Mandatory=$true)][string]$AppVersion,
    [Parameter(Mandatory=$true)][string]$ExpectedCommit,
    [Parameter(Mandatory=$true)][ValidateSet("present","deleted")][string]$SourceState
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "install_test_helpers.ps1")
. (Join-Path $PSScriptRoot "installer_helpers.ps1")
. (Join-Path $PSScriptRoot "upgrade_test_helpers.ps1")
Assert-HostedInstallRunner
$repoRoot = Split-Path -Parent $PSScriptRoot
$evidenceRoot = Join-Path $env:RUNNER_TEMP ("tokenscope-install-evidence/upgrade-"+$SourceState)
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$case = $null
try {
    $baseline = Get-Content -LiteralPath (Join-Path $PSScriptRoot "fixtures/upgrade_baseline.json") -Raw -Encoding utf8 | ConvertFrom-Json
    $manifest = Join-Path $repoRoot $baseline.expected_manifest
    $metadata = Invoke-UpgradePython -Arguments @((Join-Path $PSScriptRoot "release_assets.py"),"verify",
        "--directory",(Split-Path -Parent $InstallerPath),"--commit",$ExpectedCommit,"--version",$AppVersion)
    if ([Version]$AppVersion -le [Version]$baseline.version) { throw "新版版本必须大于历史 0.1.0" }
    $case = [pscustomobject]@{ RepoRoot=$repoRoot; DataDir=(Join-Path $env:USERPROFILE ".tokenscope");
        TempRoot=$env:RUNNER_TEMP; EvidenceRoot=$evidenceRoot; Baseline=$baseline; Metadata=$metadata;
        ExpectedManifest=$manifest; ExpectedRequests=(Get-Content -LiteralPath $manifest -Raw -Encoding utf8 | ConvertFrom-Json).events.Count;
        InstallerPath=$InstallerPath; AppVersion=$AppVersion; ExpectedCommit=$ExpectedCommit; SourceState=$SourceState;
        SourcesRoot=(Join-Path $env:RUNNER_TEMP ("tokenscope-upgrade-sources-"+$SourceState));
        OldInstallerPath=$null; OldExeHash=$null; CacheHash=$null; SettingsHash=$null; PricingHash=$null }
    Invoke-UpgradeScenario -Case $case
    $message = "真实升级 $($baseline.version) -> $AppVersion / $SourceState：旧程序产数、迁移、四桶、配置保留与重启幂等全部通过"
    Write-Host "PASS $message"
    if ($env:GITHUB_STEP_SUMMARY) { Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value $message -Encoding utf8NoBOM }
    exit 0
}
catch {
    if ($null -eq $case) {
        @{ pass=$false; detail=$_.Exception.Message; source_state=$SourceState } | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $evidenceRoot "initial-failure.json") -Encoding utf8NoBOM
    }
    Write-Host "FAIL 真实升级：$($_.Exception.Message)"
    exit 1
}
