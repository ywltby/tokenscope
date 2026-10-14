$repo = "$env:TEMP\ccusage-research\ccusage"
$doc = "C:\Users\admin\Desktop\项目\tokenscope\docs\research\2026-10-14-ccusage-usage-statistics.md"

# 建立文件名 -> 实际行数 的索引
$lineCounts = @{}
$basenameCounts = @{}
Get-ChildItem $repo -Recurse -File -Include *.rs, *.ts, *.json, *.md, *.toml -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch '\\target\\|\\node_modules\\|\\.git\\' } | ForEach-Object {
        $rel = $_.FullName.Replace("$repo\", "").Replace('\', '/')
        $count = (Get-Content $_.FullName -Encoding UTF8 -ErrorAction SilentlyContinue | Measure-Object).Count
        $lineCounts[$rel] = $count
        $base = $_.Name
        if (-not $basenameCounts.ContainsKey($base)) { $basenameCounts[$base] = @() }
        $basenameCounts[$base] += $rel
    }
Write-Output ("索引文件数: " + $lineCounts.Count)

$over = @()
$ambiguous = 0
$checked = 0
$missing = @{}

$lines = Get-Content $doc -Encoding UTF8
for ($i = 0; $i -lt $lines.Count; $i++) {
    foreach ($m in [regex]::Matches($lines[$i], '([A-Za-z0-9_\-/\.]+\.(?:rs|ts|json|toml|md)):(\d+)(?:-(\d+))?')) {
        $file = $m.Groups[1].Value
        $a = [int]$m.Groups[2].Value
        $b = if ($m.Groups[3].Success) { [int]$m.Groups[3].Value } else { $a }
        $target = $null
        if ($file -match '/') {
            foreach ($cand in @($file, "rust/$file")) {
                if ($lineCounts.ContainsKey($cand)) { $target = $cand; break }
            }
            if (-not $target) {
                # 尝试后缀匹配
                $hits = @($lineCounts.Keys | Where-Object { $_.EndsWith("/$file") -or $_ -eq $file })
                if ($hits.Count -eq 1) { $target = $hits[0] }
            }
        } else {
            if ($basenameCounts.ContainsKey($file)) {
                if ($basenameCounts[$file].Count -eq 1) { $target = $basenameCounts[$file][0] }
                else {
                    # 同名歧义：用同名文件的最大行数做宽松上界检查
                    $maxLines = ($basenameCounts[$file] | ForEach-Object { $lineCounts[$_] } | Measure-Object -Maximum).Maximum
                    $checked++
                    if ($b -gt $maxLines) {
                        $over += ("L{0}: {1}:{2}-{3} 越界（同名文件最大 {4} 行）" -f ($i + 1), $file, $a, $b, $maxLines)
                    }
                    continue
                }
            }
        }
        if (-not $target) {
            if (-not $missing.ContainsKey($file)) { $missing[$file] = 0 }
            $missing[$file]++
            continue
        }
        $checked++
        $max = $lineCounts[$target]
        if ($b -gt $max) {
            $over += ("L{0}: {1}:{2}-{3} 越界（{4} 实际 {5} 行）" -f ($i + 1), $file, $a, $b, $target, $max)
        }
    }
}

Write-Output ("已解析并检查的引用: $checked ；同名歧义跳过: $ambiguous")
Write-Output ""
Write-Output "=== 越界引用 ==="
if ($over.Count -eq 0) { Write-Output "  无（全部在文件行数范围内）" } else { $over | Select-Object -First 30 }
Write-Output ""
Write-Output "=== 未能解析的文件名（前 20）==="
$missing.Keys | Sort-Object -Property { -$missing[$_] } | Select-Object -First 20 | ForEach-Object { "  $_ x$($missing[$_])" }
