#Requires -Version 7
# 安装一个真实历史起点包，让旧程序产数据，再直接覆盖当前包；仅一次性 Runner。
# 起点由登记表决定（默认矩阵里的 origin），脚本不自己拼版本或路径。
param(
    [Parameter(Mandatory=$true)][string]$InstallerPath,
    [Parameter(Mandatory=$true)][string]$AppVersion,
    [Parameter(Mandatory=$true)][string]$ExpectedCommit,
    [Parameter(Mandatory=$true)][ValidateSet("present","deleted")][string]$SourceState,
    [Parameter(Mandatory=$true)][string]$Origin
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "install_test_helpers.ps1")
. (Join-Path $PSScriptRoot "installer_helpers.ps1")
. (Join-Path $PSScriptRoot "upgrade_test_helpers.ps1")
Assert-HostedInstallRunner
$repoRoot = Split-Path -Parent $PSScriptRoot
$evidenceRoot = Join-Path $env:RUNNER_TEMP ("tokenscope-install-evidence/upgrade-"+$Origin+"-"+$SourceState)
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$case = $null
try {
    $metadata = Invoke-UpgradePython -Arguments @((Join-Path $PSScriptRoot "release_assets.py"),"verify",
        "--directory",(Split-Path -Parent $InstallerPath),"--commit",$ExpectedCommit,"--version",$AppVersion)
    if ([Version]$AppVersion -le [Version]$Origin) { throw "当前版本必须高于起点 $Origin" }
    $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath $InstallerPath -AppVersion $AppVersion `
        -ExpectedCommit $ExpectedCommit -SourceState $SourceState -Origin $Origin `
        -EvidenceRoot $evidenceRoot -Metadata $metadata
    Invoke-UpgradeScenario -Case $case
    $message = "真实升级 $Origin（$($case.Profile)）-> $AppVersion / $SourceState：" +
        "旧程序产数、迁移、四桶、配置保留与重启幂等全部通过"
    Write-Host "PASS $message"
    if ($env:GITHUB_STEP_SUMMARY) { Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value $message -Encoding utf8NoBOM }
    exit 0
}
catch {
    if ($null -eq $case) {
        @{ pass=$false; detail=$_.Exception.Message; source_state=$SourceState; origin=$Origin } | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $evidenceRoot "initial-failure.json") -Encoding utf8NoBOM
    }
    Write-Host "FAIL 真实升级：$($_.Exception.Message)"
    exit 1
}
