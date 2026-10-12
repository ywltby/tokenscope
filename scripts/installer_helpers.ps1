#Requires -Version 7
# 共享安装器操作；仅定义函数，导入不执行安装或清理。
function Assert-HostedInstallRunner {
    param([string]$Actions=$env:GITHUB_ACTIONS, [string]$Environment=$env:RUNNER_ENVIRONMENT,
        [string]$TempRoot=$env:RUNNER_TEMP)
    if ($Actions -ne "true" -or $Environment -ne "github-hosted" -or
        -not $TempRoot -or -not (Test-Path -LiteralPath $TempRoot -PathType Container)) {
        throw "真实安装/升级验收仅允许一次性 GitHub-hosted Runner；拒绝修改本机或持久用户目录"
    }
}

function Get-UninstallEntry {
    $keyRoot = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall"
    foreach ($key in Get-ChildItem $keyRoot -ErrorAction SilentlyContinue) {
        $entry = Get-ItemProperty $key.PSPath -ErrorAction SilentlyContinue
        if ($entry.DisplayName -like "TokenScope*") {
            $location = [string]$entry.InstallLocation
            if ($location.Length -ge 2 -and $location.StartsWith('"') -and $location.EndsWith('"')) {
                $entry.InstallLocation = $location.Substring(1,$location.Length-2)
            }
            return $entry
        }
    }
    return $null
}

function Invoke-SilentInstall {
    param([Parameter(Mandatory=$true)][string]$Path,
        [Parameter(Mandatory=$true)][string]$EvidenceRoot)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "安装包不存在：$Path" }
    $process = Start-Process -FilePath $Path -ArgumentList "/S" -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
        New-Item -ItemType Directory -Path $EvidenceRoot -Force | Out-Null
        "installer=$Path`nexit_code=$($process.ExitCode)`nutc=$((Get-Date).ToUniversalTime().ToString('o'))" |
            Set-Content -LiteralPath (Join-Path $EvidenceRoot "install-failure.txt") -Encoding utf8NoBOM
        throw "安装器退出码异常：$($process.ExitCode)"
    }
}

function Reset-InstallData {
    param([Parameter(Mandatory=$true)][string]$DataDir,
        [Parameter(Mandatory=$true)][string]$UserRoot)
    $resolved = [System.IO.Path]::GetFullPath($DataDir)
    $expected = [System.IO.Path]::GetFullPath((Join-Path $UserRoot ".tokenscope"))
    if (-not $resolved.Equals($expected,[StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝清理不属于指定用户 .tokenscope 的目录：$resolved"
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}

function Remove-UpgradeFixtureSources {
    param([Parameter(Mandatory=$true)][string]$SourcesRoot,
        [Parameter(Mandatory=$true)][string]$TempRoot,
        [Parameter(Mandatory=$true)][ValidateSet("present","deleted")][string]$SourceState)
    $resolved = [System.IO.Path]::GetFullPath($SourcesRoot)
    $expected = [System.IO.Path]::GetFullPath((Join-Path $TempRoot ("tokenscope-upgrade-sources-"+$SourceState)))
    if (-not $resolved.Equals($expected,[StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝清理固定升级夹具之外的来源目录：$resolved"
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
    foreach ($name in @("claude","codex")) {
        New-Item -ItemType Directory -Path (Join-Path $resolved $name) -Force | Out-Null
    }
}
