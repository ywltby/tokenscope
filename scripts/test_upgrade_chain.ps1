#Requires -Version 7
# 连续升级链：起点集合按版本升序逐跳覆盖安装，最后装当前包；仅一次性 Runner。
# 两条链（present/deleted）各自独立，deleted 链在最老版本采集后删除来源日志。
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
$evidenceRoot = Join-Path $env:RUNNER_TEMP ("tokenscope-install-evidence/upgrade-chain-"+$SourceState)
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$case = $null
try {
    $metadata = Invoke-UpgradePython -Arguments @((Join-Path $PSScriptRoot "release_assets.py"),"verify",
        "--directory",(Split-Path -Parent $InstallerPath),"--commit",$ExpectedCommit,"--version",$AppVersion)
    $selection = Invoke-UpgradePython -Arguments @((Join-Path $PSScriptRoot "upgrade_baselines.py"),
        "select","--current-version",$AppVersion)
    $chain = @($selection.chain)
    if ($chain.Count -eq 0) { throw "起点集合为空：不能用空矩阵当通过" }
    $origin = $chain[0]
    if ([Version]$AppVersion -le [Version]$origin) { throw "当前版本必须高于链首起点 $origin" }
    $case = New-UpgradeCase -RepoRoot $repoRoot -InstallerPath $InstallerPath -AppVersion $AppVersion `
        -ExpectedCommit $ExpectedCommit -SourceState $SourceState -Origin $origin `
        -EvidenceRoot $evidenceRoot -Metadata $metadata
    Invoke-UpgradeChain -Case $case
    $message = "连续升级链 $(@($case.Chain) -join ' -> ') / $SourceState：逐跳事实、备份、设置与重启幂等全部通过"
    Write-Host "PASS $message"
    if ($env:GITHUB_STEP_SUMMARY) { Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value $message -Encoding utf8NoBOM }
    exit 0
}
catch {
    if ($null -eq $case) {
        @{ pass=$false; detail=$_.Exception.Message; source_state=$SourceState } | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $evidenceRoot "initial-failure.json") -Encoding utf8NoBOM
    }
    Write-Host "FAIL 连续升级链：$($_.Exception.Message)"
    exit 1
}
