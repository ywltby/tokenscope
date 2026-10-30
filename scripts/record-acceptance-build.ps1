<#
.SYNOPSIS
    Build the acceptance executable and write a controlled build record.

.DESCRIPTION
    AC10 的启动预检不再相信 EXE 里的字符串标记：标记只能预筛，任何普通二进制
    追加标记都可以通过预筛，但无法伪造"由哪条命令、在哪个提交上构建出来的这个
    字节序列"。本脚本就是那个受控构建来源：

      1. 以固定命令构建（--features acceptance,tauri/custom-protocol）；
      2. 记录产物绝对路径、SHA-256、构建命令、features 与 `git rev-parse HEAD`；
      3. 写出 qa-artifacts/acceptance-build/build-record.json。

    native-acceptance.mjs 在创建输出目录、准备隔离根、启动任何进程之前重算磁盘
    产物哈希并与该记录逐字段核对；缺记录、路径不符、命令不合法、提交不一致或
    哈希不匹配一律拒绝启动（零准备、零启动）。

    -SkipBuild 只重新登记现有产物（用于同一次构建后复位证据目录）；它不会改变
    "记录必须来自本仓库这条构建命令"的事实。

    与仓库其它脚本一样：UTF-8 无 BOM、LF。Windows PowerShell 5.1 读无 BOM UTF-8
    会把中文当 ANSI，请用 pwsh 或 `& ([scriptblock]::Create((Get-Content -Raw
    -Encoding UTF8 ...)))`。
#>
[CmdletBinding()]
param(
    [string]$Exe = 'target/release/tokenscope.exe',
    [string]$OutDir = 'qa-artifacts/acceptance-build',
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
# 必须与 frontend/scripts/lib/native-single-instance.mjs 的 ACCEPTANCE_BUILD_COMMAND
# 逐字一致（scripts/test_record_acceptance_build.py 会核对，改一处必须同步另一处）。
$buildCommand = 'cargo build --locked -p tokenscope-app --release --features acceptance,tauri/custom-protocol --bin tokenscope'

if (-not $SkipBuild) {
    Write-Host "[record-acceptance-build] $buildCommand"
    $parts = $buildCommand -split ' '
    $cargoProgram = $parts[0]
    $cargoArguments = $parts[1..($parts.Count - 1)]
    & $cargoProgram @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw "acceptance 构建失败（退出码 $LASTEXITCODE）" }
}

$exeFull = [System.IO.Path]::GetFullPath((Join-Path $repo $Exe))
if (-not (Test-Path -LiteralPath $exeFull -PathType Leaf)) {
    throw "找不到验收产物：$exeFull（先运行不带 -SkipBuild 的本脚本）"
}

$commit = (& git -C $repo rev-parse HEAD).Trim()
if ($commit -notmatch '^[0-9a-f]{40}$') {
    throw "无法确定当前检出的源提交：'$commit'"
}

$sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $exeFull).Hash.ToLowerInvariant()

$record = [ordered]@{
    kind            = 'tokenscope-acceptance-build'
    artifact_path   = $exeFull
    artifact_sha256 = $sha256
    build_command   = $buildCommand
    features        = @('acceptance', 'tauri/custom-protocol')
    source_commit   = $commit
    built_at        = (Get-Date).ToString('o')
    recorder        = 'scripts/record-acceptance-build.ps1'
}

$outFull = [System.IO.Path]::GetFullPath((Join-Path $repo $OutDir))
if (-not (Test-Path -LiteralPath $outFull)) {
    New-Item -ItemType Directory -Force -Path $outFull | Out-Null
}
$recordPath = Join-Path $outFull 'build-record.json'
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$text = (($record | ConvertTo-Json -Depth 4) -replace "`r`n", "`n")
[System.IO.File]::WriteAllText($recordPath, $text, $utf8NoBom)

Write-Host "[record-acceptance-build] 记录 = $recordPath"
Write-Host ("  产物   = {0}" -f $exeFull)
Write-Host ("  SHA256 = {0}" -f $sha256)
Write-Host ("  提交   = {0}" -f $commit)
