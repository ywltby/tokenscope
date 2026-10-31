<#
.SYNOPSIS
    Build the acceptance executable and write a controlled build record.

.DESCRIPTION
    AC10 的启动预检不再相信 EXE 里的字符串标记：标记只能预筛，任何普通二进制
    追加标记都可以通过预筛，但无法伪造"由哪条命令、在哪个提交上构建出来的这个
    字节序列"。本脚本就是那个受控构建来源：

      1. 以固定命令构建（--features acceptance,tauri/custom-protocol）；
      2. 记录产物绝对路径、SHA-256、构建命令、features 与 `git rev-parse HEAD`，
         以及源树干净标志（git status --porcelain）；
      3. 写出 qa-artifacts/acceptance-build/build-record.json。

    native-acceptance.mjs 在创建输出目录、准备隔离根、启动任何进程之前重算磁盘
    产物哈希并与该记录逐字段核对；缺记录、路径不符、命令不合法、提交不一致或
    哈希不匹配一律拒绝启动（零准备、零启动）。

    来源证明边界（2026-10-31 收窄）：
      - 本脚本永远执行受控构建命令（增量构建下通常为 no-op）。-SkipBuild 只
        跳过"重新登记之外的手工步骤"，不能跳过命令本身——跳过命令意味着放弃
        "产物字节确由受控命令产生"的证明，等于允许把任意普通 PE 的哈希登记进
        记录。
      - -Exe 只接受受控命令的实际产物 target/release/tokenscope.exe（相对或
        绝对等价路径）。其他路径一律拒绝：预检按登记路径核对哈希，登记一个
        别处路径的哈希就能让那个文件获得 verified。
      - 缺少来源证明的登记（无法确定 git 提交、无法执行受控命令）直接失败，
        不产出记录。

    与仓库其它脚本一样：UTF-8 无 BOM、LF。Windows PowerShell 5.1 读无 BOM UTF-8
    会把中文当 ANSI，请用 pwsh 或 `& ([scriptblock]::Create((Get-Content -Raw
    -Encoding UTF8 ...)))`。
#>
[CmdletBinding()]
param(
    [string]$Exe = 'target/release/tokenscope.exe',
    [string]$OutDir = 'qa-artifacts/acceptance-build',
    # 兼容旧调用形态；受控命令始终执行（增量下为 no-op），见来源证明边界。
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
# 必须与 frontend/scripts/lib/native-single-instance.mjs 的 ACCEPTANCE_BUILD_COMMAND
# 逐字一致（scripts/test_record_acceptance_build.py 会核对，改一处必须同步另一处）。
$buildCommand = 'cargo build --locked -p tokenscope-app --release --features acceptance,tauri/custom-protocol --bin tokenscope'
$controlledArtifact = Join-Path $repo 'target/release/tokenscope.exe'

# 来源证明第 1 步：登记路径必须是受控命令的实际产物。-Exe 指向别处时，登记的
# 哈希无法与受控命令关联（任何普通 PE 都能被登记），直接失败。
$exeFull = [System.IO.Path]::GetFullPath((Join-Path $repo $Exe))
$controlledFull = [System.IO.Path]::GetFullPath($controlledArtifact)
if ($exeFull -ne $controlledFull) {
    throw "-Exe 只接受受控构建的实际产物 $controlledFull（收到 $exeFull）。" +
        "登记其他路径的哈希会让任意文件获得验收身份，拒绝登记。"
}

# 来源证明第 2 步：永远执行受控命令。-SkipBuild 语义收窄为"不重做手工准备"，
# 命令本身必须运行（增量构建通常 no-op，秒级）；否则无法证明产物字节来自本命令。
Write-Host "[record-acceptance-build] $buildCommand"
$parts = $buildCommand -split ' '
$cargoProgram = $parts[0]
$cargoArguments = $parts[1..($parts.Count - 1)]
& $cargoProgram @cargoArguments
if ($LASTEXITCODE -ne 0) { throw "acceptance 构建失败（退出码 $LASTEXITCODE）" }

if (-not (Test-Path -LiteralPath $exeFull -PathType Leaf)) {
    throw "受控命令运行后仍未找到验收产物：$exeFull"
}

$commit = (& git -C $repo rev-parse HEAD).Trim()
if ($commit -notmatch '^[0-9a-f]{40}$') {
    throw "无法确定当前检出的源提交：'$commit'"
}

# 来源证明第 3 步：记录源树状态。CI checkout 是干净树；脏树上的产物无法仅凭
# 提交哈希复现。预检侧要求记录必须携带该字段（缺失即旧格式记录，拒绝），
# 并把 dirty 数量如实带出，由验收使用方自行判断可信度。
$gitStatus = @(& git -C $repo status --porcelain)
$treeClean = ($gitStatus.Count -eq 0)

$sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $exeFull).Hash.ToLowerInvariant()

# 构建者分级（六轮复核）：GitHub Runner（GITHUB_ACTIONS=true）= ci，携带 ci
# 段；本地 = local。预检侧 verified 要求 builder=ci 且预检本身运行在 CI 环境
# ——本地记录无法独立证明来源，一律 unverified。
$isCi = $env:GITHUB_ACTIONS -eq 'true'
$builder = if ($isCi) { 'ci' } else { 'local' }

$record = [ordered]@{
    kind            = 'tokenscope-acceptance-build'
    artifact_path   = $exeFull
    artifact_sha256 = $sha256
    build_command   = $buildCommand
    features        = @('acceptance', 'tauri/custom-protocol')
    source_commit   = $commit
    tree_clean      = $treeClean
    tree_dirty_count = $gitStatus.Count
    builder         = $builder
    built_at        = (Get-Date).ToString('o')
    recorder        = 'scripts/record-acceptance-build.ps1'
}
if ($isCi) {
    $record.ci = [ordered]@{
        run_id      = $env:GITHUB_RUN_ID
        repository  = $env:GITHUB_REPOSITORY
        run_attempt = $env:GITHUB_RUN_ATTEMPT
        run_number  = $env:GITHUB_RUN_NUMBER
    }
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
