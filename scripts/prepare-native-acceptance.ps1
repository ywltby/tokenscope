<#
.SYNOPSIS
    RC10：为原生生产验收准备**隔离根**（合成日志 + 有效价格 + 离线设置）。

.DESCRIPTION
    创建一次验收专用的临时目录树，并把所有落盘目标写进去：
      <Root>\tokenscope\            TokenScope 自有数据目录（settings.toml、pricing.toml、
                                    快照、索引、cache.db、logs、view-cache、窗口状态）
      <Root>\sources\claude\        合成 Claude Code 项目日志
      <Root>\sources\codex\         合成 Codex 会话日志
      <Root>\webview\               WebView2 用户数据（由 acceptance 构建的 identifier/
                                    appDirectoriesOverride 指向这里）
      <Root>\manifest.json          绝对路径清单 + 每个文件的 SHA256/大小/写入时间

    安全约束（与根库 acceptance::bootstrap 的校验一致）：
      - -Root 必须是绝对路径；
      - 不得等于或包含真实的 ~/.tokenscope、~/.claude、~/.codex；
      - 目录已存在且非空时拒绝执行（除非它带着本脚本生成的 manifest.json，
        此时只做校验与补齐，绝不覆盖已有验收产物）；
      - 不修改 HOME/USERPROFILE/APPDATA，不写注册表，不安装自启；
      - 所有文本一律 UTF-8（无 BOM），换行 LF。

.EXAMPLE
    $root = Join-Path ([System.IO.Path]::GetTempPath()) ('tokenscope-native-' + [guid]::NewGuid().ToString('N'))
    .\scripts\prepare-native-acceptance.ps1 -Root $root
    $env:TOKENSCOPE_ACCEPTANCE_ROOT = $root
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Root,

    [switch]$SkipPricing,

    # 合成 Claude 事件总条数。默认 4 条（小样本，够看空/满两种状态）；
    # 需要验证明细分页 / 会话过期（RC02）时传 >200（前端单页 limit=200）。
    [int]$Events = 4
)

$ErrorActionPreference = 'Stop'

function Write-Utf8NewLineLF {
    param([string]$Path, [string]$Content)
    $dir = Split-Path -Parent $Path
    if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    $text = ($Content -replace "`r`n", "`n")
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $text, $utf8NoBom)
}

