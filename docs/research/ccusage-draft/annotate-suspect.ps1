$ErrorActionPreference = "Stop"
$d = "C:\Users\admin\Desktop\项目\tokenscope\docs\research\ccusage-draft"
$note = "> 第 8 轮实测复核（§13.5）已闭环本节中与内嵌表键集、运行时行为、格式化相关的条目；此处保留原始清单与当时的判断，逐条状态以 §13.5 为准。"

$targets = @(
    "S1-main-and-claude.md",
    "S2-codex.md",
    "S3-pricing.md",
    "S4-aggregation.md",
    "S5-other-adapters.md",
    "S6-config-output-history.md"
)

foreach ($name in $targets) {
    $path = Join-Path $d $name
    $lines = Get-Content $path -Encoding UTF8
    $idx = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^#{2,4}\s.*(未验证|存疑)') { $idx = $i; break }
    }
    if ($idx -lt 0) { Write-Output ("{0}: no section found" -f $name); continue }
    if ($lines[$idx + 1] -like "*§13.5*") { Write-Output ("{0}: already annotated" -f $name); continue }

    $new = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $new.Add($lines[$i])
        if ($i -eq $idx) {
            $new.Add("")
            $new.Add($note)
        }
    }
    $text = ($new -join "`n") + "`n"
    [System.IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false)))
    Write-Output ("{0}: note inserted after line {1}" -f $name, ($idx + 1))
}
