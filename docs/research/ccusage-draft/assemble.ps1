$ErrorActionPreference = "Stop"
$d = "C:\Users\admin\Desktop\项目\tokenscope\docs\research\ccusage-draft"
$out = "C:\Users\admin\Desktop\项目\tokenscope\docs\research\2026-10-14-ccusage-usage-statistics.md"

$all = New-Object System.Collections.Generic.List[string]

function Add-Raw {
    param([string[]]$Lines)
    if ($script:all.Count -gt 0 -and $script:all[$script:all.Count - 1].Trim() -ne "") {
        $script:all.Add("")
    }
    foreach ($l in $Lines) { $script:all.Add($l) }
}

function Convert-Section {
    param([string[]]$Lines, [string]$ChapterTitle)
    $result = New-Object System.Collections.Generic.List[string]
    $seenH2 = $false
    foreach ($line in $Lines) {
        if ($line -match '^#\s') {
            continue
        }
        elseif ($line -match '^##\s') {
            if (-not $seenH2) {
                $seenH2 = $true
                $result.Add("")
                $result.Add($ChapterTitle)
                $result.Add("")
            }
            else {
                $result.Add(($line -replace '^##\s', '### '))
            }
        }
        else {
            $result.Add($line)
        }
    }
    return $result
}

# 头部（含 §1-3）：原样保留（含其 ## 章节标题）
Add-Raw (Get-Content (Join-Path $d "FINAL-head.md") -Encoding UTF8)

# §4-§8
$map = @(
    @{ File = "S1-main-and-claude.md"; Title = "## 4. 主干与 Claude Code 适配器" },
    @{ File = "S2-codex.md"; Title = "## 5. Codex 适配器" },
    @{ File = "S5-other-adapters.md"; Title = "## 6. 其余 16 个适配器的数据面矩阵" },
    @{ File = "S4-aggregation.md"; Title = "## 7. 聚合、时间与报告形态" },
    @{ File = "S3-pricing.md"; Title = "## 8. 定价数据来源、匹配链与金额计算" }
)
foreach ($entry in $map) {
    $lines = Get-Content (Join-Path $d $entry.File) -Encoding UTF8
    Convert-Section $lines $entry.Title | ForEach-Object { $all.Add($_) }
}

# §9 / §10：拆分 S6，并把章内 `### N.M` 降为 `#### N.M`（避免与章节父节同级）
$s6 = Get-Content (Join-Path $d "S6-config-output-history.md") -Encoding UTF8
$marker = ($s6 | Select-String -Pattern '^##\s+7\.\s+TS' | Select-Object -First 1).LineNumber
if (-not $marker) { throw "S6 未找到 §7 分隔标记" }
$s6a = $s6[0..($marker - 2)]
$s6b = $s6[($marker - 1)..($s6.Count - 1)]
$s6b = $s6b | ForEach-Object {
    if ($_ -match '^###\s') { $_ -replace '^### (7)\.', '### 10.' } else { $_ }
}
# s6a：插入章标题后，把分片原 `## N.` 父节整体降为 `### N.`（保留父节，避免子节失去层级）
$s6aFixed = New-Object System.Collections.Generic.List[string]
$s6aFixed.Add("## 9. 配置、CLI 解析与输出契约")
$s6aFixed.Add("")
foreach ($line in $s6a) {
    if ($line -match '^###\s') { $s6aFixed.Add(($line -replace '^###\s', '#### ')) }
    elseif ($line -match '^##\s') { $s6aFixed.Add(($line -replace '^##\s', '### ')) }
    elseif ($line -match '^#\s') { continue }
    else { $s6aFixed.Add($line) }
}
Add-Raw $s6aFixed
Convert-Section $s6b "## 10. 版本史：v19 TypeScript → v20 Rust 的语义变化" | ForEach-Object { $all.Add($_) }

# §11 / §12 / §13：拆分 tail 文件
$tail = Get-Content (Join-Path $d "FINAL-tail-11-13.md") -Encoding UTF8
$cut = ($tail | Select-String -Pattern '^##\s+13\.' | Select-Object -First 1).LineNumber
if (-not $cut) { throw "tail 未找到 §13 分隔标记" }
$part11 = $tail[0..($cut - 2)]
$part13 = $tail[($cut - 1)..($tail.Count - 1)]
Add-Raw $part11
Add-Raw (Get-Content (Join-Path $d "FINAL-12-compare.md") -Encoding UTF8)
Add-Raw $part13

# §13.5 第 8 轮实测复核实录
Add-Raw (Get-Content (Join-Path $d "FINAL-round8.md") -Encoding UTF8)

# §14 横向专题（第 6 轮补充）
Add-Raw (Get-Content (Join-Path $d "FINAL-supplement.md") -Encoding UTF8)

# 附录
Add-Raw (Get-Content (Join-Path $d "FINAL-appendix.md") -Encoding UTF8)

Set-Content -Path $out -Value $all -Encoding utf8NoBOM

# 统一为 LF（.gitattributes: * text=auto eol=lf）
$rawText = [System.IO.File]::ReadAllText($out)
$rawText = $rawText.Replace("`r`n", "`n").Replace("`r", "`n")
[System.IO.File]::WriteAllText($out, $rawText, (New-Object System.Text.UTF8Encoding($false)))

$stats = Get-Item $out
Write-Output "输出: $($stats.FullName)"
Write-Output "行数: $($all.Count)"
Write-Output "字节: $($stats.Length)"
Write-Output "章节清单:"
$all | Select-String -Pattern '^## ' | ForEach-Object { "$($_.LineNumber): $($_.Line)" }
Write-Output "标题前缺空行（应为 0）:"
for ($i = 1; $i -lt $all.Count; $i++) {
    if ($all[$i] -match '^#{2,4} ' -and $all[$i - 1].Trim() -ne "") { Write-Output "  L$($i+1): $($all[$i])" }
}