function Get-Sha256 {
    param([string]$Path)
    (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

# ── 1. 校验 -Root ─────────────────────────────────────────────────
$rootFull = [System.IO.Path]::GetFullPath($Root)
if (-not [System.IO.Path]::IsPathRooted($rootFull)) {
    throw "-Root 必须是绝对路径，收到 $Root"
}

$homeDir = [System.IO.Path]::GetFullPath($env:USERPROFILE)
$protected = @(
    $homeDir,
    (Join-Path $homeDir '.tokenscope'),
    (Join-Path $homeDir '.claude'),
    (Join-Path $homeDir '.codex')
)
foreach ($p in $protected) {
    if ($rootFull -eq $p -or $p.StartsWith($rootFull, [StringComparison]::OrdinalIgnoreCase)) {
        throw "-Root 不得等于或包含真实用户数据目录（与 $p 冲突）——验收必须与真实数据隔离"
    }
}

# 已存在的目录：只允许是"本脚本之前准备过的根"（带 manifest），否则拒绝，
# 避免把别的目录里的文件误当成验收产物覆盖掉。
$manifestPath = Join-Path $rootFull 'manifest.json'
if (Test-Path -LiteralPath $rootFull) {
    $existing = @(Get-ChildItem -LiteralPath $rootFull -Force)
    if ($existing.Count -gt 0 -and -not (Test-Path -LiteralPath $manifestPath)) {
        throw "$rootFull 已存在且非空，但没有 manifest.json——拒绝写入非本次新建的目录"
    }
}

# ── 2. 目录骨架 ───────────────────────────────────────────────────
$dataDir = Join-Path $rootFull 'tokenscope'
$claudeDir = Join-Path $rootFull 'sources\claude'
$codexDir = Join-Path $rootFull 'sources\codex'
$webviewDir = Join-Path $rootFull 'webview'
foreach ($d in @($dataDir, $claudeDir, $codexDir, $webviewDir)) {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
}

$written = [ordered]@{}

function Record-File {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path
    $written[$Path] = [ordered]@{
        path     = $Path
        bytes    = $item.Length
        sha256   = (Get-Sha256 -Path $Path)
        written  = (Get-Date).ToString('o')
    }
}

# ── 3. 合成 agent 日志（真实 agent 日志永不参与） ──────────────────
if ($Events -lt 4) { throw "-Events 至少 4（基线合成事件数），收到 $Events" }
$projectSlug = '-acceptance-workspace-tokenscope'
# 布局必须与适配器默认根一致：<claude 根>\projects\<slug>\*.jsonl
$projectDir = Join-Path (Join-Path $claudeDir 'projects') $projectSlug
$session = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
$entries = @(
    @{ ts = '2026-10-06T01:00:00.000Z'; model = 'claude-sonnet-4-5'; in = 1200; out = 640; cr = 3000; cw = 100 },
    @{ ts = '2026-10-06T02:30:00.000Z'; model = 'claude-sonnet-4-5'; in = 900;  out = 400; cr = 2000; cw = 0 },
    @{ ts = '2026-10-07T03:15:00.000Z'; model = 'gpt-5-codex';       in = 500;  out = 200; cr = 0;    cw = 0 },
    @{ ts = '2026-10-07T04:45:00.000Z'; model = 'mystery-model';     in = 700;  out = 300; cr = 0;    cw = 0 }
)
# 分页/过期验收需要 >200 条明细：按需补齐合成事件（确定性取值，无随机）。
$fillerModels = @('claude-sonnet-4-5', 'gpt-5-codex')
for ($i = $entries.Count; $i -lt $Events; $i++) {
    $entries += @{
        ts    = ('2026-10-{0:d2}T{1:d2}:{2:d2}:00.000Z' -f (5 + ($i % 3)), (1 + ($i % 20)), ($i % 60))
        model = $fillerModels[$i % 2]
        in    = 300 + ($i % 50) * 7
        out   = 120 + ($i % 30) * 3
        cr    = ($i % 4) * 250
        cw    = ($i % 3) * 100
    }
}
$idx = 0
$lines = foreach ($e in $entries) {
    $idx++
    $msg = @{
        id      = "msg-acceptance-$idx"
        role    = 'assistant'
        model   = $e.model
        content = @(@{ type = 'text'; text = 'acceptance synthetic reply' })
        usage   = @{
            input_tokens              = $e.in
            output_tokens             = $e.out
            cache_read_input_tokens   = $e.cr
            cache_creation_input_tokens = $e.cw
        }
    } | ConvertTo-Json -Compress -Depth 8
    (@{
        type = 'assistant'
        isSidechain = $false
        sessionId = $session
        timestamp = $e.ts
        message = ($msg | ConvertFrom-Json)
    } | ConvertTo-Json -Compress -Depth 10)
}
$claudeLog = Join-Path $projectDir "${session}.jsonl"
Write-Utf8NewLineLF -Path $claudeLog -Content (($lines -join "`n") + "`n")
Record-File -Path $claudeLog

# 畸形行样例：坏 JSON 必须被跳过并计入 bad_lines（适配器契约）。
$badLine = Join-Path $projectDir "broken-${session}.jsonl"
$badBody = (($lines[0]) + "`n" + '{"this": "is not a valid line"' + "`n")
Write-Utf8NewLineLF -Path $badLine -Content $badBody
Record-File -Path $badLine

$codexDay = Join-Path (Join-Path $codexDir 'sessions') '2026\10\07'
New-Item -ItemType Directory -Force -Path $codexDay | Out-Null
$codexLines = @(
    (@{ timestamp = '2026-10-07T05:00:00.000Z'; type = 'session_meta'; payload = @{ id = 's-acc-1'; session_id = 's-acc-1'; cwd = 'C:\acceptance\workspace\tokenscope' } } | ConvertTo-Json -Compress -Depth 8),
    (@{ timestamp = '2026-10-07T05:00:00.000Z'; type = 'turn_context'; payload = @{ turn_id = 't-1'; model = 'gpt-5-codex'; cwd = 'C:\acceptance\workspace\tokenscope' } } | ConvertTo-Json -Compress -Depth 8),
    (@{ timestamp = '2026-10-07T05:00:10.000Z'; type = 'event_msg'; payload = @{ type = 'token_count'; info = @{ last_token_usage = @{ input_tokens = 2000; output_tokens = 800; cached_input_tokens = 1000; cache_write_input_tokens = 100; reasoning_output_tokens = 0; total_tokens = 2800 } } } } | ConvertTo-Json -Compress -Depth 10),
    (@{ timestamp = '2026-10-07T05:00:20.000Z'; type = 'event_msg'; payload = @{ type = 'token_count'; info = @{ last_token_usage = @{ input_tokens = 100; output_tokens = 20; cached_input_tokens = 0; cache_write_input_tokens = 0; reasoning_output_tokens = 0; total_tokens = 120 } } } } | ConvertTo-Json -Compress -Depth 10)
)
$codexLog = Join-Path $codexDay 'rollout-acceptance.jsonl'
Write-Utf8NewLineLF -Path $codexLog -Content (($codexLines -join "`n") + "`n")
Record-File -Path $codexLog

# ── 4. 有效价格（外置 pricing.toml） ───────────────────────────────
if (-not $SkipPricing) {
    $pricing = @'
# 验收专用合成价格表（不是任何真实渠道账单）
[[model]]
prefix = "claude-sonnet-4-5"
input = 3.0
output = 15.0
cache_write = 3.75
cache_read = 0.3

[[model]]
prefix = "gpt-5-codex"
input = 1.25
output = 10.0

# 故意留一个未收录模型：mystery-model 必须始终显示"未知"，不得按 0 静默
'@
    $pricingPath = Join-Path $dataDir 'pricing.toml'
    Write-Utf8NewLineLF -Path $pricingPath -Content $pricing
    Record-File -Path $pricingPath
}

# ── 5. 设置：显式两来源目录 + 关闭自动同步（验收期间绝不联网） ─────
$settings = @"
# 验收专用设置（由 scripts/prepare-native-acceptance.ps1 生成）
price_auto_sync = false

[sources.claude]
enabled = true
dir = "$($claudeDir -replace '\\','/')"

[sources.codex]
enabled = true
dir = "$($codexDir -replace '\\','/')"
"@
$settingsPath = Join-Path $dataDir 'settings.toml'
Write-Utf8NewLineLF -Path $settingsPath -Content $settings
Record-File -Path $settingsPath

# ── 6. manifest：绝对路径清单 + 初始散列 ──────────────────────────
$manifest = [ordered]@{
    kind                = 'tokenscope-native-acceptance'
    root                = $rootFull
    data_dir            = $dataDir
    claude_source_dir   = $claudeDir
    claude_projects_dir = (Join-Path $claudeDir 'projects')
    codex_sessions_dir  = (Join-Path $codexDir 'sessions')
    codex_source_dir    = $codexDir
    webview_data_dir    = $webviewDir
    env_var             = @{ name = 'TOKENSCOPE_ACCEPTANCE_ROOT'; value = $rootFull }
    price_auto_sync     = $false
    claude_events       = $Events
    files               = $written.Values
    prepared_at         = (Get-Date).ToString('o')
    script              = 'scripts/prepare-native-acceptance.ps1'
}
$manifest | ConvertTo-Json -Depth 6 | ForEach-Object { Write-Utf8NewLineLF -Path $manifestPath -Content $_ }

Write-Host "验收隔离根已准备：$rootFull"
Write-Host "  数据目录 = $dataDir"
Write-Host "  来源目录 = $claudeDir | $codexDir"
Write-Host "  WebView  = $webviewDir"
Write-Host "  清单     = $manifestPath（$($written.Count) 个文件，含 SHA256）"
Write-Host "下一步：设置环境变量 TOKENSCOPE_ACCEPTANCE_ROOT 后启动 acceptance 构建"
