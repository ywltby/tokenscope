$f = "C:\Users\admin\Desktop\项目\tokenscope\docs\research\2026-10-14-ccusage-usage-statistics.md"
$lines = Get-Content $f -Encoding UTF8

# 收集章节号（## N.）与小节号（### N.M / #### N.M）
$chapters = @{}
$subsections = @{}
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^##\s+(\d+)\.') { $chapters[[int]$Matches[1]] = $i + 1 }
    if ($lines[$i] -match '^#{3,4}\s+(\d+)\.(\d+)') {
        $key = "$($Matches[1]).$($Matches[2])"
        if (-not $subsections.ContainsKey($key)) { $subsections[$key] = $i + 1 }
    }
    if ($lines[$i] -match '^###\s+13\.(\d+)') { $subsections["13.$($Matches[1])"] = $i + 1 }
}
Write-Output ("章节: " + (($chapters.Keys | Sort-Object) -join ', '))

# 提取引用并检查
$refs = @{}
for ($i = 0; $i -lt $lines.Count; $i++) {
    foreach ($m in [regex]::Matches($lines[$i], '§\s?(\d+)(?:\.(\d+))?')) {
        $key = if ($m.Groups[2].Success) { "$($m.Groups[1].Value).$($m.Groups[2].Value)" } else { $m.Groups[1].Value }
        if (-not $refs.ContainsKey($key)) { $refs[$key] = @() }
        $refs[$key] += ($i + 1)
    }
}
Write-Output ""
Write-Output "=== 引用检查（章号引用） ==="
foreach ($key in ($refs.Keys | Sort-Object)) {
    if ($key -match '^\d+$') {
        $n = [int]$key
        $ok = $chapters.ContainsKey($n) -or $n -le 14
        Write-Output ("  §{0} x{1} -> {2}" -f $key, $refs[$key].Count, ($(if ($ok) { "OK" } else { "缺章" })))
    }
}
Write-Output ""
Write-Output "=== 引用检查（小节引用，可能有歧义，需人工判断） ==="
$suspect = @()
foreach ($key in ($refs.Keys | Sort-Object)) {
    if ($key -match '^\d+\.\d+$') {
        $ok = $subsections.ContainsKey($key)
        $line = $refs[$key][0]
        if (-not $ok) { $suspect += ("  §{0} x{1} 首见 L{2}（该章内无同名小节，可能是章内编号习惯）" -f $key, $refs[$key].Count, $line) }
    }
}
if ($suspect.Count -eq 0) { Write-Output "  （全部命中已登记的小节号）" } else { $suspect | Select-Object -First 25 }
Write-Output ""
Write-Output ("小节引用总数: " + (($refs.Keys | Where-Object { $_ -match '^\d+\.\d+$' } | Measure-Object).Count))
Write-Output ("登记小节号: " + (($subsections.Keys | Sort-Object | Select-Object -First 40) -join ', '))
